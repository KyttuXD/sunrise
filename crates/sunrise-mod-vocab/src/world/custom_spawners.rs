//! A level's custom spawners (`ServerLevel#tickCustomSpawners`) and the borrowed registration
//! editor used by [`Hooks::modify_custom_spawners`](crate::Hooks::modify_custom_spawners).
use crate::callback_owner::CallbackOwner;
use crate::{LevelRef, ServerHost};
use std::fmt;

/// A built-in spawner vanilla runs for the Overworld (`MinecraftServer#createLevels`).
// no wire: server-side custom spawner selector, not a serialized payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

/// An owned custom spawner ticked once per eligible server level tick
/// (`net.minecraft.world.level.CustomSpawner`).
///
/// Register an instance with [`CustomSpawners::add`] in
/// [`Hooks::modify_custom_spawners`](crate::Hooks::modify_custom_spawners). The level owns it
/// until it is removed during registration, the level unloads, or the world closes. Each tick
/// runs as the registering mod in a server transaction. A phase refusal reports and rolls back
/// that tick's writes without disabling the callback. A panic also disables this instance's
/// callback, logs the mod's name and queues an error toast. Other instances are unaffected.
/// Disposal also runs under the registering mod's panic containment.
// no wire: owned server-side spawner object with local state and tick callback.
pub trait CustomSpawner: Send + 'static {
    /// Run one tick on `level`. `spawn_enemies` is the chunk source's spawn setting, set from
    /// `ServerLevel#isSpawningMonsters` (`spawn_mobs` and `spawn_monsters`) at the server's spawn
    /// settings refresh. It does not check difficulty; individual spawners and entities do that.
    fn tick(&mut self, host: &mut dyn ServerHost, level: LevelRef, spawn_enemies: bool);
}

impl<F> CustomSpawner for F
where
    F: FnMut(&mut dyn ServerHost, LevelRef, bool) + Send + 'static,
{
    fn tick(&mut self, host: &mut dyn ServerHost, level: LevelRef, spawn_enemies: bool) {
        self(host, level, spawn_enemies);
    }
}

/// An entry as seen during registration, without access to its callable object.
// no wire: borrowed metadata from a level's in-process registration list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CustomSpawnerEntry<'a> {
    /// One of vanilla's spawners.
    Builtin(SpawnerKind),
    /// An owned spawner added by a mod; labels need not be unique.
    #[non_exhaustive]
    Registered {
        /// The registering mod's manifest id.
        mod_id: &'static str,
        /// The label the registering mod passed to [`CustomSpawners::add`].
        label: &'a str,
    },
}

/// A borrowed editor of a server level's custom spawner list during `ModifyCustomSpawnersEvent`.
/// Earlier listeners' accepted edits are visible. This listener's edits apply only if its call
/// succeeds, and cannot take ownership of or tick an entry.
// no wire: borrows in-process registrations for one listener invocation.
// exhaustive: private fields prevent mods from constructing an editor.
pub struct CustomSpawners<'a> {
    owner: &'static str,
    earlier: &'a [RegisteredCustomSpawner],
    edits: &'a mut SpawnerEdits,
}

impl CustomSpawners<'_> {
    /// Append an owned spawner at the end of the list. `label` is registration metadata for
    /// listeners inspecting [`Self::entries`]; it is not used when ticking and need not be unique.
    pub fn add(&mut self, label: &str, spawner: impl CustomSpawner) {
        self.edits.added.push(RegisteredCustomSpawner {
            label: label.into(),
            spawner: Some(Box::new(spawner)),
            owner: CallbackOwner::for_mod(self.owner),
        });
    }

    /// Remove the built-in `kind` so it never ticks; return whether it was in the list.
    pub fn remove(&mut self, kind: SpawnerKind) -> bool {
        let Some(slot) = self
            .edits
            .builtins
            .iter_mut()
            .find(|slot| **slot == Some(kind))
        else {
            return false;
        };
        *slot = None;
        true
    }

    /// Whether the built-in `kind` is currently in the list.
    pub fn contains(&self, kind: SpawnerKind) -> bool {
        self.edits.builtins.contains(&Some(kind))
    }

    /// The remaining built-in spawners, in tick order.
    pub fn builtins(&self) -> impl Iterator<Item = SpawnerKind> + '_ {
        self.edits.builtins.iter().copied().flatten()
    }

    /// Inspect all entries in tick order, including objects added by earlier listeners.
    /// The zero-based position in this iterator can be passed to [`Self::remove_at`].
    pub fn entries(&self) -> impl Iterator<Item = CustomSpawnerEntry<'_>> + '_ {
        self.builtins().map(CustomSpawnerEntry::Builtin).chain(
            self.earlier
                .iter()
                .enumerate()
                .filter(|(i, _)| !self.edits.removed.contains(i))
                .map(|(_, entry)| entry.description())
                .chain(
                    self.edits
                        .added
                        .iter()
                        .map(RegisteredCustomSpawner::description),
                ),
        )
    }

    /// Remove the entry at `index` from the current list, returning whether one was present.
    /// As with a vector, later entries shift for subsequent edits. No callable is returned.
    pub fn remove_at(&mut self, index: usize) -> bool {
        let builtin_count = self.builtins().count();
        let Some(index) = index.checked_sub(builtin_count) else {
            if let Some(slot) = self
                .edits
                .builtins
                .iter_mut()
                .filter(|slot| slot.is_some())
                .nth(index)
            {
                *slot = None;
                return true;
            }
            return false;
        };
        let mut earlier = (0..self.earlier.len()).filter(|i| !self.edits.removed.contains(i));
        let earlier_count = self.earlier.len() - self.edits.removed.len();
        if let Some(i) = earlier.nth(index) {
            self.edits.removed.push(i);
            return true;
        }
        let index = index - earlier_count;
        if index < self.edits.added.len() {
            drop(self.edits.added.remove(index));
            true
        } else {
            false
        }
    }

    /// Total number of built-in and registered spawners currently in the list.
    pub fn len(&self) -> usize {
        self.builtins().count() + self.earlier.len() - self.edits.removed.len()
            + self.edits.added.len()
    }

    /// Whether there are no spawners in the current list.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for CustomSpawners<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.entries()).finish()
    }
}

