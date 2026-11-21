#!/usr/bin/env python3
"""Name the private entity renderer/layer wrapper for its callback, not its draw slot."""

import re
from pathlib import Path

root = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game/modding"
for name in ("native_entity_renderers.rs", "native_entity_layers.rs"):
    path = root / name
    source = path.read_text()
    migrated = re.sub(r"\bResolved\b", "EntityDrawCallback", source)
    count = 26 if name == "native_entity_renderers.rs" else 3
    if len(re.findall(r"\bResolved\b", source)) != count:
        raise SystemExit(f"{path}: expected {count} occurrences of Resolved")
    path.write_text(migrated)
