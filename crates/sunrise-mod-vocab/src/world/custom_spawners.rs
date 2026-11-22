//! A level's custom spawners (`CustomSpawner`): the list `ModifyCustomSpawnersEvent` edits.
//!
//! Vanilla `ServerLevel` owns a list of `CustomSpawner` instances (`MinecraftServer#createLevels`
//! populates the Overworld with its five built-ins and other dimensions with an empty list) and
//! ticks each once per eligible server tick (`ServerLevel#tickCustomSpawners`). Mods edit that
//! list once per level in [`Hooks::modify_custom_spawners`](crate::Hooks::modify_custom_spawners),
//! removing built-ins or appending owned [`CustomSpawner`] objects directly.
use crate::callback_owner::CallbackOwner;
use crate::{LevelRef, ServerHost};
use std::{any::TypeId, fmt};

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
/// Register an instance once per level in
/// [`Hooks::modify_custom_spawners`](crate::Hooks::modify_custom_spawners) with
/// [`CustomSpawners::add`]. The level owns the instance for its lifetime and drops it when the
/// level unloads or the world closes. Each tick runs as the registering mod inside a server
/// transaction. A panic rolls back that tick's writes, disables this spawner type for the
/// registering mod for the rest of the session, logs loudly and notifies the client error toast
/// queue. Ordinary Host errors do not disable a spawner.
/// Dropping a spawner also runs under the registering mod's panic containment.
// no wire: owned server-side spawner object with local state and tick callback.
pub trait CustomSpawner: Send + 'static {
    /// Vanilla `CustomSpawner#tick(ServerLevel, boolean)`: run one custom-spawner tick on
    /// `level` outside debug worlds while the `spawn_mobs` game rule is enabled
    /// (`ServerChunkCache#tickChunks`). `spawn_enemies` matches
    /// `ServerLevel#isSpawningMonsters()` (`spawn_mobs` and `spawn_monsters` game rules
    /// enabled, independent of Peaceful difficulty; entity- and spawner-specific difficulty
    /// checks run separately).
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

/// One level-owned mod custom spawner with its registering mod's session owner.
// no wire: owns a server-side spawner object and its process-local mod identity.
// hidden: the game ticks registered spawners; mods register through CustomSpawners::add.
#[doc(hidden)]
#[non_exhaustive]
pub struct RegisteredCustomSpawner {
    spawner: Option<Box<dyn CustomSpawner>>,
    owner: CallbackOwner,
}

impl RegisteredCustomSpawner {
    /// Whether this spawner's registering mod and concrete type are still enabled this session.
    pub fn is_enabled(&self) -> bool {
        self.owner.is_enabled()
    }

    /// The registering mod's identifier.
    pub fn owner_id(&self) -> &'static str {
        self.owner.id()
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

/// One step taken from a level's custom spawner list during `ServerLevel#tickCustomSpawners`.
// no wire: in-process dispatch step carrying a temporarily taken custom spawner.
// hidden: used only by the game's custom spawner tick loop.
#[doc(hidden)]
#[non_exhaustive]
pub enum CustomSpawnerStep {
    /// Run a vanilla built-in spawner.
    Builtin(SpawnerKind),
    /// Run a mod-owned custom spawner, then return it via [`CustomSpawners::put_back`].
    Mod(RegisteredCustomSpawner),
    /// Skip a disabled, poisoned or currently re-entered mod spawner slot.
    Skip,
}

/// Engine checkpoint for rolling back one listener's edits to [`CustomSpawners`].
// no wire: process-local rollback token for one modify_custom_spawners listener.
// hidden: used only by generated modify_custom_spawners dispatch.
// exhaustive: private fields prevent downstream construction.
#[doc(hidden)]
pub struct CustomSpawnersCheckpoint {
    total_len: usize,
    mod_count: usize,
    builtins: Vec<(usize, SpawnerKind)>,
}

enum SpawnerSlot {
    Builtin(SpawnerKind),
    Mod(Option<RegisteredCustomSpawner>),
}

/// A level's custom spawners, in the order they tick: what `ModifyCustomSpawnersEvent` edits.
/// The Overworld starts with vanilla's five built-ins, other dimensions start empty (matching
/// `MinecraftServer#createLevels`), and a spawner a mod adds goes at the end.
// no wire: owns per-level server-side CustomSpawner objects.
// exhaustive: its fields are private, so mods cannot build or destructure it
pub struct CustomSpawners {
    entries: Vec<SpawnerSlot>,
}

impl CustomSpawners {
    /// The Overworld's initial list: vanilla's five spawners in the order Sunrise ticks them.
    // hidden: the game builds each level's list, mods only edit it
    #[doc(hidden)]
    pub fn vanilla() -> Self {
        use SpawnerKind::*;
        Self {
            entries: [WanderingTrader, Patrol, Cat, VillageSiege, Phantom]
                .into_iter()
                .map(SpawnerSlot::Builtin)
                .collect(),
        }
    }

