#!/usr/bin/env python3
"""Move the falling-block engine fixtures from dimension strings to level handles."""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-client/src/game"

EDITS = {
    "entities/falling_save.rs": (
        """                            for x in 0..6 {
                                assert!(game.spawn_falling_block(
                                    game.dim.id(),
                                    sand,
                                    [f64::from(x), 70.0, 0.5],
                                    [0.0; 3],
                                    mod_api::Falling::default()
                                ));
                            }""",
        """                            let level = game.level(game.dim.id()).unwrap();
                            for x in 0..6 {
                                assert!(
                                    game.level_spawn_falling_block(
                                        level,
                                        sand,
                                        [f64::from(x), 70.0, 0.5],
                                        [0.0; 3],
                                        mod_api::Falling::default()
                                    )
                                    .unwrap()
                                    .is_some()
                                );
                            }""",
    ),
    "modding/native_entity_move_sweep_tests.rs": (
        """            assert!(game.spawn_falling_block(
                "minecraft:overworld",
                stone,
                at(6),
                [0.5, 0.0, 0.0],
                mod_api::Falling::default()
            ));""",
        """            let level = game.level("minecraft:overworld").unwrap();
            assert!(
                game.level_spawn_falling_block(
                    level,
                    stone,
                    at(6),
                    [0.5, 0.0, 0.0],
                    mod_api::Falling::default()
                )
                .unwrap()
                .is_some()
            );""",
    ),
    "native_hooks/block_entity_on_load_routes_tests.rs": (
        """            assert!(game.spawn_falling_block(
                "minecraft:overworld",
                state,
                [1.5, 66.0, 1.5],
                [0.0; 3],
                Falling::default()
            ));""",
        """            let level = game.level("minecraft:overworld").unwrap();
            assert!(
                game.level_spawn_falling_block(
                    level,
                    state,
                    [1.5, 66.0, 1.5],
                    [0.0; 3],
                    Falling::default()
                )
                .unwrap()
                .is_some()
            );""",
    ),
    "runtime/transaction_entities_tests.rs": (
        """        ("spawn_falling_block", |game, _| {
            let sand = (game.pack.assets.registry)
                .default_state("minecraft:sand")
                .unwrap();
            let dimension = game.dim.id();
            game.spawn_falling_block(
                dimension,
                sand,
                [1.5, 72.0, 1.5],
                [0.0; 3],
                Default::default(),
            )
        }),""",
        """        ("level_spawn_falling_block", |game, f| {
            let sand = (game.pack.assets.registry)
                .default_state("minecraft:sand")
                .unwrap();
            game.level_spawn_falling_block(f.level, sand, [1.5, 72.0, 1.5], [0.0; 3], Default::default())
                .unwrap()
                .is_some()
        }),""",
    ),
}


def main():
    replacements = {}
    for name, (old, new) in EDITS.items():
        path = ROOT / name
        source = path.read_text()
        if source.count(old) != 1:
            raise SystemExit(f"{path}: expected one old falling-block call")
        replacements[path] = source.replace(old, new)
    for path, source in replacements.items():
        path.write_text(source)


if __name__ == "__main__":
    main()
