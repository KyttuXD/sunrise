#!/usr/bin/env python3
"""Move living_travel test listeners from raw vector fields to the scoped motion API."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parent.parent / "crates/sunrise-client/src"
for relative in (
    "game/modding/native_living_travel_sweep_tests.rs",
    "game/modding/native_living_travel_tests.rs",
    "game/network/lan_actions/guest_body/tests/travel.rs",
    "motion/travel.rs",
):
    path = ROOT / relative
    source = path.read_text()
    source = re.sub(
        r"=> \*event\.(input|velocity) = ([^\n]+),$",
        r"=> { let _ = event.motion.set_\1(\2); }",
        source,
        flags=re.MULTILINE,
    )
    source = re.sub(
        r"\*event\.(input|velocity) = ([^\n;]+);",
        r"let _ = event.motion.set_\1(\2);",
        source,
    )
    source = re.sub(r"\*event\.(input|velocity)\b", r"event.motion.\1()", source)
    path.write_text(source)