    /// An empty spawner list, as vanilla creates for non-Overworld dimensions
    /// (`MinecraftServer#createLevels`).
    // hidden: the game builds each level's list, mods only edit it
    #[doc(hidden)]
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Appends an owned mod spawner at the end of this level's list. If this spawner type was
    /// already disabled for the registering mod after an earlier panic this session, `spawner`
    /// is dropped under the mod's panic containment and not added.
    pub fn add<S: CustomSpawner>(&mut self, spawner: S) {
        let owner = crate::content::crash::current_native_mod()
            .zip(crate::runtime::installed())
            .map_or_else(
                || CallbackOwner::for_mod("unknown"),
                |(mod_id, installed)| {
                    CallbackOwner::for_installed_type(
                        installed,
                        mod_id,
                        "custom spawner",
                        TypeId::of::<S>(),
                    )
                },
            );
        if !owner.is_enabled() {
            owner.dispose("custom spawner", "CustomSpawner::drop", spawner);
            return;
        }
        self.entries
            .push(SpawnerSlot::Mod(Some(RegisteredCustomSpawner {
                spawner: Some(Box::new(spawner)),
                owner,
            })));
    }

    /// Takes the built-in spawner `kind` out so it never ticks; returns `false` if it was not
    /// in the list.
    pub fn remove(&mut self, kind: SpawnerKind) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|slot| !matches!(slot, SpawnerSlot::Builtin(k) if *k == kind));
        self.entries.len() != before
    }

    /// Whether the built-in spawner `kind` is currently in the list.
    pub fn contains(&self, kind: SpawnerKind) -> bool {
        self.entries
            .iter()
            .any(|slot| matches!(slot, SpawnerSlot::Builtin(k) if *k == kind))
    }

    /// The built-in spawners remaining in this list, in tick order.
    pub fn builtins(&self) -> impl Iterator<Item = SpawnerKind> + '_ {
        self.entries.iter().filter_map(|slot| match slot {
            SpawnerSlot::Builtin(kind) => Some(*kind),
            SpawnerSlot::Mod(_) => None,
        })
    }

    /// Total number of spawners in the list (built-in plus mod-registered).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the list contains no spawners.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Snapshot the list before invoking one `modify_custom_spawners` listener so a panic
    /// rolls back only that listener's removals and additions.
    // hidden: used only by generated modify_custom_spawners dispatch
    #[doc(hidden)]
    pub fn checkpoint(&self) -> CustomSpawnersCheckpoint {
        let mut mod_count = 0;
        let mut builtins = Vec::new();
        for (index, slot) in self.entries.iter().enumerate() {
            match slot {
                SpawnerSlot::Builtin(kind) => builtins.push((index, *kind)),
                SpawnerSlot::Mod(_) => mod_count += 1,
            }
        }
        CustomSpawnersCheckpoint {
            total_len: self.entries.len(),
            mod_count,
            builtins,
        }
    }

    /// Restore the list to `checkpoint`, dropping any mod spawners appended after it.
    // hidden: used only by generated modify_custom_spawners dispatch
    #[doc(hidden)]
    pub fn rollback(&mut self, checkpoint: CustomSpawnersCheckpoint) {
        let mut all_mods = self.entries.drain(..).filter_map(|slot| match slot {
            SpawnerSlot::Builtin(_) => None,
            SpawnerSlot::Mod(spawner) => Some(spawner),
        });
        let preserved_mods: Vec<Option<RegisteredCustomSpawner>> =
            all_mods.by_ref().take(checkpoint.mod_count).collect();
        // Drop any mod spawners added by the rolled-back listener before rebuilding.
        drop(all_mods);

        let mut restored = Vec::with_capacity(checkpoint.total_len);
        let mut builtins = checkpoint.builtins.into_iter().peekable();
        let mut mods = preserved_mods.into_iter();
        for index in 0..checkpoint.total_len {
            if builtins.peek().is_some_and(|&(pos, _)| pos == index) {
                let (_, kind) = builtins.next().unwrap();
                restored.push(SpawnerSlot::Builtin(kind));
            } else if let Some(spawner) = mods.next() {
                restored.push(SpawnerSlot::Mod(spawner));
            }
        }
        self.entries = restored;
    }

    /// Take the entry at `index` for one tick step without holding a borrow on the list.
    // hidden: used only by the game's custom spawner tick loop
    #[doc(hidden)]
    pub fn take_at(&mut self, index: usize) -> Option<CustomSpawnerStep> {
        match self.entries.get_mut(index)? {
            SpawnerSlot::Builtin(kind) => Some(CustomSpawnerStep::Builtin(*kind)),
            SpawnerSlot::Mod(slot) => {
                if slot
                    .as_ref()
                    .is_some_and(RegisteredCustomSpawner::is_enabled)
                {
                    Some(CustomSpawnerStep::Mod(slot.take().unwrap()))
                } else {
                    Some(CustomSpawnerStep::Skip)
                }
            }
        }
    }

    /// Return a mod spawner taken by [`Self::take_at`] back to its slot after ticking.
    // hidden: used only by the game's custom spawner tick loop
    #[doc(hidden)]
    pub fn put_back(&mut self, index: usize, spawner: RegisteredCustomSpawner) {
        if let Some(SpawnerSlot::Mod(slot @ None)) = self.entries.get_mut(index) {
            *slot = Some(spawner);
        }
    }
}

