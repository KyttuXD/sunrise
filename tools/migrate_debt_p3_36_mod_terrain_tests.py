#!/usr/bin/env python3
"""Pass the selected generator and biome source to P3-36 terrain fixtures."""
import re
from pathlib import Path


path = Path("crates/sunrise-client/src/modgen.rs")
source = path.read_text()
production, tests = source.split("#[cfg(test)]", 1)
for stem, generator in (
    ("stem", "generators().get(stem[\"generator\"][\"type\"].as_str()?)?.clone()"),
    ("&stem", 'generators["test:by_level"].clone()'),
    ("&stem", 'generators()["test:slab"].clone()'),
    ("&stem", 'generators()["test:slab"].clone()'),
    ("&oasis_stem()", 'generators()["test:slab"].clone()'),
):
    old = rf"ModTerrain::for_stem\(\n( +)([^\n]+),\n +{re.escape(stem)},\n +&generators(?:\(\))?,"
    new = rf'ModTerrain::new(\n\1\2,\n\1&{stem.removeprefix("&")}["generator"]["biome_source"],\n\1{generator},'
    tests, count = re.subn(old, new, tests, count=1)
    if count != 1:
        raise SystemExit(f"Missing fixture for {stem}")
path.write_text(production + "#[cfg(test)]" + tests)
