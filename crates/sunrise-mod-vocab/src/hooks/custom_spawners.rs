//! The custom spawner hook: declaration the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `ModifyCustomSpawnersEvent`: the server level `dimension` builds its custom
    /// spawner list once when the level opens. `spawners` starts with vanilla's five built-ins
    /// (`WanderingTrader`, `Patrol`, `Cat`, `VillageSiege` and `Phantom`, in the order Sunrise
    /// ticks them) for the Overworld, and starts empty for other dimensions (matching vanilla
    /// `MinecraftServer#createLevels`). Take a built-in spawner out with `remove` and it never
    /// ticks, so its random draws are gone with it; append an owned `CustomSpawner` object with
    /// `add` and the level owns that instance for its lifetime, ticking it directly once per
    /// eligible server tick (outside debug worlds, with `spawn_mobs` enabled, matching
    /// `ServerChunkCache#tickChunks`) inside a server transaction and dropping it when the level
    /// unloads or the world closes. Every entry in the list — including `Phantom` and all
    /// appended mod spawners — runs in one ordered walk (`ServerLevel#tickCustomSpawners`). With
    /// no listener, or one that leaves the list alone, the level ticks exactly vanilla's
    /// spawners. Server-side, not cancellable.
    #[notes = r#"## Custom spawners

`modify_custom_spawners` (NeoForge's `ModifyCustomSpawnersEvent`) fires once per server level when it opens, with the level's `CustomSpawners` list: vanilla's five built-ins (`WanderingTrader`, `Patrol`, `Cat`, `VillageSiege`, `Phantom`) in the order Sunrise ticks them for the Overworld, and an empty list for other dimensions (`MinecraftServer#createLevels`). `remove` takes a built-in out so it never ticks (its random draws go with it); `add` appends an owned `CustomSpawner` instance (a struct implementing `CustomSpawner` or a closure) to the end of the list. With no listener, or one that leaves the list alone, nothing changes: the same spawners tick in the same order.

Each registered `CustomSpawner` is owned by that level for its lifetime, ticked directly as its registering mod once per eligible server tick (outside debug worlds, with `spawn_mobs` enabled, matching `ServerChunkCache#tickChunks`) inside a server transaction, and dropped when the level unloads or the world closes. A panic rolls back that tick's writes, disables that spawner type for the registering mod for the rest of the session, and reports the mod and callback through the shared panic queue. Ordinary Host errors do not disable the spawner.

The callback's `spawn_enemies` is the chunk source's stored setting: `spawn_mobs && spawn_monsters` sampled at server startup, on difficulty updates, and when `spawn_monsters` is set (`MinecraftServer#updateMobSpawningFlags`). Changing only `spawn_mobs` changes whether the pass runs, not the stored setting. Peaceful is not a separate filter on that setting.

Sunrise ticks every entry in the custom spawner list in one ordered walk during the chunk-spawning phase (`ServerLevel#tickCustomSpawners`): the built-in spawners in list order (`WanderingTrader`, `Patrol`, `Cat`, `VillageSiege`, `Phantom`; vanilla's own built-in order differs, and moving it is a separate parity change), followed by any mod-appended `CustomSpawner` objects in list order so they see the effects of all preceding entries, including `Phantom`."#]
    #[hook(
        version = 2,
        receiver = "read",
        dispatch = "flat",
        side = "server",
        contract = "listen",
        host = "read",
        neoforge = "ModifyCustomSpawnersEvent",
        cancellable = false
    )]
    fn modify_custom_spawners(dimension: &str, spawners: &mut mod_api::CustomSpawners);
}
