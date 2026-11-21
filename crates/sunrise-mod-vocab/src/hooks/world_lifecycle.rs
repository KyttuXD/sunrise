//! The `world_lifecycle` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0003

pub trait Hooks {
    /// Read-only world phases on integrated and dedicated servers, never on the client window.
    /// `error_text` is the engine's error from an open, server loop, or final save failure;
    /// it is empty on a successful close.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_world_lifecycle",
        dispatch = "flat",
        side = "server",
        contract = "listen",
        version = 2,
        neoforge = "ServerStartingEvent, ServerStartedEvent, ServerStoppingEvent, ServerStoppedEvent"
    )]
    fn world_lifecycle(context: &LifecycleContext, phase: WorldPhase, error_text: &str);
}
