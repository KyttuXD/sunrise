//! The `camera_position` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Edits the world-space position in blocks after `Camera.alignWithEntity` places the
    /// frame camera (`Camera.setPosition`), including third-person back-off and sleeping offsets.
    ///
    /// Listeners see the position left by earlier listeners in dependency order. A proposal
    /// containing NaN or infinity is reported and discarded, leaving the preceding position.
    /// The rendered world, fog and sound listener follow the camera. The camera entity's eye
    /// and its use for targeting are unchanged; no entity moves and no chunk tickets are added.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "crate::client::camera::validate_camera_position"
    )]
    fn camera_position(partial: f32, position: &mut [f64; 3]);
}
