//! The custom spawner registration hook (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `ModifyCustomSpawnersEvent`: a logical server level has been constructed.
    /// The Overworld starts with its built-in spawners; other levels start empty. Remove a built-in
    /// with `spawners.remove(kind)` or append an owned `CustomSpawner` with `spawners.add(spawner)`.
    /// The level retains each object until it closes. This event is not cancellable and fires
    /// before `level_load`, not during ticking.
    #[notes = r#"## Custom spawners

`modify_custom_spawners` fires once per server level lifetime, before `level_load`, even if mob spawning is off. The Overworld starts with its built-in spawners; other levels start empty. `spawners.remove(kind)` takes a built-in out without changing the relative order or tick phase of the others. `spawners.add(spawner)` appends an owned `CustomSpawner`. A failed registration listener's edits are not applied; later listeners still run.

A level calls each appended object's `tick` only while `spawn_mobs` is enabled, in registration order. The object retains its own state between ticks. It receives the level's `LevelRef`, a `ServerHost`, and `spawn_enemies`, which is true when `spawn_monsters` is enabled and the difficulty is not Peaceful. An object's panic disables that instance's callback, logs and queues an error toast naming the mod. A phase refusal is reported without disabling the callback. Closing the level drops its objects; a later open registers fresh ones."#]
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "server",
        contract = "listen",
        host = "read",
        neoforge = "ModifyCustomSpawnersEvent",
        cancellable = false,
        event_per_listener = true,
        isolate = true,
        version = 2
    )]
    fn modify_custom_spawners(
        level: mod_api::LevelRef,
        spawners: &mut mod_api::__private::LevelCustomSpawners,
    );
}
