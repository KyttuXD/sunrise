//! The `compute_fov_modifier` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The effective camera player's FOV modifier, after flying, movement speed and bow use.
    /// Runs at `AbstractClientPlayer.getFieldOfViewModifier`, including for a spectated player;
    /// a non-player camera and the first-person spyglass's early return do not fire it.
    /// `base` is the unscaled modifier. `modifier` starts as `lerp(effect_scale, 1, base)`;
    /// edit it to change the target that `Camera.tickFov` eases and clamps to 0.1..=1.5.
    /// A non-finite proposal is refused and the preceding answer stands.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "super::client::camera::validate_fov",
        neoforge = "ComputeFovModifierEvent",
        cancellable = false,
        version = 2
    )]
    fn compute_fov_modifier(
        player: &mod_api::client::camera::FovPlayer,
        base: f32,
        effect_scale: f32,
        modifier: &mut f32,
    );
}
