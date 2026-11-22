#!/usr/bin/env python3
"""Spell out empty resident decoration inputs in standalone probes and fixtures."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FILES = [
    "crates/sunrise-world/examples/structure_blocks.rs",
    "crates/sunrise-world/examples/probe/chunk_dump.rs",
    "crates/sunrise-world/examples/chunkgen_bench.rs",
    "crates/sunrise-world/src/worldgen/generator/mod_terrain.rs",
    "crates/sunrise-client/src/vanillagen/lod.rs",
    "crates/sunrise-client/src/vanillagen/structure_mobs.rs",
    "crates/sunrise-client/src/mod_content_tests.rs",
    "crates/sunrise-client/src/save/loot_tests.rs",
    "crates/sunrise-client/tests/generation_saves.rs",
    "crates/sunrise-client/tests/forced_chunks.rs",
    "crates/sunrise-client/examples/gen_light_bench.rs",
]
for relative in FILES:
    path = ROOT / relative
    text = path.read_text()
    text = re.sub(
        r"\.decorate\((?P<args>(?:&mut worker, )?&mut (?:region|restored))\)",
        r".decorate(\g<args>, Default::default())",
        text,
    )
    text = re.sub(
        r"(fn decorate\(&self, [^,]+, _?region: &mut \[Proto; 9\])\)",
        r"\1, _: sunrise_client::streaming::DecorationDependencies<'_>)",
        text,
    )
    path.write_text(text)
