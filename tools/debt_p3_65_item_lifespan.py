#!/usr/bin/env python3
"""Add the default ItemEntity lifespan to existing constructors and fixtures."""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CLIENT = ROOT / "crates/sunrise-client/src"


def main() -> None:
    files = subprocess.check_output(
        [
            "git", "grep", "-l", "ItemEntity {", "--",
            "crates/sunrise-client", "crates/sunrise-gameplay",
        ],
        cwd=ROOT,
        text=True,
    ).splitlines()
    for name in files:
        path = ROOT / name
        original = path.read_text()
        lines = original.splitlines(keepends=True)
        inserts = []
        for start, line in enumerate(lines):
            if (
                not re.search(r"\bItemEntity \{$", line)
                or "->" in line
                or line.startswith(("impl ", "pub struct "))
            ):
                continue
            indent = " " * (len(line) - len(line.lstrip()))
            field_indent = indent + "    "
            end = next(
                i for i in range(start + 1, len(lines)) if lines[i].startswith(indent + "}")
            )
            fields = lines[start + 1 : end]
            if any(field.startswith((field_indent + "..", field_indent + "lifespan:")) for field in fields):
                continue
            age = next(
                i for i in range(start + 1, end) if re.match(field_indent + r"age[:,]", lines[i])
            )
            inserts.append((age + 1, field_indent + "lifespan: ItemEntity::LIFETIME,\n"))
        for at, field in reversed(inserts):
            lines.insert(at, field)
        updated = "".join(lines)
        if updated != original:
            path.write_text(updated)

    path = CLIENT / "entities.rs"
    source = path.read_text()
    if "pub lifespan: i32," not in source:
        path.write_text(source.replace(
            "    pub age: i32,\n    /// Signed NBT counter;",
            "    pub age: i32,\n    /// The age at which the server asks whether this item should expire.\n"
            "    pub lifespan: i32,\n    /// Signed NBT counter;",
            1,
        ).replace(
            "impl ItemEntity {\n    pub const WIDTH",
            "impl ItemEntity {\n    /// `ItemEntity.LIFETIME`: the default five-minute lifetime, in ticks.\n"
            "    pub const LIFETIME: i32 = 6000;\n    pub const WIDTH",
            1,
        ))

    path = CLIENT / "entities/item_fall.rs"
    source = path.read_text()
    if "lifespan: Self::LIFETIME," not in source:
        path.write_text(source.replace("            age: 0,\n", "            age: 0,\n            lifespan: Self::LIFETIME,\n", 1))


if __name__ == "__main__":
    main()
