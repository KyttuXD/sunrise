//! The `entity_tick_post` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0155

pub trait Hooks {
    /// The entities `entities` ticked in the level `dimension`, and `kinds` holds the type of
    /// each, in the same order (NeoForge's `EntityTickEvent.Post`). A canceled Pre prevents
    /// Post, but an entity that dies or is removed while ticking still gets it. A type named with
    /// `entity_tick_each` is told alone right after its own tick, even without a Pre
    /// subscriber; a family is told once, after its last tick. Told where `entity_tick_pre`
    /// is, on the server and on a client (`host` says which side). Subscribe to Post
    /// directly; no Pre subscription or separate opt-in poll is needed.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_entity_tick_post",
        dispatch = "flat",
        side = "both",
        version = 4,
        contract = "listen",
        neoforge = "EntityTickEvent.Post",
        cancellable = false
    )]
    fn entity_tick_post(dimension: &str, entities: &[u32], kinds: &[&str]);
}
