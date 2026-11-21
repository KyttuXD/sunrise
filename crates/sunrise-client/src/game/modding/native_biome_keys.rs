//! Borrow registered biomes from the installed server registry.
use super::Game;
use mod_api::{BiomeRef, LevelReadError};

impl Game<'_> {
    pub(in crate::game) fn biome(&self, id: &str) -> Result<Option<BiomeRef<'_>>, LevelReadError> {
        let tables = self
            .world_tables
            .as_ref()
            .filter(|_| self.is_logical_server())
            .ok_or(LevelReadError::Unavailable)?;
        let biomes = &tables.biomes;
        Ok(biomes
            .index
            .get(id)
            .and_then(|&slot| biomes.biome_ref(slot)))
    }
}
