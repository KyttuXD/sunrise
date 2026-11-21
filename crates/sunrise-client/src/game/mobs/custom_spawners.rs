//! A `ServerLevel`'s built-in and native custom spawners.

use super::{Dim, Game};
use mod_api::{__private::LevelCustomSpawners, SpawnerKind};

impl Game<'_> {
    /// At the end of level construction, after the level is available to the read Host.
    pub(super) fn build_custom_spawners(&self, dim: Dim) -> LevelCustomSpawners {
        // MinecraftServer.createLevels supplies the built-ins only to the Overworld.
        let mut spawners = if dim == Dim::Overworld {
            LevelCustomSpawners::overworld()
        } else {
            LevelCustomSpawners::default()
        };
        if mod_api::__private::listened(mod_api::HookId::ModifyCustomSpawners) {
            self.native_modify_custom_spawners(
                self.level_ref(self.lend_epoch(), dim.key()),
                &mut spawners,
            );
        }
        spawners
    }

    /// `ServerLevel.tickCustomSpawners`: keep the existing order and RNG of the built-ins.
    /// Phantom still ticks after the mobs, in `spawn_phantoms`.
    pub(super) fn tick_custom_spawners(&mut self) {
        use sunrise_client::gamerules::vanilla::SPAWN_MONSTERS;

        let spawn_enemies = self.ui.rules.bool_at(SPAWN_MONSTERS) && self.ui.difficulty != 0;
        if self
            .creatures
            .custom_spawners
            .contains(SpawnerKind::WanderingTrader)
        {
            self.tick_wandering_trader();
        }
        if self.creatures.custom_spawners.contains(SpawnerKind::Patrol) {
            self.tick_patrol_spawner();
        }
        if self.creatures.custom_spawners.contains(SpawnerKind::Cat) {
            self.tick_cat_spawner();
        }
        if self
            .creatures
            .custom_spawners
            .contains(SpawnerKind::VillageSiege)
        {
            self.tick_village_siege();
        }
        // Lend the rest of the game to the Host while the resident list is being ticked.
        // Taking the Vec header needs no lookup, allocation or movement of the objects.
        let mut native = std::mem::take(&mut self.creatures.custom_spawners.native);
        for spawner in &mut native {
            spawner.tick(self, spawn_enemies);
        }
        self.creatures.custom_spawners.native = native;
    }
}

#[cfg(test)]
#[path = "tests/custom_spawners_tests.rs"]
mod tests;
