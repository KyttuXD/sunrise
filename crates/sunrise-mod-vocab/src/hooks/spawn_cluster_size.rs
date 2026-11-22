//! The `spawn_cluster_size` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `SpawnClusterSizeEvent`: how many mobs one natural spawn group may contain.
    /// Runs only at the group-size check in `NaturalSpawner.spawnCategoryForPosition`
    /// (`EventHooks.getMaxSpawnClusterSize`). Read the finalized `mob` through `host`;
    /// `vanilla` is the species' `Mob.getMaxSpawnClusterSize` result.
    /// Answer `Some(limit)` to replace it and `None` to leave it to the next part and then to
    /// vanilla's. The first part to answer wins, in dispatch order, and the group stops growing
    /// once its size reaches the limit, so a limit of 0 or 1 ends it with the mob just spawned.
    /// The host is the read-only one: a listener runs on replayed ticks and must be a pure
    /// function of its inputs.
    #[hook(
        target = "sunrise_client::game::native_hooks::spawn_cluster_size::Game<'_>::native_spawn_cluster_size",
        receiver = "read",
        dispatch = "api_carrier",
        side = "server",
        host = "read",
        contract = "replace",
        out = "limit",
        scope = "entity",
        key = "mob",
        neoforge = "SpawnClusterSizeEvent"
    )]
    fn spawn_cluster_size(mob: UnspawnedMobRef, vanilla: u32, limit: &mut Option<u32>);
}
