use super::*;

/// The hook point. Does nothing in Sunrise without a listener.
impl Game<'_> {
    /// `ModifyCustomSpawnersEvent`: the logical server level `dimension` builds its custom
    /// spawner list once when it opens. `spawners` starts with vanilla's built-ins for the
    /// Overworld and empty for other dimensions; a mod removes built-ins or adds owned
    /// `CustomSpawner` objects.
    pub(in crate::game) fn native_modify_custom_spawners(
        &self,
        dimension: &str,
        spawners: &mut mod_api::CustomSpawners,
    ) {
        self.fire_modify_custom_spawners(dimension, spawners);
    }
}
