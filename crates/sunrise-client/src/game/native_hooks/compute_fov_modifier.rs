use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// Viewpoint player FOV modifier before camera easing, except for the spyglass return.
    pub(in crate::game) fn native_compute_fov_modifier(
        &self,
        player: &mod_api::client::camera::FovPlayer,
        base: f32,
        effect_scale: f32,
        modifier: &mut f32,
    ) {
        self.fire_compute_fov_modifier(player, base, effect_scale, modifier);
    }
}
