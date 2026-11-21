#!/usr/bin/env python3
"""One-off migration of P3-66's item creation pipeline to signed delay ticks.

Run from the repository root on the original P3-66 head (a730c0b820c).
"""

from pathlib import Path


for filename in (
    "sunrise-client/src/game.rs",
    "sunrise-client/src/game/blocks/spill.rs",
    "sunrise-client/src/game/items/item_entities.rs",
    "sunrise-client/src/game/items/item_entity_spawn.rs",
    "sunrise-client/src/game/mobs/allays.rs",
    "sunrise-client/src/game/mobs/creatures/mod.rs",
    "sunrise-client/src/game/mobs/mob_level.rs",
    "sunrise-client/src/game/modding/native_level_items.rs",
    "sunrise-client/src/mobs.rs",
    "sunrise-gameplay/src/allays.rs",
    "sunrise-gameplay/src/foxes.rs",
    "sunrise-gameplay/src/mob_level.rs",
    "sunrise-mod-vocab/src/block/prepared_drops.rs",
    "sunrise-mod-vocab/src/host/level_entities.rs",
    "sunrise-mod-vocab/src/item/types.rs",
):
    path = Path("crates") / filename
    src = path.read_text()
    updated = src.replace("pickup_delay: u32", "pickup_delay: i32")
    if filename.endswith(("/spill.rs", "/prepared_drops.rs")):
        updated = updated.replace(
            "fn pickup_delay(&self, index: usize) -> Option<u32>",
            "fn pickup_delay(&self, index: usize) -> Option<i32>",
        )
    if filename.endswith("/prepared_drops.rs"):
        updated = updated.replace("fn pickup_delay(&self) -> u32", "fn pickup_delay(&self) -> i32")
    if filename.endswith("/spill.rs"):
        updated = updated.replace("pub(super) delay: u32", "pub(super) delay: i32")
    if filename.endswith("/item_entity_spawn.rs"):
        updated = updated.replace("pickup_delay: pickup_delay as i32,", "pickup_delay,")
    assert src != updated, path
    path.write_text(updated)

path = Path("crates/sunrise-client/src/game/network/lan_actions/tests/pickup_post_tests.rs")
src = path.read_text()
assert src.count("    delay: u32,") == 1, path
path.write_text(src.replace("    delay: u32,", "    delay: i32,"))
