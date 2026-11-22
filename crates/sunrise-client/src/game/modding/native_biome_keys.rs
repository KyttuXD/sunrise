//! Scoped world-data biome views belong to the same installed snapshot as data maps.
use super::Game;

impl Game<'_> {
    pub(in crate::game) fn biome(
        &self,
        id: &str,
    ) -> Result<mod_api::BiomeView<'_>, mod_api::BiomeKeyError> {
        let maps = self
            .loaded_data_maps()
            .map_err(|_| mod_api::BiomeKeyError::Unavailable)?;
        maps.biome_keys.view(id)
    }

    pub(in crate::game) fn biome_view(
        &self,
        key: &mod_api::BiomeKey,
    ) -> Result<mod_api::BiomeView<'_>, mod_api::BiomeKeyError> {
        let maps = self
            .loaded_data_maps()
            .map_err(|_| mod_api::BiomeKeyError::Unavailable)?;
        maps.biome_keys.view_key(key)
    }

    pub(in crate::game) fn biome_view_id(
        &self,
        view: mod_api::BiomeView<'_>,
    ) -> Result<&str, mod_api::BiomeKeyError> {
        let maps = self
            .loaded_data_maps()
            .map_err(|_| mod_api::BiomeKeyError::Unavailable)?;
        maps.biome_keys.id(view)
    }
}
