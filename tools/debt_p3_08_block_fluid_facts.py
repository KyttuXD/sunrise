#!/usr/bin/env python3
"""Migrate P3-08's repeated block-state and bucket fact calls to typed receivers."""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
TEST_FILE = ROOT / "crates/sunrise-client/src/game/modding/native_world_read.rs"


def replace(source, old, new, count=1):
    if source.count(old) != count:
        raise SystemExit(f"expected {count} occurrence(s) of {old!r}")
    return source.replace(old, new)


engine, marker, tests = TEST_FILE.read_text().partition("#[cfg(test)]\nmod tests")
if not marker:
    raise SystemExit(f"{TEST_FILE}: cannot find test module")

tests = replace(
    tests,
    "            assert_eq!(\n                game.state_outline(stone),",
    "            let states = mod_api::HostBlockStates::block_states(&*game);\n"
    "            let invalid_state = mod_api::__private::block_state_from_ordinal(\n"
    "                stone.try_into_api().unwrap().block(), u16::MAX,\n"
    "            );\n"
    "            assert_eq!(\n                game.state_outline(stone),",
)
for method, replacement in [("state_outline", "outline"), ("state_fluid_state", "fluid_state")]:
    tests = replace(
        tests,
        f"game.{method}(BlockStateId(u32::MAX))",
        f"invalid_state.{replacement}(&states)",
    )
    tests, converted = re.subn(
        rf"game\.{method}\(([a-z_]+)\)",
        rf"\1.try_into_api().unwrap().{replacement}(&states)",
        tests,
    )
    if converted != {"outline": 3, "fluid_state": 6}[replacement]:
        raise SystemExit(f"unexpected number of {method} tests: {converted}")

tests = replace(tests, "Some(vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]])", "Some(&[[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]][..])")
tests = replace(tests, "Some(vec![[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]])", "Some(&[[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]][..])")
tests = replace(tests, "air.try_into_api().unwrap().outline(&states), Some(Vec::new())", "air.try_into_api().unwrap().outline(&states), Some(&[][..])")
tests = replace(tests, "host.state_fluid_state(slab)", "slab.fluid_state(&host.block_states())")
tests = replace(tests, "    use super::*;\n", "    use super::*;\n    use mod_api::HostBlockStates;\n")

before, marker, buckets = tests.partition("    fn fluid_sameness_buckets_and_bucket_sounds_follow_vanilla() {")
if not marker:
    raise SystemExit(f"{TEST_FILE}: cannot find bucket fact test")

ids = {
    "water": "minecraft:water",
    "flowing_water": "minecraft:flowing_water",
    "lava": "lava",
    "flowing_lava": "flowing_lava",
    "empty": "minecraft:empty",
}
lookup = r'"(?:minecraft:)?([a-z_]+)"'
buckets = replace(
    buckets,
    "            let host: &dyn mod_api::Host = game;\n",
    "            let host: &dyn mod_api::Host = game;\n"
    "            let states = host.block_states();\n"
    + "".join(f'            let {key} = host.fluid_key("{id}").unwrap();\n' for key, id in ids.items())
    + '            assert_eq!(host.fluid_key("nope:missing"), None);\n',
)
buckets = replace(buckets, '            assert!(!host.fluid_is_same("nope:missing", "nope:missing"));\n', "")
buckets = re.sub(rf"host\.fluid_is_same\({lookup}, {lookup}\)", r"\1.is_same(&states, \2)", buckets)
for method in ["bucket", "pickup_sound", "empty_sound"]:
    buckets = replace(buckets, f'            assert_eq!(host.fluid_{method}("nope:missing"), None);\n', "")
    buckets = re.sub(rf"host\.fluid_{method}\({lookup}\)", rf"\1.{method}(&states)", buckets)
if "host.fluid_" in buckets.replace("host.fluid_key(", ""):
    raise SystemExit(f"{TEST_FILE}: unmigrated fluid fact call")

TEST_FILE.write_text(engine + "#[cfg(test)]\nmod tests" + before + marker + buckets)

for page in ["capabilities/block-fluids.md", "registry-keys.md", "testing.md"]:
    path = ROOT / "docs/modding/src/concepts" / page
    source = path.read_text()
    for method in ["bucket", "pickup_sound", "empty_sound"]:
        source = source.replace(f"`fluid_{method}`", f"`FluidKey::{method}`")
    path.write_text(source)
