//! The `compute_fov_modifier` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Player FOV scale for the effective camera player (local or spectated),
    /// after vanilla effects and before camera easing. The hook doesn't fire for
    /// a non-player camera or while the camera player scopes with a spyglass in
    /// first person (`AbstractClientPlayer.getFieldOfViewModifier` returns `0.1`
    /// before `ComputeFovModifierEvent`). `player.using` reports the camera
    /// player's item use, not the local player's when those differ.
    /// Invalid values or values outside 0.01..=10 revert to base.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
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
