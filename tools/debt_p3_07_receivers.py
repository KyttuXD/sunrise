#!/usr/bin/env python3
"""Replay the mechanical parts of P3-07 on their pre-migration sources.

Step 1 runs only bulk-indices. Run each remaining phase separately in the
slice that introduces its receivers; extend the table there if the final
signature differs. The table does not replace that slice's engine, API,
error, or documentation work. --dry-run checks anchors without writing.
"""

import argparse
import re
from pathlib import Path
from textwrap import dedent, indent

ROOT = Path(__file__).resolve().parents[1]
GAME = "crates/sunrise-client/src/game"
VOCAB = "crates/sunrise-mod-vocab/src"
API = "crates/sunrise-mod-api/src"
BOOK = "docs/modding/src"


def lines(before, after, *, column=0, count=1):
    margin = " " * column
    return (
        indent(dedent(before).strip("\n"), margin),
        indent(dedent(after).strip("\n"), margin),
        count,
    )


def projectile_summon(x, column):
    return lines(
        f'''
        let _ = host.summon(
            event.dimension,
            "minecraft:snowball",
            [{x}, 70.0, 0.5],
            Default::default(),
        );
        ''',
        f'''
        let level = host.level(event.dimension).expect("the projectile's level");
        let _ = level.summon(host, "minecraft:snowball", [{x}, 70.0, 0.5], Default::default());
        ''',
        column=column,
    )


