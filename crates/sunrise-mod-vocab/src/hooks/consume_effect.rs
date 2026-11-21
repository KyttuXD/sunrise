//! The `consume_effect` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0086

pub trait Hooks {
    /// A custom entry of the stack's effective `consumable`, in list order,
    /// interleaved with built-in effects. `stack` is the read-only snapshot
    /// before consumption shrinks it, including components. `user` is the
    /// player or mob that started consuming this stack. The kind's subscriber
    /// is bound when the definition is decoded. Nested applications run in call
    /// order, including another entry of the same kind. A panic in the callback
    /// disables that callback for the session; later effects still run. An unwind
    /// from engine work or another listener reached through a living-effect call
    /// propagates instead, without disabling the calling consume callback.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_consume_effect",
        dispatch = "direct",
        side = "server",
        contract = "listen",
        version = 3,
        neoforge = "none (runtime ConsumeEffect.apply; RegisterEvent registers effect types, not invocations)"
    )]
    #[notes = "`consume_effect` receives the custom kind, retained definition data, pre-shrink stack snapshot and consuming entity (player or mob). Entries come from the effective consumable, including stack patches, and run in list order between the built-in entries. The callback is bound at decode; nested effects are not filtered by kind."]
    fn consume_effect(
        kind: &str,
        data: &sunrise_core::nbt::Tag,
        stack: &ItemStack,
        user: mod_api::EntityRef,
    );
}
