#!/usr/bin/env python3
"""Migrate the runtime-level test callers to the recast flat-level constructor."""
from pathlib import Path

root = Path(__file__).resolve().parents[1]
path = root / 'crates/sunrise-client/src/game/world/runtime_levels_tests.rs'
old = 'RuntimeLevel::new("minecraft:overworld", RuntimeGenerator::Flat, '
text = path.read_text()
assert text.count(old) == 2
text = text.replace(old, 'RuntimeLevel::flat("minecraft:overworld", ')
text = text.replace('    RuntimeGenerator, RuntimeLevel, RuntimeLevelError,',
                    '    RuntimeLevel, RuntimeLevelError,')
path.write_text(text.replace('table.queue_unload(', 'table.queue_unload("test", '))
