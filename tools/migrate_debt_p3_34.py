#!/usr/bin/env python3
"""Apply the P3-34 typed general-entity declaration migration to the client fixtures."""

from pathlib import Path
import re
import subprocess


for name in subprocess.check_output(
    ["git", "grep", "-l", "-E", r"declare_general_entity\(|GeneralEntityTypeDeclaration \{", "--", "crates/sunrise-client"],
    text=True,
).splitlines():
    path = Path(name)
    before = path.read_text()
    after = re.sub(
        r'(\.declare_general_entity\(\s*)("[a-z_]+"|path|name),',
        r'\1mod_api::Identifier::from_parts("mymod", \2).unwrap(),',
        before,
    )
    after = re.sub(
        r'(GeneralEntityTypeDeclaration \{\s*mod_id: [^\n]+\n\s*id: )((?:id|"[a-z:]+")\.(?:into|to_owned)\(\))',
        lambda m: m[1] + 'mod_api::Identifier::parse(' + m[2].rsplit('.', 1)[0] + ').unwrap()',
        after,
    )
    if after != before:
        path.write_text(after)
