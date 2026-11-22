#!/usr/bin/env python3
"""Pass the CPU-only stitch limit from stage_pack's non-renderer test callers."""
from pathlib import Path
import re
import subprocess


def main():
    paths = subprocess.check_output(
        ["git", "grep", "-l", r"\bstage_pack(", "--", "crates/sunrise-client/src"], text=True
    ).splitlines()
    migrated = 0
    for path in map(Path, paths):
        if path.as_posix().endswith("/world/packs/mod.rs"):
            continue
        old = path.read_text()
        new, count = re.subn(
            r"(\bstage_pack\(\n(?:[^\n]*\n){12})( +)(&[\w:]+::new\(\),)\n(?= +\))",
            r"\1\2\3\n\2sunrise_client::hud::MAX_ATLAS_SIZE,\n",
            old,
        )
        if count:
            path.write_text(new)
        migrated += count
    assert migrated == 7, migrated


if __name__ == "__main__":
    main()
