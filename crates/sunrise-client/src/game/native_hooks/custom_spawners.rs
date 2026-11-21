use super::*;

impl Game<'_> {
    /// `ModifyCustomSpawnersEvent`: edit the level's list at construction.
    pub(in crate::game) fn native_modify_custom_spawners(
        &self,
        level: mod_api::LevelRef,
        spawners: &mut mod_api::__private::LevelCustomSpawners,
    ) {
        self.fire_modify_custom_spawners(level, spawners);
    }
}
