//! The custom spawner hook: declaration the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `ModifyCustomSpawnersEvent`: edit this logical server level's list once when
    /// it opens. The Overworld starts with the five vanilla built-ins, in Sunrise's existing
    /// order; other levels start empty. Inspect `spawners.entries()`, remove an entry with
    /// `remove_at` (or a built-in by kind with `remove`), or append a labeled `CustomSpawner`
    /// with `add`. A refused or panicking listener's edits never apply. Not cancellable.
    #[notes = r#"## Custom spawners

`modify_custom_spawners` (NeoForge's `ModifyCustomSpawnersEvent`) fires once per server level when it opens. The Overworld starts with the five built-ins (`WanderingTrader`, `Patrol`, `Cat`, `VillageSiege`, `Phantom`) in Sunrise's existing order; other dimensions start empty (`MinecraftServer#createLevels`). The borrowed `spawners` editor exposes all entries, including the mod id and label of objects added by earlier listeners. `remove_at` takes an entry out by its current list position; `remove` takes out a built-in by kind. `add` appends a labeled, owned `CustomSpawner` instance. The metadata is for registration only, not tick dispatch. Edits apply only if the listener succeeds; a panic disables that listener for the session, logs the mod's name and queues an error toast.

The level owns each accepted instance until removal during registration, level unload, or world close. The registered objects tick directly, once per eligible server tick, after the built-ins (including the phantom pass), in registration order. As in `ServerChunkCache#tickChunks`, debug worlds and ticks with `spawn_mobs` off do not run these callbacks. `spawn_enemies` is the chunk source's setting updated by `MinecraftServer#updateMobSpawningFlags` from `ServerLevel#isSpawningMonsters`; it is independent of difficulty. A phase refusal reports and rolls back only that instance's writes for the tick. A panic also disables only that instance's callback, logs the mod's name and queues an error toast."#]
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
    fn modify_custom_spawners(dimension: &str, spawners: mod_api::CustomSpawners<'_>);
}
