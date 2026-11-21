#!/usr/bin/env python3
"""Move the P3-38 shearing fixtures to a borrowed tool and the new Part signature."""

from pathlib import Path
import re


GAME = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game"


def main() -> None:
    for relative in (
        "entities/lightning.rs",
        "interface/level_sounds.rs",
        "mobs/part_shearing.rs",
    ):
        path = GAME / relative
        source = path.read_text()
        source = re.sub(
            r"game\.shear_entity\((\([^()]+\))\)",
            r'game.shear_entity(\1, &sunrise_client::items::ItemStack::new("minecraft:shears", 1))',
            source,
        )
        if relative == "mobs/part_shearing.rs":
            source = source.replace("&mod_api::ItemStack,", "mod_api::ItemStackView<'_>,")
        path.write_text(source)


if __name__ == "__main__":
    main()
