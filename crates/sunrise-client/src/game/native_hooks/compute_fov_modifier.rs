use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// Viewpoint player FOV scale, after vanilla player effects and before camera easing.
    pub(in crate::game) fn native_compute_fov_modifier(
        &self,
        player: &mod_api::client::camera::FovPlayer,
        base: f32,
        modifier: &mut f32,
    ) {
        self.fire_compute_fov_modifier(player, base, modifier);
    }
}
