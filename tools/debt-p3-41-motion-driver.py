#!/usr/bin/env python3
"""Move the motion/pose orchestration callers onto the shared MotionTick trait."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-client"
for name in (
    "examples/probe/scene_motion_probe.rs",
    "src/game/modding/native_entity_refs/restitution_replay_tests.rs",
    "src/game/network/remote/movement_input_tests.rs",
    "src/lan/hub/tests/collision_exclusion_tests.rs",
    "src/lan/hub/tests/collision_part_tests.rs",
):
    path = ROOT / name
    source = path.read_text()
    # Insert at module scope, past the module documentation and any inner attributes.
    lines = source.splitlines(keepends=True)
    i = next(i for i, line in enumerate(lines) if line.startswith("use "))
    lines.insert(i, "use sunrise_client::motion::MotionTick as _;\n")
    path.write_text("".join(lines))
