//! The `spawn_cluster_size` entity Part hook.

trait Hooks {
    /// Answer the natural spawning pack limit, at `Mob.getMaxSpawnClusterSize`. `mob` is the
    /// finalized mob that has not yet joined the level. `vanilla` is the species' limit. Return
    /// `Some(limit)` to replace it (first answer wins) or `None` to leave it unchanged. A limit
    /// of 0 or 1 ends the pack with the mob already added. This read-only callback must be pure.
    #[hook(
        target = "sunrise_client::game::creatures::Game<'_>::spawn_cluster_size",
        side = "server",
        contract = "replace",
        out = "answer",
        scope = "entity",
        key = "kind",
        dispatch = "direct",
        callable = true,
        neoforge = "SpawnClusterSizeEvent"
    )]
    fn spawn_cluster_size(
        #[engine] kind: &str,
        host: &dyn Host,
        mob: UnspawnedMobRef,
        reason: SpawnReason,
        spawner: Option<SpawnSource>,
        vanilla: u32,
    ) -> Option<u32>;
}
