#!/usr/bin/env python3
"""Remove the dedicated native Zoom declaration and its forwarding paths."""

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
GAME = "crates/sunrise-client/src/game"
VOCAB = "crates/sunrise-mod-vocab/src"
RENDER = "crates/sunrise-client-render/src"
FIXTURES = "crates/sunrise-mod-api-gen/dispatch-tests/src/fixtures"


def remove(path, start, end):
    file = ROOT / path
    text = file.read_text()
    begin = text.index(start)
    stop = text.index(end, begin + len(start))
    file.write_text(text[:begin] + text[stop:])


def replace(path, old, new):
    file = ROOT / path
    text = file.read_text()
    assert text.count(old) == 1, (path, old)
    file.write_text(text.replace(old, new))


(ROOT / f"{VOCAB}/hooks/zoom.rs").unlink()
remove(f"{VOCAB}/client/types.rs", "/// The zoom a native mod asks for", "/// `ParticleStatus`")
remove(f"{GAME}/native_hooks/base.rs", "    /// The zoom a native mod asks for", "    /// Computed world fog")
remove(f"{GAME}/native_hooks/shared.rs", "    /// The zoom in effect while playing", "    /// The pose of native mods'")
remove(f"{GAME}/interface/extras.rs", "    /// A mod's zoom in effect", "    /// The PvP HUD widgets")
replace(f"{GAME}/interface/extras.rs", "//! Sunrise's extras in the game (`docs/extras.md`); a native mod's zoom (`native_zoom`) on the\n//! camera; clicks and frame times for mods (`native_clicks`, `native_frame_rate`).", "//! Sunrise's extras in the game (`docs/extras.md`); clicks and frame times for mods\n//! (`native_clicks`, `native_frame_rate`).")
replace(f"{GAME}/interface/extras.rs", "extras::{Corner, Cps, FpsCounter, fov_target}", "extras::{Corner, Cps, FpsCounter}")
replace(f"{GAME}/interface/fov.rs", "        let target = self.zoomed_fov_target(target);\n", "")
replace(f"{GAME}/interface/fov.rs", "self.zoomed_fov_target(1.0)", "1.0")
replace(f"{GAME}/interface/fov.rs", "self.zoomed_fov_degrees(fov)", "fov")
replace(f"{GAME}/interface/pointer_capture.rs", "        let zoom = self.zoom_turn_scale();\n        self.turn_player(x * zoom, y * zoom);", "        self.turn_player(x, y);")
replace(f"{GAME}/native_hooks/mod.rs", "{finite, finite_params, local_sound_ok}", "{finite_params, local_sound_ok}")
replace(f"{GAME}/native_hooks/sound_and_shader_helpers.rs", "\n/// `v` within `lo..=hi`; `lo` when it is not a number.\npub(super) fn finite(v: f32, lo: f32, hi: f32) -> f32 {\n    if v.is_finite() { v.clamp(lo, hi) } else { lo }\n}\n", "")
remove(f"{RENDER}/extras.rs", "/// The field-of-view modifier target", "/// Frame times over the last few seconds")
remove(f"{RENDER}/extras.rs", "    #[test]\n    fn a_mods_zoom_eases_like_the_spyglass()", "    #[test]\n    fn fps_counter_reports_rate_minimum_and_low()")
replace(f"{FIXTURES}/shared.rs", "#[derive(Debug, PartialEq)]\nstruct Zoom(u32);\n", "")
replace(f"{FIXTURES}/shared.rs", "MapDecoration, MapInfo, Zoom,", "MapDecoration, MapInfo,")
remove(f"{FIXTURES}/shared.rs", "    fn zoom(&self", "    fn living_incoming_damage(")
remove(f"{FIXTURES}/base.rs", "    let mut zoom = None;", "    install_parts(Parts {")

# Keep the generic wiring fixture, pointing it at the adjacent FOV hook.
file = ROOT / "crates/sunrise-mod-api-gen/src/lib.rs"
text = file.read_text()
file.write_text(text.replace("native_zoom", "native_compute_fov")
                .replace("dispatch_zoom", "dispatch_compute_fov")
                .replace("fire_zoom", "fire_compute_fov"))
