#!/usr/bin/env python3
"""Remove the downstream native-state branches before moving the refusal to the parser.

Run once on 99c5682ffe6, then rustfmt the touched Rust files. The follow-up keeps
E_MIXIN_STATE_LAYOUT at the manifest boundary and removes its input-only field.
"""

from pathlib import Path


crates = Path(__file__).resolve().parents[1] / "crates"
edits = {
    "sunrise-mod-package/src/native_route.rs": [
        ("        state,\n", "        state: _,\n"),
        ('            (state.is_empty(), "[[native.state]]"),\n', ""),
    ],
    "sunrise-native-build/src/game_uses.rs": [
        ('        (native.state.is_empty(), "[[native.state]]"),\n', ""),
    ],
    "sunrise-native-build/src/lib.rs": [
        ("\n                || !native.state.is_empty()", ""),
    ],
    "sunrise-native-build/src/package.rs": [
        ('        ("state", toml::Value::try_from(&bundle.native().state)),\n', ""),
    ],
    "sunrise-native-build/src/prebuilt.rs": [
        ("            native(&|n| n.state.push(forwarded())),\n", ""),
        ('                Some("[[native.state]]"),\n', ""),
    ],
}
for file, replacements in edits.items():
    path = crates / file
    source = path.read_text()
    for old, new in replacements:
        if source.count(old) != 1:
            raise SystemExit(f"{file}: expected one occurrence of {old!r}")
        source = source.replace(old, new, 1)
    path.write_text(source)
