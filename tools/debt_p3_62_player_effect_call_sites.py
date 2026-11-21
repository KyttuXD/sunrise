#!/usr/bin/env python3
"""One-time P3-62 migration of plain player-store writes to LivingEntity.addEffect."""

from pathlib import Path

GAME = Path("crates/sunrise-client/src/game")


def migrate(file: str, *calls: str) -> None:
    path = GAME / file
    source = path.read_text()
    for call in calls:
        assert source.count(call) == 1, (path, call)
        source = source.replace(call, call.replace(".potions.effects.add(", ".add_player_effect("))
    path.write_text(source)


if __name__ == "__main__":
    migrate("blocks/beacon.rs", "self.potions.effects.add(ambient(primary, amplifier));",
            "self.potions.effects.add(ambient(s, 0));")
    migrate("blocks/block_entity_anims.rs", "g.potions.effects.add(power);")
    migrate("mobs/raids/mod.rs", "g.potions.effects.add(effect);")
    migrate("items/potions.rs", "            self.potions.effects.add(effect);\n        }\n    }\n    /// A client's own effects")
