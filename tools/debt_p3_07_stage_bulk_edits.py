#!/usr/bin/env python3
"""Give the bulk edit entry point an explicit transaction-staging argument."""
from pathlib import Path
import re

paths = [
    Path("crates/sunrise-client-render/src/block_edit.rs"),
    Path("crates/sunrise-client/src/game/blocks/block_edits.rs"),
]
for path in paths:
    original = path.read_text()
    result = re.sub(r"(\.edit_runs\()(\s*)", r"\1\2None, ", original)
    result = result.replace("None, \n", "None,\n")
    if path == paths[0]:
        result = result.replace(
            "        runs: &[Run],",
            "        _journal: Option<&mut EditJournal>,\n        runs: &[Run],",
        )
        result = result.replace(
            "/// Per state answers, worked out once each.",
            "/// The changes a Host transaction holds until its root commits.\n"
            "#[derive(Debug, Default)]\n"
            "pub struct EditJournal;\n\n"
            "/// Per state answers, worked out once each.",
        )
    assert result != original, path
    path.write_text(result)
