#!/usr/bin/env python3
"""Replay P3-34's mechanical declaration and spawn-callback caller edits on the part-2 base."""
import re
from pathlib import Path
import subprocess


files = subprocess.check_output(
    ["git", "grep", "-l", "-E", "declare_general_entity\\(|SpawnSource|GeneralEntityTypeDeclaration \\{", "--", "crates", "docs/modding"],
    text=True,
).splitlines()
client = "crates/sunrise-client/src/"
for filename in files:
    path = Path(filename)
    original = path.read_text()
    source = original
    if filename.startswith(client):
        source = re.sub(
            r'(\.declare_general_entity\(\s*)"(robot|render_probe)",',
            r'\1mod_api::Identifier::from_parts("mymod", "\2").unwrap(),',
            source,
        )
        source = re.sub(
            r'(\.declare_general_entity\(\s*)(path|name),',
            r'\1mod_api::Identifier::from_parts("mymod", \2).unwrap(),',
            source,
        )
        source = re.sub(
            r'(GeneralEntityTypeDeclaration \{\s*mod_id: [^\n]+\n\s*id: )(id\.into\(\)|id\.to_owned\(\)|"[^"]+"\.into\(\)|"[^"]+"\.to_owned\(\))',
            lambda m: m[1] + "mod_api::Identifier::parse_namespaced(" + m[2].rsplit(".", 1)[0] + ").unwrap()",
            source,
        )
        source = source.replace("Option<mod_api::SpawnSource>", "Option<mod_api::Spawner>")
    if source != original:
        path.write_text(source)

Path("crates/sunrise-mod-api-gen/dispatch-tests/src/fixtures/stubs/SpawnSource.rs").unlink()
