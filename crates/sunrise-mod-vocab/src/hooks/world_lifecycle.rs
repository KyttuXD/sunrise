//! The `world_lifecycle` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0003

pub trait Hooks {
    /// Read-only world phases on integrated and dedicated servers, never on the client window.
    /// `error_text` carries the engine's error for `WorldOpenFailed` and `WorldCloseFailed`,
    /// empty otherwise. A server loop or final save failure fires `WorldCloseFailed`.
    /// A normal stop with only individually reported record failures still fires `WorldCloseFinal`.
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
