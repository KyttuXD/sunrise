//! Owned server-level custom spawners and the editor lent to each registration listener.

use crate::{LevelRef, ServerHost, callback_owner::CallbackOwner};

/// A built-in custom spawner in the Overworld's starting list.
#[derive(sunrise_mod_api_macros::Wire, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SpawnerKind {
    /// `PhantomSpawner`: phantoms for players who have not slept.
    Phantom,
    /// `PatrolSpawner`: pillager patrols.
    Patrol,
    /// `CatSpawner`: cats in villages and swamp huts.
    Cat,
    /// `VillageSiege`: zombie sieges on a village at night.
    VillageSiege,
    /// `WanderingTraderSpawner`: the wandering trader and its llamas.
    WanderingTrader,
}

const fn bit(kind: SpawnerKind) -> u8 {
    1 << kind as u8
}

// no wire: a native spawner owns code and mutable state for one live server level.
/// A custom spawner owned by one server level, like vanilla's `CustomSpawner`.
pub trait CustomSpawner: Send + 'static {
    /// Run once per eligible server tick while `spawn_mobs` is enabled. `spawn_enemies`
    /// is true when `spawn_monsters` is enabled and the difficulty is not Peaceful.
    /// This instance's state carries over between ticks. A panic disables the instance's
    /// callback and is logged and shown in an error toast; a phase refusal is logged without
    /// disabling it.
    fn tick(&mut self, host: &mut dyn ServerHost, level: LevelRef, spawn_enemies: bool);
}

// no wire: the editor borrows the live server level's spawner list and stages native objects.
// exhaustive: private fields make this a loan from dispatch, not something a mod can construct.
/// Edit this level's spawners during [`Hooks::modify_custom_spawners`](crate::Hooks::modify_custom_spawners).
/// A listener sees the preceding listeners' edits. Its own edits are applied only if it completes
/// without panicking or making a Host call that is refused by the current phase.
pub struct CustomSpawners<'a> {
    list: &'a mut LevelCustomSpawners,
    level: LevelRef,
    owner: &'static str,
    removed: u8,
    appended: Vec<BoundCustomSpawner>,
}

impl CustomSpawners<'_> {
    /// Append an owned spawner to this level. The level drops the instance when it closes.
    pub fn add(&mut self, spawner: impl CustomSpawner) {
        self.appended.push(BoundCustomSpawner {
            owner: CallbackOwner::for_mod(self.owner),
            level: self.level,
            spawner: Some(Box::new(spawner)),
        });
    }

    /// Whether the list still contains this built-in, taking this listener's edits into account.
    pub fn contains(&self, kind: SpawnerKind) -> bool {
        self.list.contains(kind) && self.removed & bit(kind) == 0
    }

    /// Remove this built-in without changing the order of the others. Returns whether it was present.
    pub fn remove(&mut self, kind: SpawnerKind) -> bool {
        let present = self.contains(kind);
        self.removed |= bit(kind);
        present
    }

    pub(crate) fn commit(mut self) {
        self.list.builtins &= !self.removed;
        self.list.native.append(&mut self.appended);
    }
}

// no wire: engine-owned list of live spawner objects.
/// The spawners stored with the current or parked server level.
#[derive(Default)]
pub struct LevelCustomSpawners {
    builtins: u8,
    /// The level's custom objects in registration order, lent to its tick loop.
    pub native: Vec<BoundCustomSpawner>,
}

impl LevelCustomSpawners {
    /// The Overworld's five built-ins (`MinecraftServer.createLevels`). Other levels start empty.
    pub fn overworld() -> Self {
        use SpawnerKind::*;
        Self {
            builtins: bit(WanderingTrader)
                | bit(Patrol)
                | bit(Cat)
                | bit(VillageSiege)
                | bit(Phantom),
            ..Self::default()
        }
    }

    /// Whether this level has a built-in spawner.
    pub fn contains(&self, kind: SpawnerKind) -> bool {
        self.builtins & bit(kind) != 0
    }

    pub(crate) fn edit(&mut self, owner: &'static str, level: LevelRef) -> CustomSpawners<'_> {
        CustomSpawners {
            list: self,
            level,
            owner,
            removed: 0,
            appended: Vec::new(),
        }
    }
}

// no wire: native object bound to its registering mod and the open level.
/// One object in a level's custom spawner list.
pub struct BoundCustomSpawner {
    owner: CallbackOwner,
    level: LevelRef,
    spawner: Option<Box<dyn CustomSpawner>>,
}

impl BoundCustomSpawner {
    /// Invoke only this object's callback under its registering mod, without another lookup.
    pub fn tick<H: ServerHost>(&mut self, host: &mut H, spawn_enemies: bool) {
        if !self.owner.is_enabled() {
            return;
        }
        // Only Drop empties this slot, to dispose of the object under its owner's boundary.
        let spawner = self.spawner.as_deref_mut().expect("live custom spawner");
        let _ = crate::run_transaction(host, |host, _| {
            self.owner
                .call("custom spawner", "CustomSpawner::tick", || {
                    spawner.tick(host, self.level, spawn_enemies);
                })
        });
    }
}

impl Drop for BoundCustomSpawner {
    fn drop(&mut self) {
        self.owner
            .dispose("custom spawner", "CustomSpawner::drop", self.spawner.take());
    }
}
