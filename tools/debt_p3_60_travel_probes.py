#!/usr/bin/env python3
"""One-off p3-60 migration of travel probes to the input comparison argument."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates"

for name in ("living_air_fixture", "living_fluid_fixture"):
    path = ROOT / f"sunrise-world/examples/probe/{name}.rs"
    source = path.read_text()
    source, _ = re.subn(
        r"(travel_in_(?:air|fluid)\(&mut w, &mut MoveGate::new\(\), )input\)",
        r"\1input.into()).map(|_| ())",
        source,
    )
    source = source.replace(
        "relative_friction_movement(&mut w, &mut MoveGate::new(), input, friction)\n"
        '                .and_then(|v| writeln!(w.out, "vector {}", vector(v)))',
        "relative_friction_movement(&mut w, &mut MoveGate::new(), input.into(), friction)\n"
        '                .and_then(|(v, _)| writeln!(w.out, "vector {}", vector(v)))',
    )
    source = re.sub(
        r"(travel_in_(?:water|lava)\(\n(?:[^\n]+\n){2}\s*)input,(.*?\n            \))",
        r"\1input.into(),\2.map(|_| ())",
        source,
        flags=re.S,
    )
    path.write_text(source)

path = ROOT / "sunrise-client/src/game/modding/native_surface.rs"
source = path.read_text()
source = source.replace(
    "&mut sunrise_world::move_gate::MoveGate::new(),\n                    Vec3::new(1.0, 0.0, 0.0),",
    "&mut sunrise_world::move_gate::MoveGate::new(),\n                    Vec3::new(1.0, 0.0, 0.0).into(),",
)
path.write_text(source)
