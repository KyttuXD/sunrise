//! The three independent stages of preparing a world frame's camera and view.

pub trait Hooks {
    /// NeoForge's `ViewportEvent.ComputeCameraAngles`: edit `[yaw, pitch, roll]` in degrees,
    /// in listener order, at the entity rotation assignment in `Camera.alignWithEntity`.
    /// `partial` is the camera entity's render partial tick. Roll starts at zero; the front
    /// third-person view subsequently adds 180° to yaw and negates pitch and roll, and a bed
    /// can subsequently set its own orientation. Like NeoForge, this does not fire on the
    /// new-movement minecart interpolation branch. Only the render camera rotates; the
    /// entity's own angles and targeting do not change. Non-finite edits are refused.
    #[hook(
        target = "sunrise_client::game::native_hooks::camera_setup::Game<'_>::native_compute_camera_angles",
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "crate::client::camera::validate_angles",
        neoforge = "ViewportEvent.ComputeCameraAngles"
    )]
    fn compute_camera_angles(partial: f32, angles: &mut [f32; 3]);

    /// Edit the effective render camera's world position in blocks (`Camera.setPosition`),
    /// after `Camera.alignWithEntity` has placed it, including detached and sleeping views.
    /// `position` starts at that position and carries each preceding listener's accepted edit.
    /// Visibility, fog and the sound listener use it. The camera entity's eye, targeting,
    /// movement, hand origin and chunk tickets do not. Non-finite edits are refused.
    #[hook(
        target = "sunrise_client::game::native_hooks::camera_setup::Game<'_>::native_camera_position",
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "crate::client::camera::validate_position",
        neoforge = "none"
    )]
    fn camera_position(partial: f32, position: &mut [f64; 3]);

    /// Edit the column-major view-effect transform after `GameRenderer.bobHurt` and
    /// `bobView` and before the world projection's spinning effect. `transform` starts at
    /// vanilla's pose and carries each preceding listener's accepted edit. Post-multiply it
    /// to add a transform in view space. The world and first-person hand see the result;
    /// camera position, orientation, picking and audio do not. Non-finite or non-invertible
    /// transforms are refused. Read `host.graphics_options().screen_effect_scale` if your
    /// effect should respect Distortion Effects; the game does not scale your edits.
    #[hook(
        target = "sunrise_client::game::native_hooks::camera_setup::Game<'_>::native_view_transform",
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "crate::client::camera::validate_transform",
        neoforge = "none"
    )]
    fn view_transform(partial: f32, transform: &mut [f32; 16]);
}
