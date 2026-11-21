//! The `entity_tick_post` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0155

pub trait Hooks {
    /// The entities `entities` ticked in the level `dimension`, and `kinds` holds the type of
    /// each, in the same order (NeoForge's `EntityTickEvent.Post`). A tick canceled by Pre
    /// is not reported; an entity that died or was removed in its tick is. An entity that
    /// ticks individually is told right after its own tick; a family is told once, after
    /// its last tick. The coverage and logical sides match `entity_tick_pre`. Implement
    /// this hook to subscribe to Post; you do not have to subscribe to Pre.
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
