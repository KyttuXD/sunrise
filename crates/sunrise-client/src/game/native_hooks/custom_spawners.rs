use super::*;

/// The hook point. Does nothing in Sunrise without a listener.
impl Game<'_> {
    /// `ModifyCustomSpawnersEvent`: the logical server level `dimension` builds its custom
    /// spawner list once when it opens. `spawners` starts with vanilla's built-ins for the
    /// Overworld and empty for other dimensions; a mod can remove entries or append owned
    /// `CustomSpawner` objects without taking ownership of the list.
    pub(in crate::game) fn native_modify_custom_spawners(
        &self,
        dimension: &str,
        spawners: &mut mod_api::__private::CustomSpawnerList,
    ) {
        self.fire_modify_custom_spawners(dimension, spawners);
    }
}
