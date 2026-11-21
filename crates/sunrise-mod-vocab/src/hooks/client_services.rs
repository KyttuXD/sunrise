//! The `client_services` declarations reconciled with the fork API.

pub trait Hooks {
    /// The normal menu blur request, before finite/range validation. None disables it.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        neoforge = "none (NeoForge has no menu blur event)"
    )]
    fn menu_background(radius: &mut Option<f32>);
}
