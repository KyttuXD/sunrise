#!/usr/bin/env python3
"""Initialize the resident stack revision in existing ItemEntity constructors.

Run from the repository root on P3-66 after its signed delay migration.
"""

import re
from pathlib import Path


for filename in (
    "sunrise-client/examples/probe/lava_cauldron_item_fixture.rs",
    "sunrise-client/src/entities.rs",
    "sunrise-client/src/entities/fluids.rs",
    "sunrise-client/src/entities/item_fall.rs",
    "sunrise-client/src/entity_nbt.rs",
    "sunrise-client/src/game/blocks/machines/mod.rs",
    "sunrise-client/src/game/items/fishing.rs",
    "sunrise-client/src/game/modding/native_surface.rs",
    "sunrise-client/src/game/world/end/end_gateway_ticking_tests.rs",
    "sunrise-gameplay/src/foxes.rs",
    "sunrise-gameplay/src/item_areas.rs",
):
    path = Path("crates") / filename
    src = path.read_text()
    updated, count = re.subn(
        r"(?m)^( +)(pickup_delay: [^\n]+,\n)(?= +bob_offset\b)",
        r"\1stack_revision: 0,\n\1\2",
        src,
    )
    assert count == (7 if filename.endswith("/entities.rs") else 1), path
    path.write_text(updated)

path = Path("crates/sunrise-client/src/entities.rs")
src = path.read_text()
anchor = "    pub stack: ItemStack,\n    pub thrower: Option<[i32; 4]>,"
assert src.count(anchor) == 1, path
path.write_text(src.replace(anchor, """    pub stack: ItemStack,
    /// Identity of `DATA_ITEM` across stack replacement, even when values compare equal.
    /// `ItemEntity.playerTouch` retains its original stack across the Pre callback.
    pub stack_revision: u64,
    pub thrower: Option<[i32; 4]>,"""))
