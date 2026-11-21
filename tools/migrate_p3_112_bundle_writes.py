#!/usr/bin/env python3
"""P3-112: pass the item table to bundle writes so empty patches stay empty."""

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src"
for name, replacements in {
    "game/blocks/block_breaking.rs": [("contents.store(&mut bundle);", "contents.store(items, &mut bundle);")],
    "game/items/equip_answers.rs": [("contents.store(&mut bundle);", "contents.store(game.items(), &mut bundle);")],
    "game/modding/native_item_bundle.rs": [("bundle.store(&mut changed);", "bundle.store(self.res.items(), &mut changed);")],
    "lan/menu.rs": [
        ("contents.store(&mut carried);", "contents.store(items, &mut carried);"),
        ("contents.store(&mut bundle);", "contents.store(items, &mut bundle);"),
        ("toggle_selected(&mut bundle, -1);", "toggle_selected(items, &mut bundle, -1);"),
    ],
    "menu/bundle.rs": [
        ("contents.store(carried);", "contents.store(env.items, carried);"),
        ("contents.store(&mut bundle);", "contents.store(env.items, &mut bundle);"),
        ("toggle_selected(&mut bundle, -1);", "toggle_selected(env.items, &mut bundle, -1);"),
    ],
}.items():
    path = ROOT / name
    source = path.read_text()
    for old, new in replacements:
        source = source.replace(old, new)
    path.write_text(source)
