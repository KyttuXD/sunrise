//! Loaded and undecorated column queries through a level handle.
use super::Game;
use mod_api::{BlockState, ColumnRead, Heightmap, LevelReadError, LevelRef, TerrainColumn};
use std::ops::ControlFlow;

impl Game<'_> {
    pub(super) fn level_surface_height(
        &self,
        level: LevelRef,
        [x, z]: [i32; 2],
        heightmap: Heightmap,
    ) -> Result<Option<i32>, LevelReadError> {
        let world = if self.is_logical_server() {
            self.native_level_world_result(level)?
        } else {
            self.native_level_read_world(level)
                .ok_or(LevelReadError::Unavailable)?
        };
        Ok(world
            .chunks
            .contains_key(&(x >> 4, z >> 4))
            .then(|| self.edit_height(world, heightmap, x, z)))
    }

    pub(super) fn level_terrain_surface(
        &self,
        level: LevelRef,
        [x, z]: [i32; 2],
    ) -> Result<Option<TerrainColumn>, LevelReadError> {
        self.native_level_world_result(level)?;
        let Some(terrain) = self.level_locator(level) else {
            return Ok(None);
        };
        let started = std::time::Instant::now();
        let surface = terrain.terrain_surface(x, z);
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.options.spikes.is_some_and(|threshold| ms > threshold) {
            log::info!("Sunrise: slow mod terrain surface query {ms:.1} ms at ({x}, {z})");
        }
        let Some(surface) = surface else {
            return Ok(None);
        };
        let index = super::native_worldgen::prepare(self.pack.assets.registry)
            .map_err(|_| LevelReadError::Unavailable)?;
        let block = index
            .to_api(surface.state)
            .map_err(|_| LevelReadError::Unavailable)?
            .block();
        Ok(Some(
            mod_api::__private::built::TerrainColumn {
                height: surface.height,
                block,
                water: surface.water.map(|(height, _)| height),
                biome: world_biomes!(self).name(surface.biome).to_owned(),
            }
            .into(),
        ))
    }

    pub(super) fn level_visit_column_blocks(
        &self,
        level: LevelRef,
        [x, z]: [i32; 2],
        heightmap: Heightmap,
        (below, above): (u32, u32),
        visit: &mut dyn FnMut(BlockState) -> ControlFlow<()>,
    ) -> Result<ColumnRead, LevelReadError> {
        let world = self.native_level_world_result(level)?;
        let chunk = world.chunks.get(&(x >> 4, z >> 4));
        let base = chunk.map(|_| self.edit_height(world, heightmap, x, z));
        let water_color = chunk.zip(base).map_or(0, |(chunk, y)| {
            world_biomes!(self).get(chunk.biome(x, y - 1, z)).water
        });
        let total = i64::from(below) + i64::from(above);
        let start = i64::from(base.unwrap_or(0)) - i64::from(below);
        if total != 0 {
            let index = super::native_worldgen::prepare(self.pack.assets.registry)
                .map_err(|_| LevelReadError::Unavailable)?;
            for y in start..start + total {
                let state = match (chunk, i32::try_from(y)) {
                    (Some(chunk), Ok(y)) if (chunk.min_y()..chunk.max_y()).contains(&y) => {
                        chunk.block(x & 15, y, z & 15)
                    }
                    _ => world.air,
                };
                let state = index
                    .to_api(state)
                    .map_err(|_| LevelReadError::Unavailable)?;
                if visit(state).is_break() {
                    break;
                }
            }
        }
        Ok(ColumnRead::new(base, water_color))
    }
}

#[cfg(test)]
mod tests {
    use super::super::native_entity_refs::tests::with_game;
    use std::sync::Arc;
    use sunrise_client::vanillagen::{TerrainResources, VanillaTerrain};
    use sunrise_world::worldgen::generator::FlatSettings;

    #[test]
    #[ignore = "daily"]
    fn terrain_surface_reads_flat_layers_without_chunks() {
        with_game(|game| {
            game.options.dedicated = Some(Vec::new());
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
            let level = game.level(game.dim.id()).unwrap();
            let before = (game.world.chunks.len(), game.streaming.pending());
            for position in [[100000, -100000], [0, 0], [-1, 31], [100000, -100000]] {
                let column = level.terrain_surface(game, position).unwrap().unwrap();
                // Four layers from the overworld minimum -64, independent of the sampler.
                assert_eq!(column.height, -60);
                assert_eq!(
                    column.block,
                    mod_api::__private::block_key("minecraft:grass_block").unwrap()
                );
                assert_eq!(column.water, None);
                assert_eq!(column.biome, "minecraft:plains");
            }
            assert_eq!(before, (game.world.chunks.len(), game.streaming.pending()));
            game.enter_level(super::super::Dim::Nether).unwrap();
            let before = (game.world.chunks.len(), game.streaming.pending());
            let parked = level
                .terrain_surface(game, [100000, -100000])
                .unwrap()
                .unwrap();
            assert_eq!(parked.height, -60);
            assert_eq!(parked.biome, "minecraft:plains");
            assert_eq!(before, (game.world.chunks.len(), game.streaming.pending()));
        });
    }

    #[test]
    #[ignore = "daily"]
    fn terrain_surface_distinguishes_missing_generator_from_stale_level() {
        with_game(|game| {
            let level = game.level(game.dim.id()).unwrap();
            game.end.set_locator(None);
            assert_eq!(level.terrain_surface(game, [0, 0]), Ok(None));
            let stale = super::LevelRef::new(
                mod_api::WorldEpoch::new(level.epoch().get() + 1),
                level.dimension(),
            );
            assert_eq!(
                stale.terrain_surface(game, [0, 0]),
                Err(super::LevelReadError::Stale),
            );
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
                    assert_eq!(level.surface_height(game, pos, heightmap), Ok(base));
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
