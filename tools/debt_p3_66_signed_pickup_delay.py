#!/usr/bin/env python3
"""Migrate item pickup delay producers and storage to Java ItemEntity's int."""
import re
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parents[1]


def replace(path, old, new):
    file = ROOT / path
    source = file.read_text()
    if source.count(old) != 1:
        raise SystemExit(f"{path}: expected one occurrence of {old!r}")
    file.write_text(source.replace(old, new))


def main():
    files = subprocess.check_output(
        ["git", "grep", "-lz", "pickup_delay: u32", "--", "crates/*"], cwd=ROOT
    ).decode().rstrip("\0").split("\0")
    for name in files:
        file = ROOT / name
        file.write_text(file.read_text().replace("pickup_delay: u32", "pickup_delay: i32"))

    game = "crates/sunrise-client/src/game/"
    vocab = "crates/sunrise-mod-vocab/src/"
    replace(game + "blocks/spill.rs", "pub(super) delay: u32,", "pub(super) delay: i32,")
    for name in [game + "blocks/spill.rs", vocab + "block/prepared_drops.rs"]:
        file = ROOT / name
        source, count = re.subn(
            r"(fn pickup_delay\([^\n]*\) -> )(Option<u32>|u32)",
            lambda m: m[1] + m[2].replace("u32", "i32"),
            file.read_text(),
        )
        if count != (1 if name.startswith(game) else 4):
            raise SystemExit(f"{name}: unexpected number of pickup_delay accessors: {count}")
        file.write_text(source)

    file = ROOT / (game + "entities/ahead.rs")
    source = file.read_text()
    assert source.count("(u32, i32, i64)") == 2
    file.write_text(source.replace("(u32, i32, i64)", "(u32, i32, i32)"))
    replace(game + "items/item_entity_spawn.rs", "pickup_delay: i64::from(pickup_delay),", "pickup_delay,")
    replace(
        game + "native_hooks/shared/world_helpers.rs",
        "// The helper exposes an unsigned delay; keep its projection.\n"
        "                pickup_delay: e.pickup_delay.clamp(0, i64::from(u32::MAX)) as u32,",
        "pickup_delay: e.pickup_delay,",
    )
    replace(
        "crates/sunrise-client/src/entities.rs",
        "/// Signed NBT counter; widened to retain the legacy internal u32::MAX sentinel.\n"
        "    pub pickup_delay: i64,",
        "/// `ItemEntity.pickupDelay`: signed in memory, `32767` never ticks down.\n"
        "    pub pickup_delay: i32,",
    )
    replace(
        "crates/sunrise-client/src/entities.rs",
        "self.pickup_delay == i64::from(i16::MAX) || self.pickup_delay == i64::from(u32::MAX)",
        "self.pickup_delay == i32::from(i16::MAX)",
    )
    replace(
        "crates/sunrise-client/src/entities.rs",
        """        for delay in [i64::from(i16::MAX), i64::from(u32::MAX)] {
            let mut items = [
                item("minecraft:sand", 1, 0.5, 1),
                item("minecraft:sand", 1, 0.5, 1),
            ];
            items[1].pickup_delay = delay;
            assert_eq!(merge(&mut items, 0), [Some(1), Some(1)]);
        }
""",
        """        let mut items = [
            item("minecraft:sand", 1, 0.5, 1),
            item("minecraft:sand", 1, 0.5, 1),
        ];
        items[1].pickup_delay = 32767;
        assert_eq!(merge(&mut items, 0), [Some(1), Some(1)]);
""",
    )
    nbt = "crates/sunrise-client/src/entity_nbt.rs"
    replace(nbt, 'pickup_delay: i64::from(short("PickupDelay", 0))', 'pickup_delay: i32::from(short("PickupDelay", 0))')
    replace(
        nbt,
        "e.pickup_delay\n                    .clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16,",
        "e.pickup_delay.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,",
    )
    replace(nbt, """        let mut legacy = item_entity_from_nbt(&fresh, 1, 0.0).unwrap();
        legacy.pickup_delay = i64::from(u32::MAX);
        assert!(tick(&mut legacy));
        assert_eq!(legacy.pickup_delay, i64::from(u32::MAX));
""", "")
    for name, subject in [
        ("modding/native_item_entity_refs.rs", "self.dropped[index]"),
        ("native_hooks/on_entity_item_update.rs", "self.game.dropped[self.index]"),
    ]:
        replace(game + name, f"{subject}.pickup_delay = i64::from(ticks);", f"{subject}.pickup_delay = ticks;")
        replace(game + name, """        if delay == i64::from(u32::MAX) {
            Some(i32::from(i16::MAX))
        } else {
            i32::try_from(delay).ok()
        }
""", "        Some(delay)\n")
    replace(game + "modding/native_item_entity_refs.rs", """            game.dropped[0].pickup_delay = i64::from(u32::MAX);
            assert_eq!(item.pickup_delay(game), Some(32767));
""", "")
    replace(game + "native_hooks/item_picked_up.rs", "pickup_delay: &mut i64,", "pickup_delay: &mut i32,")
    replace(game + "native_hooks/item_picked_up_tests.rs", "fn decide(mut delay: i64, target: Option<[i32; 4]>, answer: TriState) -> (i64, bool)", "fn decide(mut delay: i32, target: Option<[i32; 4]>, answer: TriState) -> (i32, bool)")


if __name__ == "__main__":
    main()
