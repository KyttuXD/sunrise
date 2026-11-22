#!/usr/bin/env python3
"""Migrate static Brigadier registry arguments and the command parse fixture to RegistryId."""

from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[1]
COMMAND = ROOT / "crates/sunrise-command/src"
VARIANTS = r"Res|ResKey|ResOrTag|ResOrTagKey|ResourceSelector|ResourceOrId"
STATIC = re.compile(rf'(ArgType::(?:{VARIANTS})\(\s*)("[^"\n]+")(?=\s*\))')


def main():
    trees = ("tree_data", "tree_execute", "tree_extra", "tree_misc", "tree_world", "vanilla")
    for module in (*trees, "network"):
        path = COMMAND / f"{module}.rs"
        before = path.read_text(encoding="utf-8")
        after = STATIC.sub(r"\1RegistryId::from_static(\2)", before)
        if module == "tree_data":
            after = after.replace(
                "ArgType::ResourceOrId(registry)",
                "ArgType::ResourceOrId(RegistryId::from_static(registry))",
            )
        if after == before:
            continue
        if module == "network":
            after = after.replace("NodeKind, Suggests};", "NodeKind, RegistryId, Suggests};", 1)
        else:
            after = after.replace("    Dispatcher,", "    Dispatcher, RegistryId,", 1)
        path.write_text(after, encoding="utf-8")

    path = ROOT / "crates/sunrise-client/examples/probe/command_parse_fixture.rs"
    before = path.read_text(encoding="utf-8")
    after = before.replace(
        "ArgType::ResourceOrId(r) => match *r", "ArgType::ResourceOrId(r) => match r.as_str()"
    )
    if after != before:
        path.write_text(after, encoding="utf-8")


if __name__ == "__main__":
    main()
