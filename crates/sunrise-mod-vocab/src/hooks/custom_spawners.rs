//! The custom spawner registration hook (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `ModifyCustomSpawnersEvent`: a logical server level has been constructed.
    /// The Overworld starts with its built-in spawners; other levels start empty. Remove a built-in
    /// with `spawners.remove(kind)` or append an owned `CustomSpawner` with
    /// `spawners.add_custom_spawner(Box::new(spawner))`. The level retains each object until it
    /// closes. This event is not cancellable and fires before `level_load`, not during ticking.
    #[notes = r#"## Custom spawners

`modify_custom_spawners` fires once per server level lifetime, before `level_load`, even if mob spawning is off. The Overworld starts with its built-in spawners; other levels start empty. `spawners.remove(kind)` takes a built-in out without changing the relative order or tick phase of the others. `spawners.add_custom_spawner(Box::new(spawner))` appends an owned `CustomSpawner`.

A level calls each appended object's `tick` only while `spawn_mobs` is enabled, in registration order. The object retains its own state between ticks. Its `ServerHost` is for the level named by `dimension`, and `spawn_enemies` reports the `spawn_monsters` rule and non-Peaceful difficulty; a spawner that only produces enemies can skip its work when false. An object's panic disables that callback without stopping the other objects. Closing the level drops its spawner objects; a later open registers fresh ones."#]
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
        dimension: mod_api::DimensionKey,
        spawners: &mut dyn mod_api::CustomSpawners,
    );
}
