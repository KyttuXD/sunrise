#!/usr/bin/env python3
"""Apply P3-103's mechanical attached-stack constructor and fixture migration."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

for path, defaults in (
    ("crates/sunrise-mod-vocab/src/content/attached_storage_tests.rs", 3),
    ("crates/sunrise-client/src/game/modding/attachment_index/attached_storage_tests.rs", 1),
    ("crates/sunrise-client/src/game/modding/native_item_places.rs", 1),
):
    source = ROOT / path
    text = source.read_text()
    text, _ = re.subn(
        r"(Attached(?:Items|Fluids)::new\([^\n]+\))\.unwrap\(\)", r"\1", text
    )
    text, count = re.subn(
        r"(static \w+: AttachmentType<StackList<\w+>> = AttachmentType \{\n"
        r"\s*namespace: [^\n]+\n\s*name: [^\n]+\n\s*default: )Vec::new,",
        r"\1StackList::new,",
        text,
    )
    if count != defaults:
        raise SystemExit(f"{path}: expected {defaults} old slot defaults, found {count}")
    source.write_text(text)
