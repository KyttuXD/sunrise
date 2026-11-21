#!/usr/bin/env python3
"""Replace PlayerRef::destroy_block's drop boolean with a named destination (P3-87)."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GAME = ROOT / "crates/sunrise-client/src/game/modding"
BACKEND = ROOT / "crates/sunrise-mod-vocab/src/host/player_interaction.rs"
BOOK = ROOT / "docs/modding/src/how-to/entities/breaking-as-a-player.md"


def main():
    engine = GAME / "native_player_break.rs"
    for path in (
        engine,
        GAME / "native_player_destroyed_block_tests.rs",
        GAME / "native_player_level_tests.rs",
        BACKEND,
        BOOK,
    ):
        old = src = path.read_text()
        if path == engine:
            src = src.replace(
                "use mod_api::PlayerRef;", "use mod_api::{BlockDropDestination, PlayerRef};"
            )
            src = src.replace(
                "let mut drops = into_inventory.then(Vec::new);",
                "let mut drops = (destination == BlockDropDestination::Inventory).then(Vec::new);",
            )
        if path.name == "native_player_destroyed_block_tests.rs":
            src = src.replace("use mod_api::{BreakBlockEvent", "use mod_api::{BlockDropDestination, BreakBlockEvent")
        src = src.replace("into_inventory: bool,", "destination: BlockDropDestination,")
        type_path = (
            "mod_api::BlockDropDestination"
            if path.name == "native_player_level_tests.rs"
            else "BlockDropDestination"
        )
        src = re.sub(
            r"((?:game\.player_destroy_block\(\w+|player\.destroy_block\(host), [^\n]+?, )(true|false)\)",
            lambda m: m[1] + type_path + "::" + ("Inventory" if m[2] == "true" else "World") + ")",
            src,
        )
        if path == engine:
            src = src.replace(
                "assert_eq!(player.destroy_block(host, next, collect), !collect);",
                "let destination = if collect { BlockDropDestination::Inventory } else { BlockDropDestination::World };\n"
                "            assert_eq!(player.destroy_block(host, next, destination), !collect);",
            )
        if src != old:
            path.write_text(src)


if __name__ == "__main__":
    main()
