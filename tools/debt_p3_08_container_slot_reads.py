#!/usr/bin/env python3
"""Migrate P3-08's restricted-level and capability slot-read callers to loans."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GAME = "crates/sunrise-client/src/game"
API = "crates/sunrise-mod-api/src"


def replace(file, old, new, count=1):
    path = ROOT / file
    source = path.read_text()
    if source.count(old) != count:
        raise SystemExit(f"{file}: expected {count} occurrence(s) of {old!r}")
    path.write_text(source.replace(old, new))


old_signature = (
    "    fn container_stack(&self, at: mod_api::BlockSide, slot: usize)"
    " -> Option<mod_api::ItemStack> {"
)
new_signature = """    fn lend_container_stack(
        &self,
        at: mod_api::BlockSide,
        slot: usize,
        read: &mut dyn for<'a> FnMut(mod_api::ItemStackView<'a>),
    ) -> Option<()> {"""

read_level = f"{GAME}/modding/native_level_signals.rs"
replace(read_level, old_signature, new_signature)
replace(
    read_level,
    """        if at.level != self.level
            || !self
                .game
                .world
                .is_loaded_at(at.pos[0], at.pos[1], at.pos[2])
        {
            return None;
        }""",
    "        (at.level == self.level).then_some(())?;",
    count=2,
)
replace(
    read_level,
    "        self.game.container_stack(at, slot)?.try_into_api().ok()",
    "        self.game.lend_container_stack(at, slot, read)",
)

host = f"{GAME}/modding/mod_api_host.rs"
replace(host, old_signature, new_signature, count=2)
replace(
    host,
    "        fn container_stack(at: mod_api::BlockSide, slot: usize) -> Option<mod_api::ItemStack>;",
    "        fn lend_container_stack(at: mod_api::BlockSide, slot: usize, read: &mut dyn for<'a> FnMut(mod_api::ItemStackView<'a>)) -> Option<()>;",
)
replace(
    host,
    """        if !self.native_authoritative
            || self.api_level != Some(at.level)
            || !self.world.is_loaded_at(at.pos[0], at.pos[1], at.pos[2])
        {
            return None;
        }""",
    """        if !self.native_authoritative || self.api_level != Some(at.level) {
            return None;
        }""",
    count=2,
)
replace(
    host,
    """        self.containers()
            .capability_stack(at, slot)?
            .try_into_api()
            .ok()""",
    "        self.containers().lend_capability_stack(at, slot, read)",
)
replace(
    host,
    "        mod_api::LevelHost::container_stack(&*self.level, at, slot)",
    "        mod_api::LevelHost::lend_container_stack(&*self.level, at, slot, read)",
)

journal = f"{GAME}/modding/journal_level_write.rs"
replace(journal, "FluidState, ItemStack, LevelHost,", "FluidState, ItemStackView, LevelHost,")
replace(
    journal,
    "        fn container_stack(&self, at: BlockSide, slot: usize) -> Option<ItemStack>;",
    "        fn lend_container_stack(&self, at: BlockSide, slot: usize, read: &mut dyn for<'a> FnMut(ItemStackView<'a>)) -> Option<()>;",
)

loot = f"{GAME}/world/container_loot/tests.rs"
replace(loot, "            use mod_api::{LevelHost, LevelRandomHost};", "            use mod_api::LevelRandomHost;")
replace(
    loot,
    "                assert_eq!(level.block_entity(at.pos),",
    "                let read: &dyn mod_api::LevelHost = level;\n                assert_eq!(read.block_entity(at.pos),",
    count=2,
)
for call, new_call in (
    ("level.container_size(", "read.container_size("),
    ("level.block_entity([32, 64, 8])", "read.block_entity([32, 64, 8])"),
    ("level.container_stack(at, slot)", "read.with_container_stack(at, slot, |_| ())"),
    ("level.container_stack(unloaded, 0)", "read.with_container_stack(unloaded, 0, |_| ())"),
    ("level.container_stack(at, 0)", "read.with_container_stack(at, 0, |_| ())"),
):
    replace(loot, call, new_call, count=3 if call == "level.container_size(" else 1)

replace(
    f"{GAME}/blocks/world_fluids_tests.rs",
    "            assert_eq!(host.container_stack(side, 0).unwrap().count(), 3);",
    """            assert_eq!(
                (&host as &dyn LevelHost).with_container_stack(side, 0, |stack| stack.count()),
                Some(3),
            );""",
)

replace(
    f"{API}/parts/capabilities.rs",
    """        assert!(
            backend_of(&*host)
                .and_then(|backend| backend.container_stack(on(CHEST, None), 0))
                .is_none()
        );""",
    """        let backend = backend_of(&*host).unwrap();
        assert_eq!(backend.lend_container_stack(on(CHEST, None), 0, &mut |_| {}), None);""",
)
replace(
    f"{API}/parts/capabilities.rs",
    """        assert_eq!(
            backend_of(&*host).and_then(|backend| backend.container_stack(on(CHEST, None), 0)),
            Some(stick())
        );""",
    """        let mut stack = None;
        backend_of(&*host)
            .unwrap()
            .lend_container_stack(on(CHEST, None), 0, &mut |view| stack = view.project())
            .unwrap();
        assert_eq!(stack, Some(stick()));""",
)

replace(
    f"{API}/parts/items.rs",
    """            Some(ItemResource::of(
                &backend_of(&*host)?.container_stack(on(SHELF, None), index)?,
            ))""",
    """            let mut resource = None;
            backend_of(&*host)?.lend_container_stack(on(SHELF, None), index, &mut |view| {
                resource = view.project().as_ref().map(ItemResource::of);
            })?;
            resource""",
)
replace(
    f"{API}/parts/items.rs",
    """            backend_of(&*host)
                .and_then(|backend| backend.container_stack(on(SHELF, None), index))
                .map_or(0, |held| held.count())""",
    """            let mut count = 0;
            let _ = backend_of(&*host).and_then(|backend| {
                backend.lend_container_stack(on(SHELF, None), index, &mut |view| count = view.count())
            });
            count""",
)
