#!/usr/bin/env python3
"""Use the explicit emissive input on every stage_pack call on the P3-73 base."""
from pathlib import Path
import re
import subprocess


def main():
    paths = subprocess.check_output(
        ["git", "grep", "-l", "-E", r"\bstage_pack(_with_emissive)?\(", "--", "crates/sunrise-client/src"],
        text=True,
    ).splitlines()
    migrated = 0
    for path in map(Path, paths):
        old = path.read_text()
        if path.as_posix().endswith("/world/packs/mod.rs"):
            start = old.index("pub(super) fn stage_pack(")
            end = old.index("pub(super) fn stage_pack_with_emissive(", start)
            new = (old[:start] + old[end:]).replace("stage_pack_with_emissive(", "stage_pack(")
        else:
            new, count = re.subn(
                r"(\bstage_pack\(\n(?:[^\n]*\n){5})( +)&\[\],\n(?= +&game\.pack\.assets,)",
                r"\1\2&[],\n\2&[],\n",
                old,
            )
            migrated += count
        path.write_text(new)
    assert migrated == 7, migrated


if __name__ == "__main__":
    main()