PHASES = {
    "bulk-indices": {
        "crates/sunrise-client-render/src/block_edit.rs": (
            lines("pub only: Option<u16>,", "pub only: Option<u32>,", column=4),
            lines(
                "mut matches: impl FnMut(u16, u32) -> bool,",
                "mut matches: impl FnMut(u32, u32) -> bool,",
                column=8,
            ),
            lines(
                "if !filter_memo[f].get(old, |s| matches(f as u16, s)) {",
                "if !filter_memo[f].get(old, |s| matches(f as u32, s)) {",
                column=28,
            ),
            lines(
                "fn run(x: i32, z: i32, y0: i32, y1: i32, state: u32, only: Option<u16>) -> Run {",
                "fn run(x: i32, z: i32, y0: i32, y1: i32, state: u32, only: Option<u32>) -> Run {",
                column=4,
            ),
        ),
        f"{GAME}/blocks/block_edits.rs": (
            lines(
                "Some(f) if (f as usize) < filters => Some(f as u16),",
                "Some(f) if (f as usize) < filters => Some(f),",
                column=12,
            ),
        ),
    },
    "block-operations": {
        f"{GAME}/blocks/world_fluids_tests.rs": (
            lines(
                '''
                {
                    let mut host = JournalLevelWrite::new(game, level).unwrap();
                    assert!(host.set_block_property(
                        AT,
                        "facing",
                        "west",
                        (BlockUpdateFlags::CLIENTS | BlockUpdateFlags::KNOWN_SHAPE).bits()
                    ));
                }
                ''',
                '''
                assert_eq!(
                    level.set_block_property(
                        game,
                        AT,
                        "facing",
                        "west",
                        BlockUpdateFlags::CLIENTS | BlockUpdateFlags::KNOWN_SHAPE,
                    ),
                    Ok(true)
                );
                ''',
                column=12,
            ),
        ),
        f"{API}/parts/blocks_fixtures/destroyed_by_player_game_thread.rs": (
            lines(
                'host.set_block_property(event.pos, "layers", &(layers - 1).to_string());',
                '''
                let _ = level.set_block_property(
                    host,
                    event.pos,
                    "layers",
                    &(layers - 1).to_string(),
                    mod_api::BlockUpdateFlags::NEIGHBORS | mod_api::BlockUpdateFlags::CLIENTS,
                );
                ''',
                column=12,
            ),
        ),
        f"{VOCAB}/block/fluid_util.rs": (
            lines(
                "host.dispense_with_remainder(event.dimension, event.pos, event.facing, stack, changed)",
                "level.dispense_with_remainder(host, event.pos, event.facing, stack, changed)",
                column=12,
            ),
            lines(
                "Err(()) => host.dispense_default(event.dimension, event.pos, event.facing, stack),",
                "Err(()) => level.dispense_default(host, event.pos, event.facing, stack),",
                column=8,
            ),
        ),
    },
    "entity-operations": {
        f"{GAME}/blocks/nether_portal/arrival_effects_tests.rs": (
            lines(
                '''
                assert!(
                    host.summon(
                        "minecraft:the_nether",
                        "minecraft:item",
                        *self.at.lock().unwrap(),
                        nbt,
                    )
                    .is_some()
                );
                ''',
                '''
                let level = host.level("minecraft:the_nether").unwrap();
                assert!(
                    level
                        .summon(host, "minecraft:item", *self.at.lock().unwrap(), nbt)
                        .unwrap()
                        .is_some()
                );
                ''',
                column=8,
            ),
        ),
        f"{GAME}/mobs/creatures/projectile_batch_tests.rs": (
            projectile_summon(9.5, 16),
            projectile_summon(8.5, 12),
        ),
        f"{GAME}/modding/native_projectiles/impact_panic_tests.rs": (
            projectile_summon(9.5, 12),
        ),
        f"{GAME}/modding/entity_part_state_tests.rs": (
            lines(
                '''
                let pig = game
                    .summon(
                        "minecraft:overworld",
                        "minecraft:pig",
                        [1.5, 70.0, 1.5],
                        nbt,
                    )
                    .unwrap();
                let pig = game.entity(pig).unwrap();
                ''',
                '''
                let level = game.level("minecraft:overworld").unwrap();
                let pig = level.summon(game, "minecraft:pig", [1.5, 70.0, 1.5], nbt)
                    .unwrap()
                    .unwrap();
                ''',
                column=8,
            ),
        ),
        f"{GAME}/modding/native_colliders_tests.rs": (
            lines(
                '''
                let dimension = game.dim.id();
                let id = game
                    .summon(dimension, "minecraft:pig", at, nbt)
                    .expect("a pig");
                (id, game.entity(id).expect("its handle"))
                ''',
                '''
                let level = game.level(game.dim.id()).expect("the level");
                let entity = level
                    .summon(game, "minecraft:pig", at, nbt)
                    .expect("summon write")
                    .expect("a pig");
                (entity.id(), entity)
                ''',
                column=4,
            ),
        ),
        f"{GAME}/modding/native_guest_turn_tests.rs": (
            lines(
                '''
                let id = game
                    .summon(
                        game.dim.id(),
                        "minecraft:armor_stand",
                        [1.5, 70.0, 1.5],
                        tag,
                    )
                    .unwrap();
                game.entity(id).unwrap()
                ''',
                '''
                let level = game.level(game.dim.id()).unwrap();
                level.summon(game, "minecraft:armor_stand", [1.5, 70.0, 1.5], tag)
                    .unwrap()
                    .unwrap()
                ''',
                column=4,
            ),
        ),
        f"{GAME}/runtime/transaction_entities_tests.rs": (
            lines(
                '''
                let spawned = host.summon(
                    "minecraft:overworld",
                    "minecraft:pig",
                    [1.5, 70.0, 1.5],
                    Default::default(),
                );
                ''',
                '''
                let level = host.level("minecraft:overworld").unwrap();
                let spawned = level
                    .summon(host, "minecraft:pig", [1.5, 70.0, 1.5], Default::default())
                    .unwrap()
                    .map(EntityRef::id);
                ''',
                column=12,
            ),
            lines(
                '''
                let dimension = game.dim.id();
                game.summon(dimension, kind, [1.5, 70.0, 1.5], nbt)
                ''',
                '''
                let level = game.level(game.dim.id()).unwrap();
                level.summon(game, kind, [1.5, 70.0, 1.5], nbt).unwrap().map(EntityRef::id)
                ''',
                column=4,
            ),
        ),
    },
    "experience-orbs": {
        f"{GAME}/network/lan_actions/tests/orb_merge_contract_tests.rs": (
            lines(
                "/// Forced merge of different values, then the exact total written back.",
                '''
                fn orb_handle(host: &dyn ServerHost, id: u32) -> mod_api::ExperienceOrbRef {
                    let entity = host.entity(id).expect("live orb entity");
                    host.experience_orb(entity)
                        .expect("server orb lookup")
                        .expect("experience orb handle")
                }

                /// Forced merge of different values, then the exact total written back.
                ''',
            ),
            *(
                lines(
                    "assert!(host.set_experience_orb(event.dimension, event.into, Some(11), Some(1), None));",
                    '''
                    let into = orb_handle(host, event.into);
                    assert_eq!(into.set_value(host, 11), Ok(()));
                    assert_eq!(into.set_count(host, 1), Ok(()));
                    ''',
                    column=column,
                )
                for column in (8, 12)
            ),
            lines(
                "assert!(host.set_experience_orb(event.dimension, event.into, Some(11), Some(1), Some(99)));",
                '''
                let into = orb_handle(host, event.into);
                assert_eq!(into.set_value(host, 11), Ok(()));
                assert_eq!(into.set_count(host, 1), Ok(()));
                assert_eq!(into.set_age(host, 99), Ok(()));
                ''',
                column=8,
            ),
            lines(
                "assert!(host.set_experience_orb(event.dimension, event.from, Some(6), None, None));",
                '''
                let from = orb_handle(host, event.from);
                assert_eq!(from.set_value(host, 6), Ok(()));
                ''',
                column=8,
            ),
        ),
    },
    "world-edits": {
        f"{GAME}/blocks/block_edits.rs": (
            lines(
                "let dimension = game.dim.id().to_owned();",
                "let level = game.level(game.dim.id()).unwrap();",
                column=12,
            ),
            lines(
                "let refused = game.edit_blocks(&dimension, &edit);",
                "let refused = level.edit_blocks(game, &edit);",
                column=12,
                count=2,
            ),
            lines(
                "assert_eq!(game.edit_blocks(&dimension, &edit), Ok(1));",
                "assert_eq!(level.edit_blocks(game, &edit), Ok(1));",
                column=12,
            ),
        ),
        f"{GAME}/modding/native_structure_placement.rs": (
            lines(
                "ready_chunked_game(game);",
                "ready_chunked_game(game);\nlet level = game.level(game.dim.id()).unwrap();",
                column=12,
                count=2,
            ),
            *(
                lines(
                    f"let {result} = game.place_template(",
                    f"let {result} = level.place_template(\n    game,",
                    column=12,
                )
                for result in ("placed", "refused")
            ),
            lines(
                '''
                let reason = refused.expect_err("a chunk of the box is not loaded");
                assert!(
                    reason.starts_with("chunk (1, 0) of the template's box is not loaded"),
                    "{reason}"
                );
                ''',
                '''
                assert_eq!(refused, Err(mod_api::StructureTemplateError::UnloadedChunk { chunk: [1, 0] }));
                ''',
                column=12,
            ),
        ),
        f"{BOOK}/how-to/world/placing-a-template.md": (
            lines("let placed = host.place_template(", "let placed = level.place_template(\n    host,"),
        ),
    },
    "menu-operations": {
        f"{GAME}/modding/mod_menu_decisions_tests.rs": (
            lines(
                '''
                host.menu_set_items(event.uuid, event.window, vec![Some(item)])
                    .unwrap();
                ''',
                '''
                let player = host.player(event.uuid).unwrap();
                let menu = player.menu(host, event.window).unwrap().unwrap();
                menu.set_items(host, vec![Some(item)]).unwrap();
                ''',
                column=12,
            ),
        ),
        f"{GAME}/modding/native_menu_inventory_tests.rs": (
            lines(
                "let menu = host.menu(event.uuid, event.window).unwrap();",
                '''
                let player = host.player(event.uuid).unwrap();
                let menu = player.menu(host, event.window).unwrap().unwrap();
                ''',
                column=12,
            ),
        ),
        f"{GAME}/modding/native_menu_places.rs": (
            lines(
                "let menu = host.menu(123, window).unwrap();",
                '''
                let menu = host.player(123).unwrap().menu(host, window).unwrap().unwrap();
                ''',
                column=12,
                count=6,
            ),
            lines(
                "assert_eq!(host.menu(123, window), None);",
                "assert_eq!(host.player(123).unwrap().menu(host, window), Ok(None));",
                column=12,
                count=3,
            ),
            lines(
                "assert!(host.menu(123, window).is_some());",
                "assert!(host.player(123).unwrap().menu(host, window).unwrap().is_some());",
                column=12,
            ),
            lines(
                '''
                let host: &mut dyn ServerHost = game;
                host.open_menu(
                    123,
                    3,
                    mod_api::Text::literal("pack"),
                    Vec::new(),
                    MenuReplace::CloseFirst,
                )
                .unwrap()
                .unwrap()
                ''',
                '''
                let player = game.player(123).unwrap();
                player.open_chest_menu(
                    game,
                    3,
                    mod_api::Text::literal("pack"),
                    Vec::new(),
                    MenuReplace::CloseFirst,
                )
                .unwrap()
                .unwrap()
                ''',
                column=8,
            ),
        ),
        f"{GAME}/modding/native_menus/owned_slots_tests.rs": (
            lines(
                "host.menu_set_items(123, window, vec![]),",
                "menu.set_items(host, vec![]),",
                column=12,
            ),
            lines(
                "assert_eq!(game.menu(123, window), Some(menu));",
                "assert_eq!(game.native_menu_ref(123, window), Some(menu));",
                column=12,
                count=2,
            ),
            lines(
                "assert!(game.menu(123, window).is_none());",
                "assert!(game.native_menu_ref(123, window).is_none());",
                column=12,
            ),
        ),
        f"{GAME}/modding/native_book_apply/tests.rs": (
            lines(
                '''
                event.ghost = host
                    .place_recipe_in_grid(event.recipe, 1, 1, vec![0], vec![0], event.use_max_items)
                    .unwrap();
                ''',
                '''
                let menu = host.players().acting_player()
                    .and_then(|player| player.container_menu(host)).unwrap();
                event.ghost = menu
                    .place_recipe_in_grid(host, event.recipe, 1, 1, &[0], &[0], event.use_max_items)
                    .unwrap();
                ''',
                column=8,
            ),
            lines(
                "let menu = game.menu(opened.uuid, opened.window).unwrap();",
                "let menu = game.native_menu_ref(opened.uuid, opened.window).unwrap();",
                column=12,
            ),
        ),
        f"{API}/parts/registration_identity_tests.rs": (
            lines(
                "let _: fn(&mut dyn ClientHost, ItemKey, u32) -> Result<(), String> = |h, k, n| h.give(k, n);",
                '''
                let _: fn(PlayerRef, &mut dyn ClientHost, ItemKey, u32) -> Result<(), CreativeGiveError> =
                    |p, h, k, n| p.give_creative(h, k, n);
                ''',
                column=8,
            ),
        ),
    },
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=PHASES)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    pending = {}
    for name, edits in PHASES[args.phase].items():
        path = ROOT / name
        source = path.read_text(encoding="utf-8")
        for before, after, expected in edits:
            source, found = re.subn(
                f"^{re.escape(before)}$", lambda _: after, source, flags=re.MULTILINE
            )
            if found != expected:
                raise SystemExit(
                    f"{args.phase}: {name}: expected {expected} matches of "
                    f"{before.strip()!r}, found {found}"
                )
        pending[path] = source
    if not args.dry_run:
        for path, source in pending.items():
            path.write_text(source, encoding="utf-8")
    action = "checked" if args.dry_run else "rewrote"
    print(f"{args.phase}: {action} {len(pending)} file(s)")


if __name__ == "__main__":
    main()
