//! Resident and undecorated column reads through a level handle.
use super::Game;
use core::ops::ControlFlow;
use mod_api::{BlockState, ColumnRead, ColumnReadError, Heightmap, LevelReadError, LevelRef};

impl Game<'_> {
    pub(super) fn level_visit_surface_heights(
        &self,
        level: LevelRef,
        columns: &[(i32, i32)],
        heightmap: Heightmap,
        visit: &mut dyn FnMut((i32, i32), Option<i32>) -> ControlFlow<()>,
    ) -> Result<(), LevelReadError> {
        let world = if self.is_logical_server() {
            self.native_level_world_result(level)?
        } else {
            self.native_level_read_world(level)
                .ok_or(LevelReadError::Unavailable)?
        };
        let started = std::time::Instant::now();
        let mut last: Option<((i32, i32), bool)> = None;
        for &(x, z) in columns {
            let key = (x >> 4, z >> 4);
            let loaded = match last {
                Some((k, l)) if k == key => l,
                _ => {
                    let l = world.chunks.contains_key(&key);
                    last = Some((key, l));
                    l
                }
            };
            let height = loaded.then(|| self.world_column_height(world, heightmap, x, z));
            if visit((x, z), height).is_break() {
                break;
            }
        }
        self.native_read_logged(started, "height query", columns.len());
        Ok(())
    }
    pub(super) fn level_visit_column_blocks(
        &self,
        level: LevelRef,
        [x, z]: [i32; 2],
        heightmap: Heightmap,
        (below, above): (u32, u32),
        visit: &mut dyn FnMut(BlockState) -> ControlFlow<()>,
    ) -> Result<ColumnRead, ColumnReadError> {
        let world = self.native_level_world_result(level)?;
        let chunk = world.chunks.get(&(x >> 4, z >> 4));
        let base = chunk.map(|_| self.world_column_height(world, heightmap, x, z));
        let water_color = chunk.zip(base).map_or(0, |(chunk, y)| {
            world_biomes!(self).get(chunk.biome(x, y - 1, z)).water
        });
        let origin = i64::from(base.unwrap_or(0));
        if below != 0 || above != 0 {
            let index = super::api_values::block_state::current()?;
            let air = index.to_api(world.air)?;
            for y in origin - i64::from(below)..origin + i64::from(above) {
                let state = match (chunk, i32::try_from(y)) {
                    (Some(chunk), Ok(y)) if (chunk.min_y()..chunk.max_y()).contains(&y) => {
                        index.to_api(chunk.block(x & 15, y, z & 15))?
                    }
                    _ => air,
                };
                if visit(state).is_break() {
                    break;
                }
            }
        }
        Ok(mod_api::__private::built::ColumnRead { base, water_color }.into())
    }
    /// For a server mod: visit the level generator's undecorated surface for this server level
    /// (active or parked), lending a borrowed [`mod_api::TerrainColumnView`] per column and
    /// stopping on `ControlFlow::Break(())`.
    pub(super) fn level_visit_terrain_surface(
        &self,
        level: LevelRef,
        columns: &[(i32, i32)],
        visit: &mut dyn for<'a> FnMut(
            (i32, i32),
            Option<mod_api::TerrainColumnView<'a>>,
        ) -> ControlFlow<()>,
    ) -> Result<(), LevelReadError> {
        self.native_level_world_result(level)?;
        let started = std::time::Instant::now();
        match self.level_locator(level) {
            None => {
                for &(x, z) in columns {
                    if visit((x, z), None).is_break() {
                        break;
                    }
                }
            }
            Some(generator) => {
                let index = super::native_worldgen::prepare(self.pack.assets.registry)
                    .map_err(|_| LevelReadError::Unavailable)?;
                let biomes = world_biomes!(self);
                for &(x, z) in columns {
                    let view = match generator.terrain_surface(x, z) {
                        None => None,
                        Some(surface) => {
                            let state = index
                                .to_api(surface.state)
                                .map_err(|_| LevelReadError::Unavailable)?;
                            Some(
                                mod_api::__private::built::TerrainColumnView {
                                    height: surface.height,
                                    block: state.block(),
                                    water: surface.water.map(|(height, _)| height),
                                    biome: biomes.name(surface.biome),
                                }
                                .into(),
                            )
                        }
                    };
                    if visit((x, z), view).is_break() {
                        break;
                    }
                }
            }
        }
        self.native_read_logged(started, "terrain surface query", columns.len());
        Ok(())
    }
    /// A read's time logged with `SUNRISE_SPIKES`, as an edit's is.
    fn native_read_logged(&self, started: std::time::Instant, what: &str, columns: usize) {
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.options.spikes.is_some_and(|threshold| ms > threshold) {
            log::info!("Sunrise: slow mod {what} {ms:.1} ms ({columns} columns)");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use core::ops::ControlFlow;
    use mod_api::{DimensionKey, Heightmap, LevelReadError};
    use std::sync::Arc;
    use sunrise_client::vanillagen::{TerrainResources, VanillaTerrain};
    use sunrise_world::worldgen::generator::FlatSettings;

    #[test]
    #[ignore = "daily"]
    fn terrain_surface_reads_flat_layers_without_chunks() {
        with_game(|game| {
            let level = game.level_ref(game.native_lifecycle.epoch().unwrap(), game.dim.key());
            let directory = game.options.resources.join("worldgen");
            let terrain = VanillaTerrain::for_flat(
                TerrainResources {
                    data: Some(&game.pack.data_changes),
                    dir: &directory,
                    registry: game.pack.assets.registry,
                    biomes: world_biomes!(game),
                    item_data: &game.res.core.item_data,
                },
                &Default::default(),
                12345,
                &FlatSettings {
                    layers: vec![
                        ("minecraft:bedrock".into(), 1),
                        ("minecraft:dirt".into(), 2),
                        ("minecraft:grass_block".into(), 1),
                    ],
                    biome: "minecraft:plains".into(),
                    structure_overrides: Some(Vec::new()),
                    features: false,
                    lakes: false,
                },
                false,
            )
            .unwrap();
            game.end.set_locator(Some(Arc::new(terrain)));
            let before = (game.world.chunks.len(), game.streaming.pending());
            let columns = [(100000, -100000), (0, 0), (-1, 31), (100000, -100000)];
            let mut answer = Vec::new();
            game.level_visit_terrain_surface(level, &columns, &mut |pos, col| {
                answer.push((
                    pos,
                    col.map(|c| (c.height, c.block, c.water, c.biome.to_owned())),
                ));
                ControlFlow::Continue(())
            })
            .unwrap();
            assert_eq!(answer.len(), columns.len());
            let grass = mod_api::__private::block_key("minecraft:grass_block").unwrap();
            // Four layers from the overworld minimum -64, independent of the sampler.
            for (index, (pos, column)) in answer.iter().enumerate() {
                assert_eq!(*pos, columns[index]);
                let (height, block, water, biome) = column.as_ref().unwrap();
                assert_eq!(*height, -60);
                assert_eq!(*block, grass);
                assert_eq!(*water, None);
                assert_eq!(biome, "minecraft:plains");
            }
            let mut again = Vec::new();
            game.level_visit_terrain_surface(level, &columns, &mut |pos, col| {
                again.push((
                    pos,
                    col.map(|c| (c.height, c.block, c.water, c.biome.to_owned())),
                ));
                ControlFlow::Continue(())
            })
            .unwrap();
            assert_eq!(again, answer);
            // Early break stops after the first column.
            let mut count = 0;
            game.level_visit_terrain_surface(level, &columns, &mut |_, _| {
                count += 1;
                ControlFlow::Break(())
            })
            .unwrap();
            assert_eq!(count, 1);
            assert_eq!(before, (game.world.chunks.len(), game.streaming.pending()));
        });
    }

    #[test]
    #[ignore = "daily"]
    fn terrain_surface_refuses_before_sampling() {
        with_game(|game| {
            let level = game.level_ref(game.native_lifecycle.epoch().unwrap(), game.dim.key());
            let stale = game.level_ref(level.epoch(), DimensionKey::NETHER);
            game.end.set_locator(None);
            let mut visited = 0;
            assert_eq!(
                game.level_visit_terrain_surface(stale, &[(0, 0)], &mut |_, _| {
                    visited += 1;
                    ControlFlow::Continue(())
                }),
                Err(LevelReadError::Stale)
            );
            assert_eq!(visited, 0);
            // No invented column cap: more than 4096 columns visit cleanly.
            let columns = vec![(0, 0); 5000];
            let mut seen = 0;
            game.level_visit_terrain_surface(level, &columns, &mut |_, col| {
                assert!(col.is_none());
                seen += 1;
                ControlFlow::Continue(())
            })
            .unwrap();
            assert_eq!(seen, 5000);
            let mut empty_seen = 0;
            game.level_visit_terrain_surface(level, &[], &mut |_, _| {
                empty_seen += 1;
                ControlFlow::Continue(())
            })
            .unwrap();
            assert_eq!(empty_seen, 0);
            // Client refuses before sampling.
            game.options.execution_role = crate::game::execution_role::ExecutionRole::Client;
            assert_eq!(
                game.level_visit_terrain_surface(level, &[(0, 0)], &mut |_, _| {
                    visited += 1;
                    ControlFlow::Continue(())
                }),
                Err(LevelReadError::Unavailable)
            );
            assert_eq!(visited, 0);
        });
    }

    #[test]
    #[ignore = "daily"]
    fn read_columns_returns_server_water_colors_without_colormaps() {
        use std::path::Path;
        use sunrise_assets::resource_source::ResourceRoot;
        use sunrise_client::{biome::Biomes, chunk::Chunk};

        fn link_tree(src: &Path, dst: &Path) {
            #[cfg(unix)]
            std::os::unix::fs::symlink(src, dst).unwrap();
            #[cfg(not(unix))]
            {
                if src.is_dir() {
                    std::fs::create_dir(dst).unwrap();
                    for entry in std::fs::read_dir(src).unwrap() {
                        let entry = entry.unwrap();
                        link_tree(&entry.path(), &dst.join(entry.file_name()));
                    }
                } else if std::fs::hard_link(src, dst).is_err() {
                    std::fs::copy(src, dst).unwrap();
                }
            }
        }

        fn mirror_omitting(src: &Path, dst: &Path, omit: &[&[&str]]) {
            for entry in std::fs::read_dir(src).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else {
                    link_tree(&entry.path(), &dst.join(&name));
                    continue;
                };
                let under: Vec<&[&str]> = omit
                    .iter()
                    .filter_map(|path| {
                        path.split_first()
                            .filter(|(head, _)| **head == name_str)
                            .map(|(_, tail)| tail)
                    })
                    .collect();
                if under.iter().any(|rest| rest.is_empty()) {
                    continue;
                }
                let target = dst.join(&name);
                if under.is_empty() {
                    link_tree(&entry.path(), &target);
                } else {
                    std::fs::create_dir(&target).unwrap();
                    mirror_omitting(&entry.path(), &target, &under);
                }
            }
        }

        const OMIT: &[&[&str]] = &[
            &[sunrise_core::prepared_files::FILE_NAME],
            &["assets", "minecraft", "textures", "colormap", "grass.png"],
            &["assets", "minecraft", "textures", "colormap", "foliage.png"],
            &[
                "assets",
                "minecraft",
                "textures",
                "colormap",
                "dry_foliage.png",
            ],
        ];
        let scratch = tempfile::tempdir().unwrap();
        let resources = scratch.path().join("resources");
        std::fs::create_dir(&resources).unwrap();
        mirror_omitting(
            &super::super::native_entity_refs::tests::fixture_root(),
            &resources,
            OMIT,
        );
        for name in ["grass.png", "foliage.png", "dry_foliage.png"] {
            assert!(
                !resources
                    .join("assets/minecraft/textures/colormap")
                    .join(name)
                    .exists(),
                "private fixture must omit {name}"
            );
        }
        assert!(
            Biomes::load(&ResourceRoot::directory(resources.clone())).is_err(),
            "client biome load must fail when colormaps are absent"
        );

        super::super::native_entity_refs::tests::with_game_role(
            resources,
            crate::game::execution_role::ExecutionRole::DedicatedServer,
            |game| {
                game.options.dedicated = Some(Vec::new());
                assert!(game.world_biomes.is_none());
                let plains = world_biomes!(game).id("minecraft:plains");
                let swamp = world_biomes!(game).id("minecraft:swamp");
                let mut chunk = Chunk::empty(0, 0);
                for qy in 0..chunk.sections.len() as i32 * 4 {
                    for qz in 0..4 {
                        chunk.set_biome_cell(0, qy, qz, plains);
                        chunk.set_biome_cell(1, qy, qz, swamp);
                    }
                }
                game.world.chunks.insert((0, 0), Arc::new(chunk));
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.set(0, 64, 0, stone).unwrap();
                game.world.set(4, 64, 0, stone).unwrap();

                let level = game.level(game.dim.id()).unwrap();
                let air = mod_api::__private::block_key("minecraft:air").unwrap();
                let stone = mod_api::__private::block_key("minecraft:stone").unwrap();
                let heightmap = mod_api::Heightmap::WorldSurface;
                game.enter_level(super::super::Dim::Nether).unwrap();
                for (pos, base, water, expected) in [
                    ([0, 0], Some(65), 0x3F_76E4, [stone, air]),
                    ([4, 0], Some(65), 0x61_7B64, [stone, air]),
                    ([32, 0], None, 0, [air, air]),
                ] {
                    let mut actual = None;
                    level
                        .visit_surface_heights(
                            game,
                            &[(pos[0], pos[1])],
                            heightmap,
                            &mut |_, height| {
                                actual = height;
                                ControlFlow::Continue(())
                            },
                        )
                        .unwrap();
                    assert_eq!(actual, base);
                    let mut states = Vec::new();
                    let result = level
                        .visit_column_blocks(game, pos, heightmap, (1, 1), &mut |state| {
                            states.push(state.block());
                            super::ControlFlow::Continue(())
                        })
                        .unwrap();
                    assert_eq!((result.base, result.water_color), (base, water));
                    assert_eq!(states, expected);
                }
                let mut states = Vec::new();
                let result = level
                    .visit_column_blocks(
                        game,
                        [0, 0],
                        heightmap,
                        (u32::MAX, u32::MAX),
                        &mut |state| {
                            states.push(state.block());
                            super::ControlFlow::Break(())
                        },
                    )
                    .unwrap();
                assert_eq!((result.base, result.water_color), (Some(65), 0x3F_76E4));
                assert_eq!(states, [air]);
            },
        );
    }
}
