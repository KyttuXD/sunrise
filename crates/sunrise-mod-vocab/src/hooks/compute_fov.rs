//! The `compute_fov` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0112

pub trait Hooks {
    /// The field of view after death and fluid effects, before building the world or first-person
    /// hand projection. Runs for each projection of the actual camera, including spectating, with
    /// its position and partial tick. `scoping` is true when the viewpoint player is using a
    /// spyglass in first person. Edit `degrees` for this projection only; a non-finite proposal
    /// is refused and the preceding answer stands.
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
    fn compute_fov(
        partial: f32,
        camera: [f64; 3],
        projection: mod_api::client::camera::FovProjection,
        scoping: bool,
        degrees: &mut f32,
    );
}
