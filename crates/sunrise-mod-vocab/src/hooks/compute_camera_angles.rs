//! The `compute_camera_angles` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Edits the client camera's yaw, pitch and roll in degrees before `Camera.setRotation`,
    /// at NeoForge's `ViewportEvent.ComputeCameraAngles` in `Camera.alignWithEntity`.
    ///
    /// Starts from the camera entity's interpolated yaw and pitch, with zero roll. Listeners
    /// compose in dependency order. A proposal containing NaN or infinity is reported and
    /// discarded, leaving the preceding angles for the next listener.
    ///
    /// Third-person front mirroring (`yaw + 180`, `-pitch`, `-roll`), detached back-off and
    /// sleeping bed rotation follow this event. It does not fire in the interpolated new-minecart
    /// branch, or outside entity camera setup (for example, the title panorama). It never turns
    /// the player or changes aim.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "crate::client::camera::validate_camera_angles",
        neoforge = "ViewportEvent.ComputeCameraAngles",
        cancellable = false
    )]
    fn compute_camera_angles(partial: f32, yaw: &mut f32, pitch: &mut f32, roll: &mut f32);
}
