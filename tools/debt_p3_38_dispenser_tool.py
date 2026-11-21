#!/usr/bin/env python3
"""Carry the dispensed stack through the entity shear and its direct test callers."""

from pathlib import Path

GAME = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game"


def replace(source: str, old: str, new: str) -> str:
    if source.count(old) != 1:
        raise ValueError(f"expected one occurrence of {old!r}")
    return source.replace(old, new)


def main() -> None:
    dispenser = GAME / "blocks/dispenser.rs"
    source = replace(
        dispenser.read_text(), "self.shear_entity(target)?", "self.shear_entity(target, &stack)?"
    )
    start = source.index("    pub(super) fn shear_entity(")
    end = source.index("\n    }", start)
    shear = replace(
        source[start:end],
        'fn shear_entity(&mut self, target: Pos) -> Result<bool> {\n'
        '        let shears = ItemStack::new("minecraft:shears", 1);',
        "fn shear_entity(&mut self, target: Pos, shears: &ItemStack) -> Result<bool> {",
    ).replace("&shears", "shears")
    dispenser.write_text(source[:start] + shear + source[end:])

    for path, pos in [
        ("entities/lightning.rs", "(x, 64, 0)"),
        ("interface/level_sounds.rs", "(4, 64, 2)"),
    ]:
        file = GAME / path
        file.write_text(replace(
            file.read_text(),
            f"game.shear_entity({pos})",
            f'game.shear_entity({pos}, &sunrise_client::items::ItemStack::new("minecraft:shears", 1))',
        ))

    fixture = GAME / "mobs/part_shearing.rs"
    source = fixture.read_text()
    for pos in ["(0, 64, 0)", "(4, 64, 0)"]:
        # The mixed-case fixture and the phase fixture both shear at x=4.
        source = source.replace(f"game.shear_entity({pos})", f"game.shear_entity({pos}, &shears)")
    source = replace(
        source,
        '            add(2, "minecraft:sheep", 4);\n',
        '            add(2, "minecraft:sheep", 4);\n'
        '            let shears = ItemStack::new("minecraft:shears", 1);\n',
    )
    fixture.write_text(source)


if __name__ == "__main__":
    main()