impl fmt::Debug for CustomSpawners {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut list = f.debug_list();
        for slot in &self.entries {
            match slot {
                SpawnerSlot::Builtin(kind) => {
                    list.entry(kind);
                }
                SpawnerSlot::Mod(Some(spawner)) => {
                    list.entry(&format_args!("Mod({})", spawner.owner_id()));
                }
                SpawnerSlot::Mod(None) => {
                    list.entry(&format_args!("Mod(<in-flight>)"));
                }
            }
        }
        list.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    #[test]
    fn vanilla_list_keeps_builtin_order_and_empty_starts_empty() {
        let list = CustomSpawners::vanilla();
        assert_eq!(list.len(), 5);
        assert!(!list.is_empty());
        let builtins: Vec<_> = list.builtins().collect();
        assert_eq!(
            builtins,
            [
                SpawnerKind::WanderingTrader,
                SpawnerKind::Patrol,
                SpawnerKind::Cat,
                SpawnerKind::VillageSiege,
                SpawnerKind::Phantom,
            ]
        );
        let empty = CustomSpawners::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.builtins().count(), 0);
    }

    #[test]
    fn removing_takes_a_builtin_out_once() {
        let mut list = CustomSpawners::vanilla();
        assert!(list.contains(SpawnerKind::Cat));
        assert!(list.remove(SpawnerKind::Cat));
        assert!(!list.contains(SpawnerKind::Cat));
        assert!(!list.remove(SpawnerKind::Cat));
        assert_eq!(list.len(), 4);
    }

    #[test]
    fn checkpoint_and_rollback_restore_builtins_and_drop_added_spawners() {
        struct DropCounter(Arc<AtomicUsize>);
        impl CustomSpawner for DropCounter {
            fn tick(&mut self, _: &mut dyn ServerHost, _: LevelRef, _: bool) {}
        }
        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        let kept_drops = Arc::new(AtomicUsize::new(0));
        let rolled_back_drops = Arc::new(AtomicUsize::new(0));
        let mut list = CustomSpawners::vanilla();
        list.add(DropCounter(kept_drops.clone()));
        assert_eq!(list.len(), 6);

        let checkpoint = list.checkpoint();
        assert!(list.remove(SpawnerKind::Patrol));
        assert!(list.remove(SpawnerKind::Phantom));
        list.add(DropCounter(rolled_back_drops.clone()));
        assert_eq!(list.len(), 5);
        assert_eq!(rolled_back_drops.load(Ordering::Relaxed), 0);

        list.rollback(checkpoint);
        assert_eq!(rolled_back_drops.load(Ordering::Relaxed), 1);
        assert_eq!(kept_drops.load(Ordering::Relaxed), 0);
        assert_eq!(list.len(), 6);
        let builtins: Vec<_> = list.builtins().collect();
        assert_eq!(
            builtins,
            [
                SpawnerKind::WanderingTrader,
                SpawnerKind::Patrol,
                SpawnerKind::Cat,
                SpawnerKind::VillageSiege,
                SpawnerKind::Phantom,
            ]
        );

        drop(list);
        assert_eq!(kept_drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn take_at_and_put_back_skip_reentered_slots() {
        let dropped = Arc::new(AtomicBool::new(false));
        struct Tracked(Arc<AtomicBool>);
        impl CustomSpawner for Tracked {
            fn tick(&mut self, _: &mut dyn ServerHost, _: LevelRef, _: bool) {}
        }
        impl Drop for Tracked {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }

        let mut list = CustomSpawners::empty();
        list.add(Tracked(dropped.clone()));
        let CustomSpawnerStep::Mod(spawner) = list.take_at(0).unwrap() else {
            panic!("expected mod spawner at slot 0");
        };
        assert!(matches!(list.take_at(0), Some(CustomSpawnerStep::Skip)));
        list.put_back(0, spawner);
        assert!(!dropped.load(Ordering::Relaxed));
        drop(list);
        assert!(dropped.load(Ordering::Relaxed));
    }
}
