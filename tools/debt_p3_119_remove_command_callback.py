#!/usr/bin/env python3
"""Remove the unused native builder's per-line command callback and its no-op callers."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-native-build/src"
EDITS = {
    "lib.rs": [
        ("; `line` sees each line of its standard error.", ".", 1),
        (", line: impl FnMut(&str)", "", 1),
        ("run_checked(cmd, what, line, None, None)", "run_checked(cmd, what, None, None)", 1),
        ("run_checked(cmd, what, |_| {}, None, Some(sources))", "run_checked(cmd, what, None, Some(sources))", 1),
        ("    mut line: impl FnMut(&str),\n", "", 1),
        ("                line(&raw);\n", "", 1),
        (', |_| {})', ')', 5),
    ],
    "discover.rs": [("                |_| {},\n", "", 1)],
    "mixins_patch.rs": [(', |_| {})', ')', 1)],
    "patch.rs": [(', |_| {})', ')', 2)],
    "prebuilt.rs": [
        ("                |_| {},\n", "", 1),
        (', |_| {})', ')', 1),
    ],
    "registry_compile.rs": [("        |_| {},\n", "", 1)],
    "registry_generate.rs": [(', |_| {})', ')', 1)],
    "registry_plan.rs": [(', |_| {})', ')', 2)],
}


def main():
    updates = {}
    for name, edits in EDITS.items():
        path = ROOT / name
        source = path.read_text(encoding="utf-8")
        for old, new, expected in edits:
            if source.count(old) != expected:
                raise SystemExit(f"{path}: expected {expected} occurrence(s) of {old!r}")
            source = source.replace(old, new)
        updates[path] = source
    for path, source in updates.items():
        path.write_text(source, encoding="utf-8")


if __name__ == "__main__":
    main()
