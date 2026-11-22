//! Sunrise's extras in the game (`docs/extras.md`); a native mod's zoom (`native_zoom`) on the
//! camera.
//! Each hook does nothing while its extra is off and no mod uses it.
#[path = "extras/mouse_tweaks.rs"]
pub(in crate::game) mod mouse_tweaks;
#[path = "extras/sorting.rs"]
mod sorting;

use super::{Game, Screen};
use std::time::Duration;
use sunrise_client::{
    extras::{Corner, fov_target},
    extras_hud::{self, Corners},
    hud::HudBatch,
};

#[derive(Default)]
pub(super) struct ExtrasState {
    /// Captured before pre listeners; shared by the draw and post listeners.
    pub(super) frame_partial_ticks: Option<(f32, f32)>,
}

impl Game<'_> {
    /// Once per frame: update frame timing and the extra HUD state.
    pub(super) fn extras_frame(&mut self, elapsed: Duration) {
        let (world_partial_tick, partial_tick) = self.frame_partial_ticks();
        self.extras.frame_partial_ticks = Some((world_partial_tick, partial_tick));
        self.native_render_frame_pre(elapsed, world_partial_tick, partial_tick);
        // A clear a mod asked for (in this frame's `RenderFrameEvent.Pre`, or after the last
        // frame was drawn) lands before this frame's particles are drawn, paused or not.
        self.apply_mod_particle_clear();
        self.pvp_frame();
        self.recipes_received_now();
    }
    /// A mod's zoom in effect (`native_zoom`): (factor, smooth, turning
    /// scale); none is 1.
    fn zoom_now(&self) -> (f32, bool, f64) {
        self.native_zoom_now().map_or((1.0, true, 1.0), |z| {
            (z.factor, z.smooth, f64::from(z.turn))
        })
    }
    /// The field-of-view modifier target, divided by a smooth zoom.
    pub(super) fn zoomed_fov_target(&self, vanilla: f32) -> f32 {
        match self.zoom_now() {
            (factor, true, _) => fov_target(vanilla, factor),
            _ => vanilla,
        }
    }
    /// The field of view in degrees, divided by an instant zoom.
    pub(super) fn zoomed_fov_degrees(&self, fov: f32) -> f32 {
        match self.zoom_now() {
            (factor, false, _) => fov / factor,
            _ => fov,
        }
    }
    /// Mouse turning while zoomed.
    pub(super) fn zoom_turn_scale(&self) -> f64 {
        self.zoom_now().2
    }
    /// The PvP HUD widgets and the engine's extra corner panels. The caller hides
    /// these with F1, and hands the remaining corner space to the whole-HUD Post hook.
    pub(super) fn draw_extras(&mut self, batch: &mut HudBatch, corners: &mut Corners) {
        self.draw_pvp_hud(batch, corners);
        // The layers crates registered (`sunrise_hooks::hud`): none, no canvas.
        let layers = sunrise_hooks::hud::engine::all();
        if !layers.is_empty() {
            let shown = Corner::ALL.map(|corner| !(self.debug && corner == Corner::TopLeft));
            extras_hud::draw_layers(layers, batch, &self.gui.atlas, corners, shown);
        }
    }
    /// The height the status effect icons take at the top right
    /// (`Hud.extractEffects`: beneficial at y 1, harmful at y 26).
    pub(super) fn effect_rows_height(&self) -> f32 {
        let effects = &self.potions.effects.list;
        if effects.is_empty() || self.screen != Screen::Playing || self.pvp_effects_shown() {
            return 0.0;
        }
        let harmful = effects.iter().any(|e| {
            sunrise_client::brewing::effect(e.id)
                .is_some_and(|d| d.category != sunrise_client::brewing::Category::Beneficial)
        });
        if harmful { 50.0 } else { 25.0 }
    }
}
