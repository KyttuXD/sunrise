//! The `compute_fov_modifier` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Viewpoint player's FOV scale, once per client tick with a player as the camera entity,
    /// after vanilla's player effects (including first-person scoping) and before camera easing.
    /// `base` is unchanged by mods; edit `modifier` to change the target. A non-finite proposal
    /// is refused and the previous answer stands. Vanilla `Camera.tickFov` still eases and clamps
    /// the resulting modifier to 0.1..=1.5.
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
        modifier: &mut f32,
    );
}
