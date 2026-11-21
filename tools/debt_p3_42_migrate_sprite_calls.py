#!/usr/bin/env python3
"""Migrate the client hook fixtures to native sprite loans (run on the part-2 base)."""

from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CLIENT = ROOT / "crates/sunrise-client/src/game"


def rewrite(relative, replacements):
    path = CLIENT / relative
    text = path.read_text()
    for old, new in replacements:
        if text.count(old) != 1:
            raise ValueError(f"expected one occurrence in {path}: {old}")
        text = text.replace(old, new)
    path.write_text(text)


rewrite(
    "modding/native_block_sprites.rs",
    [
        (
            'host.texture_sprites(&["minecraft:block/stone"])[0]',
            'host.texture_sprite("minecraft:block/stone").unwrap().map(|view| view.sprite)',
        ),
        (
            'host.texture_sprites(&["marker:block/own"])[0]',
            'host.texture_sprite("marker:block/own").unwrap().map(|view| view.sprite)',
        ),
    ],
)
rewrite(
    "interface/client_reload/client_reload_tests.rs",
    [
        (
            'host.block_sprites(&[host.block_state_id("minecraft:stone").unwrap()])[0].is_some()',
            'host.visit_block_sprites(host.block_state_id("minecraft:stone").unwrap(), &mut |_, _| {}).unwrap().is_some()',
        ),
        (
            'host.texture_sprites(&["minecraft:block/stone"])[0].is_some()',
            'host.texture_sprite("minecraft:block/stone").unwrap().is_some()',
        ),
    ],
)