struct SpawnerEdits {
    builtins: [Option<SpawnerKind>; 5],
    removed: Vec<usize>,
    added: Vec<RegisteredCustomSpawner>,
}

/// Engine storage for one level's built-ins and registered custom spawners.
// no wire: owns a level's native spawner objects.
// hidden: mods only receive the borrowed registration editor.
#[doc(hidden)]
#[non_exhaustive]
pub struct CustomSpawnerList {
    builtins: [Option<SpawnerKind>; 5],
    /// The ordered tail, lent to the tick loop without borrowing the game through a callback.
    pub registered: Vec<RegisteredCustomSpawner>,
}

impl CustomSpawnerList {
    /// Start with Sunrise's existing built-in order for the Overworld; elsewhere start empty.
    pub fn new(overworld: bool) -> Self {
        use SpawnerKind::*;
        Self {
            builtins: [WanderingTrader, Patrol, Cat, VillageSiege, Phantom]
                .map(|kind| overworld.then_some(kind)),
            registered: Vec::new(),
        }
    }

    /// Remaining built-in spawners, copied to allow ticking without a borrow of the list.
    pub fn builtins(&self) -> impl Iterator<Item = SpawnerKind> + use<> {
        self.builtins.into_iter().flatten()
    }

    /// Run one registration listener and commit only its accepted edits. Prior objects stay
    /// owned by the level throughout the call, so a refusal cannot remove or replace them.
    pub(crate) fn edit(&mut self, owner: &'static str, listener: impl FnOnce(CustomSpawners<'_>)) {
        let mut edits = SpawnerEdits {
            builtins: self.builtins,
            removed: Vec::new(),
            added: Vec::new(),
        };
        if !CallbackOwner::call_hook(
            crate::HookId::ModifyCustomSpawners,
            owner,
            "modify_custom_spawners",
            || {
                listener(CustomSpawners {
                    owner,
                    earlier: &self.registered,
                    edits: &mut edits,
                });
            },
        ) {
            return;
        }
        self.builtins = edits.builtins;
        if !edits.removed.is_empty() {
            edits.removed.sort_unstable();
            let mut removed = edits.removed.into_iter().peekable();
            let mut index = 0;
            self.registered.retain(|_| {
                let keep = removed.next_if_eq(&index).is_none();
                index += 1;
                keep
            });
        }
        self.registered.extend(edits.added);
    }
}

/// One level-owned mod spawner with its own callback owner and poison state.
// no wire: owns a server-side spawner object and its process-local mod identity.
// hidden: only the game ticks registered spawners.
#[doc(hidden)]
#[non_exhaustive]
pub struct RegisteredCustomSpawner {
    label: Box<str>,
    spawner: Option<Box<dyn CustomSpawner>>,
    owner: CallbackOwner,
}

impl RegisteredCustomSpawner {
    fn description(&self) -> CustomSpawnerEntry<'_> {
        CustomSpawnerEntry::Registered {
            mod_id: self.owner.id(),
            label: &self.label,
        }
    }

    /// Whether this instance's callback is enabled.
    pub fn is_enabled(&self) -> bool {
        self.owner.is_enabled()
    }

    /// Tick this spawner once as its registering mod.
    pub fn tick(
        &mut self,
        host: &mut dyn ServerHost,
        level: LevelRef,
        spawn_enemies: bool,
    ) -> Result<(), String> {
        let spawner = self.spawner.as_deref_mut().expect("live custom spawner");
        self.owner
            .call("custom spawner", "CustomSpawner::tick", || {
                spawner.tick(host, level, spawn_enemies)
            })
    }
}

impl Drop for RegisteredCustomSpawner {
    fn drop(&mut self) {
        if let Some(spawner) = self.spawner.take() {
            self.owner
                .dispose("custom spawner", "CustomSpawner::drop", spawner);
        }
    }
}
