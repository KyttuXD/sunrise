//! The `spawn_cluster_size` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// How many mobs one spawn group of this mob's type may grow to (`Mob.getMaxSpawnClusterSize`),
    /// asked where the spawner has just added a mob to a group and compares the group's size with
    /// the limit: natural spawning's packs. `mob` is the mob just spawned, finalized and in no
    /// level yet, to read through `host`; `reason` and `spawner` say why and from what it spawned
    /// (`spawner` is none but for a spawner block's mobs); `vanilla` is the limit the species has.
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
        isolate = true,
        scope = "entity",
        key = "mob",
        neoforge = "SpawnClusterSizeEvent"
    )]
    fn spawn_cluster_size(
        mob: UnspawnedMobRef,
        reason: SpawnReason,
        spawner: Option<SpawnSource>,
        vanilla: u32,
        limit: &mut Option<u32>,
    );
}
