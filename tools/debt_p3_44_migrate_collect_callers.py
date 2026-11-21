#!/usr/bin/env python3
"""Migrate the collect-hook call sites that only append a declaration or migration."""
from pathlib import Path
import re


root = Path(__file__).resolve().parents[1]
client = root / "crates/sunrise-client/src"
for name, expected in (
    ("crafting/property_sets_tests.rs", 3),
    ("lan/recipes.rs", 4),
    ("menu.rs", 1),
):
    path = client / name
    text, count = re.subn(r"\bevent\s*\.additions\s*\.push\(", "event.register(", path.read_text())
    if count != expected:
        raise SystemExit(f"{name}: expected {expected} recipe declarations, found {count}")
    path.write_text(text)

for name in ("game/native_hooks/mod_mismatch.rs", "game/world/world_loading.rs"):
    path = client / name
    text, count = re.subn(
        r'\bevent\.resolved\.push\(("[^"\n]+")\.to_owned\(\)\.into\(\)\);',
        r"event.mark_resolved(\1);",
        path.read_text(),
    )
    if count != 1:
        raise SystemExit(f"{name}: expected one resolution, found {count}")
    path.write_text(text)
