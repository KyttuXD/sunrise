//! The `weather_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0341

pub trait Hooks {
    /// Before the server writes weather parameters, `raining` and `thundering`
    /// are the requested final states. Cancel to skip the entire write: the
    /// clear, rain and thunder timers and both flags. Server-side.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_weather_change",
        dispatch = "flat",
        side = "server",
        contract = "cancel",
        out = "cancel",
        neoforge = "none (NeoForge has no such event)",
        cancellable = true,
        version = 2
    )]
    fn weather_change(dimension: &str, raining: bool, thundering: bool, cancel: &mut bool);
}
