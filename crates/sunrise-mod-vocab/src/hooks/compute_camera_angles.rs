//! The `compute_camera_angles` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        output_validator = "crate::client::camera::refused_angles_output",
        neoforge = "ViewportEvent.ComputeCameraAngles",
        cancellable = false
    )]
    /// Client camera angles before `Camera.setRotation`, third-person mirroring, detached back-off,
    /// and sleeping bed rotation (`ViewportEvent.ComputeCameraAngles` in `Camera.alignWithEntity`).
    ///
    /// Starts from the camera entity's interpolated `(view_y_rot(partial), view_x_rot(partial), 0.0)`
    /// in degrees. Listeners see the previous listener's accepted angles. A non-finite write is
    /// reported to the log with the mod ID and leaves those angles intact. This hook does not run
    /// for the interpolating `NewMinecartBehavior` rider branch or for a custom studio camera.
    ///
    /// Third-person front mirroring (`yaw + 180`, `-pitch`, `-roll`), detached back-off along the
    /// rotated look vector, and sleeping bed rotation run after this hook, matching NeoForge.
    /// `ClientHost::set_camera_type` called before or during this hook takes effect before that
    /// detached placement runs. The effective camera angles orient the picture and audio listener;
    /// the entity's eye rotation, the player's aim and the separate view transform do not change.
    fn compute_camera_angles(partial: f32, yaw: &mut f32, pitch: &mut f32, roll: &mut f32);
}
