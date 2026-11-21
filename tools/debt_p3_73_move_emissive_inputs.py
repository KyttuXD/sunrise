#!/usr/bin/env python3
"""Move emissive declarations from per-frame inputs to pack-staging inputs."""
from pathlib import Path
import re
import subprocess


def replace_once(path, pattern, replacement, flags=0):
    path = Path(path)
    result, count = re.subn(pattern, replacement, path.read_text(), flags=flags)
    assert count == 1, (path, count)
    path.write_text(result)


render = Path('crates/sunrise-client-render/src/gfx')
client = Path('crates/sunrise-client/src/game')
replace_once(
    client.with_suffix('.rs'),
    r'        let emissive_layers = if self\.title\.is_none\(\) \{.*?\n        \};\n',
    '',
    re.S,
)
replace_once(render / 'draw_frame.rs', r'            emissive_layers: \{.*?\n            \],\n', '', re.S)
paths = subprocess.check_output(
    ['git', 'grep', '-l', '-E', '^ +((pub(\\(super\\))? )?emissive_(layers|count)):', '--', 'crates'],
    text=True,
).splitlines()
for name in paths:
    path = Path(name)
    path.write_text(re.sub(r'^ +(?:pub(?:\(super\))? )?emissive_(?:layers|count):[^\n]*\n', '', path.read_text(), flags=re.M))

paths = subprocess.check_output(['git', 'grep', '-l', 'stage_pack(', '--', str(client)], text=True).splitlines()
for name in paths:
    path = Path(name)
    if path == client / 'world/packs/mod.rs':
        continue
    contents = path.read_text()
    result, count = re.subn(
        r'(\bstage_pack\(\n(?:[^\n]+\n){5})( +)&\[\],\n',
        r'\1\2&[],\n\2&[],\n',
        contents,
    )
    assert count == contents.count('stage_pack('), (path, count)
    path.write_text(result)
