use super::*;

impl Game<'_> {
    /// `ModifyCustomSpawnersEvent`: edit the level's list at construction.
    pub(in crate::game) fn native_modify_custom_spawners(
        &self,
        dimension: mod_api::DimensionKey,
        spawners: &mut dyn mod_api::CustomSpawners,
    ) {
        self.fire_modify_custom_spawners(dimension, spawners);
    }
}
