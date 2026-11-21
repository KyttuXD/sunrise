//! The `compute_fov` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0112

pub trait Hooks {
    /// The final world field of view, after the eased player modifier, death and camera fluid,
    /// and before projection and Nausea. Runs at world camera extraction, including spectating,
    /// with the actual camera position and its partial tick. The hand/HUD has its own angle.
    /// Edit `degrees`; a non-finite proposal is refused and the previous answer stands.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_compute_fov",
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "super::client::camera::validate_fov",
        neoforge = "ViewportEvent.ComputeFov",
        version = 2
    )]
    fn compute_fov(partial: f32, camera: [f64; 3], degrees: &mut f32);
}
