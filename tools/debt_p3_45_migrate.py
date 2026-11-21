#!/usr/bin/env python3
"""Move the two HUD callbacks to one name and move corner policy out of mod_api."""

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VOCAB = ROOT / "crates/sunrise-mod-vocab/src"
CLIENT = ROOT / "crates/sunrise-client/src"
RENDER = ROOT / "crates/sunrise-client-render/src"
BOOK = ROOT / "docs/modding/src/how-to/client"
FIXTURES = ROOT / "crates/sunrise-mod-api-gen/dispatch-tests/src/fixtures"


def replace(path, old, new):
    source = path.read_text()
    if old not in source:
        raise ValueError(f"missing {old!r} in {path}")
    path.write_text(source.replace(old, new))


def main():
    # Keep Sunrise's built-in panel placement private to its own code; it no longer
    # crosses the mod boundary or defines a binary format.
    corner = VOCAB / "client/extras.rs"
    corner_value = corner.read_text().split("/// A screen corner", 1)[1]
    corner_value = re.sub(r"^// exhaustive:.*\n|^#\[derive\(sunrise_mod_api_macros::Wire\)\]\n", "", corner_value, flags=re.M)
    corner_value = corner_value.replace("    // hidden: a game-side helper, not part of the mod surface\n    #[doc(hidden)]\n", "")
    replace(
        RENDER / "extras.rs",
        "/// A screen corner for an overlay.\npub use mod_api::client::extras::Corner;",
        "// no wire: this is local placement policy for the built-in panels.\n"
        "/// A screen corner" + corner_value.rstrip(),
    )

    corners = VOCAB / "client/extras_hud.rs"
    stacking = corners.read_text().split("/// The gap", 1)[1]
    stacking = stacking.replace(
        "// exhaustive: its fields are private, so mods cannot build or destructure it\n"
        "#[derive(Clone, sunrise_mod_api_macros::Wire)]\n",
        "// no wire: offsets are local to this paint pass.\n",
    ).replace("    pub(crate) ", "    ")
    replace(CLIENT / "extras_hud.rs", "pub use mod_api::client::extras_hud::Corners;\nuse sunrise_hooks::hud;", "use sunrise_hooks::hud;\n\n/// The gap" + stacking.rstrip())

    replace(VOCAB / "client/mod.rs", "".join(f"// hidden: a module of game-side types; mods use their re-exports\n#[doc(hidden)]\npub mod {name};\n" for name in ("extras", "extras_hud")), "")
    for path in (corner, corners, VOCAB / "exports/extras.rs", VOCAB / "exports/extras_hud.rs"):
        path.unlink()

    test = VOCAB / "wire/wire_layout_tests.rs"
    source = test.read_text()
    source, count = re.subn(r"    fields\(\n        Corners \{.*?\n    \);\n", "", source, flags=re.S)
    assert count == 1
    test.write_text(source)
    wire_path = "crate::client::extras::Corner"
    fixture = ROOT / "tools/fixtures/wire_enum_baseline_identities.json"
    replace(fixture, f'  "{wire_path}",\n', "")
    fixture = ROOT / "tools/fixtures/wire_enum_layouts.json"
    source = fixture.read_text()
    [entry] = [entry for entry in json.loads(source)["enums"] if entry["path"] == wire_path]
    replace(fixture, "\n".join("    " + line for line in json.dumps(entry, indent=2).splitlines()) + ",\n", "")

    # Remove the second dispatch path and update direct, one-hook consumers. The probe
    # implementing both hooks needs a hand migration.
    for path in (
        VOCAB / "hooks/gui_render_post.rs",
        CLIENT / "game/native_hooks/gui_render_post.rs",
        FIXTURES / "gui_render_post.rs",
        FIXTURES / "render_gui_post.rs",  # only a stub for Corners
    ):
        path.unlink()
    native = CLIENT / "game/native_hooks"
    replace(native / "mod.rs", "mod gui_render_post;\n", "")
    replace(native / "mod.rs", "    extras_hud::Corners,\n", "")
    replace(CLIENT / "game.rs", "        self.native_gui_render_post(&mut hud, self.ui.hide_gui, self.focused);\n", "")
    replace(CLIENT / "game/interface/extras.rs", "self.native_render_gui_post(batch, &mut corners);", "self.native_render_gui_post(batch);")
    replace(native / "render_gui_post.rs", "        corners: &mut Corners,\n", "")
    replace(native / "render_gui_post.rs", "self.fire_render_gui_post(draw, corners);", "self.fire_render_gui_post(draw);")
    replace(VOCAB / "hooks/render_gui_post.rs", ", corners: &mut Corners", "")
    replace(VOCAB / "hooks/render_gui_post.rs", '        neoforge = "RenderGuiEvent.Post"', '        version = 2,\n        neoforge = "RenderGuiEvent.Post"')

    for path in (
        CLIENT / "game/interface/block_screen_effect_draw_tests.rs",
        BOOK / "dynamic-textures.md",
        BOOK / "hud-drawing.md",
        BOOK / "screen-clipping.md",
    ):
        replace(path, "GuiRenderPostEvent", "RenderGuiPostEvent")
        replace(path, "gui_render_post", "render_gui_post")
        source = path.read_text()
        path.write_text(re.sub(r"(event\s*)\.batch\b", r"\1.draw", source))
    for path in (
        BOOK / "entities-in-screens.md",
        BOOK / "screens.md",
        VOCAB / "host/draw.rs",
        VOCAB / "hooks/gui_layer_post.rs",
    ):
        replace(path, "gui_render_post", "render_gui_post")


if __name__ == "__main__":
    main()
