#!/usr/bin/env python3
"""Thread the primary camera's roll through Frame construction and capture/replay."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
RENDER = ROOT / "crates/sunrise-client-render"
CLIENT = ROOT / "crates/sunrise-client/src/game"
ZERO_ROLL = [
    RENDER / f"fixtures/{name}.rs"
    for name in ("standard_pack_gpu_entities", "standard_pack_sky_switches", "world_views")
] + [
    RENDER / f"src/gfx/{name}.rs"
    for name in (
        "entity_pixel_test",
        "pack_post/depth_test",
        "pack_post/destination_tests",
        "pack_post/world_target_tests",
        "pack_uniforms",
    )
] + [CLIENT / "runtime/loading_window.rs"]


def edit(path, old, new):
    source = path.read_text()
    assert source.count(old) == 1, (path, old)
    path.write_text(source.replace(old, new))


for path in ZERO_ROLL:
    source = path.read_text()
    updated, count = re.subn(
        r"(?m)^( +)(pitch: [\d.]+,\n)( +fov_degrees:)",
        r"\1\2\1roll: 0.0,\n\3",
        source,
    )
    assert count == 1, path
    path.write_text(updated)

frame = RENDER / "src/gfx/frame.rs"
edit(frame, "    pub pitch: f32,\n", "    pub pitch: f32,\n    pub roll: f32,\n")

capture = RENDER / "src/gfx/capture.rs"
edit(capture, "    pub pitch: f32,\n", "    pub pitch: f32,\n    pub roll: f32,\n")
for prefix in ("frame", "f"):
    old = f"pitch: {prefix}.pitch,\n"
    data = capture.read_text()
    indent = re.search(rf"(?m)^( +){re.escape(old)}", data)[1]
    edit(capture, old, f"{old}{indent}roll: {prefix}.roll,\n")
