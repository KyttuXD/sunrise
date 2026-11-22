use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// `Mob.getMaxSpawnClusterSize` as the mob's entity parts answer it. Server-side.
    pub(in crate::game) fn native_spawn_cluster_size(
        &self,
        entity_kind: &str,
        call: mod_api::__private::SpawnClusterSizeCall<'_>,
    ) {
        self.fire_spawn_cluster_size(entity_kind, call);
    }
}
