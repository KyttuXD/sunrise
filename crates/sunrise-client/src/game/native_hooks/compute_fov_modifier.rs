use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// The camera player's FOV scale, after vanilla effects and before easing.
    /// Covers both local and spectated players, except non-player cameras and
    /// first-person spyglass scoping (`AbstractClientPlayer.getFieldOfViewModifier`
    /// returns `0.1` early). Invalid values or values outside 0.01..=10 revert to
    /// base. API 362.
    pub(in crate::game) fn native_compute_fov_modifier(
        &self,
        player: &mod_api::client::camera::FovPlayer,
        base: f32,
        modifier: &mut f32,
    ) {
        self.fire_compute_fov_modifier(player, base, modifier);
    }
}
