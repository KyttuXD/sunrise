use super::*;

impl Game<'_> {
    /// At the end of the HUD layer sequence, before screens draw (`RenderGuiEvent.Post`).
    pub(in crate::game) fn native_render_gui_post(&self, draw: &mut HudBatch) {
        self.fire_render_gui_post(draw, &self.hud_layer_bounds);
    }
}
