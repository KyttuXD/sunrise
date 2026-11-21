//! A `ServerLevel`'s built-in and native custom spawners.

use super::{Dim, Game};
use mod_api::__private::game::CallbackOwner;
use mod_api::{CustomSpawner, CustomSpawners, SpawnerKind};

const BUILTINS: [SpawnerKind; 5] = [
    SpawnerKind::WanderingTrader,
    SpawnerKind::Patrol,
    SpawnerKind::Cat,
    SpawnerKind::VillageSiege,
    SpawnerKind::Phantom,
];

#[derive(Default)]
pub(super) struct LevelCustomSpawners {
    builtins: u8,
    native: Vec<OwnedSpawner>,
}

impl CustomSpawners for LevelCustomSpawners {
    fn add_custom_spawner(&mut self, spawner: Box<dyn CustomSpawner>) {
        let id = mod_api::content::crash::current_native_mod();
        let Some(owner) = id.and_then(CallbackOwner::new) else {
            log::error!(
                "modify_custom_spawners: no installed owner for spawner registration ({id:?})"
            );
            return;
        };
        self.native.push(OwnedSpawner {
            owner,
            spawner: Some(spawner),
        });
    }

    fn contains(&self, kind: SpawnerKind) -> bool {
        self.builtins & (1 << kind as u8) != 0
    }

    fn remove(&mut self, kind: SpawnerKind) -> bool {
        let present = self.contains(kind);
        self.builtins &= !(1 << kind as u8);
        present
    }
}

struct OwnedSpawner {
    owner: CallbackOwner,
    spawner: Option<Box<dyn CustomSpawner>>,
}

impl Drop for OwnedSpawner {
    fn drop(&mut self) {
        self.owner
            .dispose("custom spawner", "drop", self.spawner.take());
    }
}

impl Game<'_> {
    /// At the end of level construction, after the level is available to the read Host.
    pub(super) fn build_custom_spawners(&self, dim: Dim) -> LevelCustomSpawners {
        // MinecraftServer.createLevels supplies the built-ins only to the Overworld.
        let mut spawners = LevelCustomSpawners::default();
        if dim == Dim::Overworld {
            spawners.builtins = BUILTINS
                .into_iter()
                .map(|kind| 1 << kind as u8)
                .fold(0, |a, b| a | b);
        }
        self.native_modify_custom_spawners(dim.key(), &mut spawners);
        spawners
    }

    /// `ServerLevel.tickCustomSpawners`: keep the existing order and RNG of the built-ins.
    /// Phantom still ticks after the mobs, in `spawn_phantoms`.
    pub(super) fn tick_custom_spawners(&mut self) {
        use sunrise_client::gamerules::vanilla::SPAWN_MONSTERS;

        let dimension = self.dim.key();
        let spawn_enemies = self.ui.rules.bool_at(SPAWN_MONSTERS) && self.ui.difficulty != 0;
        for kind in BUILTINS {
            if !self.creatures.custom_spawners.contains(kind) {
                continue;
            }
            match kind {
                SpawnerKind::WanderingTrader => self.tick_wandering_trader(),
                SpawnerKind::Patrol => self.tick_patrol_spawner(),
                SpawnerKind::Cat => self.tick_cat_spawner(),
                SpawnerKind::VillageSiege => self.tick_village_siege(),
                // Phantom runs later; no other built-ins are in this list.
                _ => {}
            }
        }
        // Lend the rest of the game to the Host while the resident list is being ticked.
        // Taking the Vec header does not allocate or move any of the objects.
        let mut native = std::mem::take(&mut self.creatures.custom_spawners.native);
        for entry in &mut native {
            if !entry.owner.is_enabled() {
                continue;
            }
            let spawner = entry.spawner.as_deref_mut().expect("live custom spawner");
            let _ = mod_api::run_transaction(self, |game, _| {
                entry
                    .owner
                    .call("custom spawner", "CustomSpawner::tick", || {
                        spawner.tick(game, dimension, spawn_enemies);
                    })
            });
        }
        self.creatures.custom_spawners.native = native;
    }
}

#[cfg(test)]
#[path = "tests/custom_spawners_tests.rs"]
mod tests;
