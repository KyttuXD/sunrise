//! Relighting and resending through the public server-level handles.

use super::native_entity_refs::tests::{run_fresh_cases, with_client_game, with_game};
use mod_api::{ChunkRefusal, LevelRef, LightLayer, WorldEpoch};
use std::sync::Arc;
use sunrise_client::chunk::Chunk;

/// A stone roof over the chunk (100, 100), all eight neighbours loaded around it.
fn roofed(game: &mut super::Game<'_>) {
    let stone = game
        .pack
        .assets
        .registry
        .default_state("minecraft:stone")
        .unwrap();
    for dz in -1..=1 {
        for dx in -1..=1 {
            let mut chunk = Chunk::empty(100 + dx, 100 + dz);
            if dx == 0 && dz == 0 {
                for (x, z) in (0..16).flat_map(|x| (0..16).map(move |z| (x, z))) {
                    chunk.set(x, 100, z, stone.0);
                }
            }
            game.world
                .chunks
                .insert((100 + dx, 100 + dz), Arc::new(chunk));
        }
    }
}

fn stale(level: LevelRef) -> LevelRef {
    LevelRef::new(WorldEpoch::new(level.epoch().get() + 1), level.dimension())
}

/// A relight is queued at once, nothing is lit during the call, and the count is the columns
/// queued (the loaded ring included); an unloaded entry does not queue its loaded neighbours.
#[test]
#[ignore = "daily"]
fn a_mod_queues_the_chunks_it_wrote() {
    isolated_game("a_mod_queues_the_chunks_it_wrote", |game| {
        roofed(game);
        let level = game.level(game.dim.id()).unwrap();
        game.world.dirty_light.clear();
        assert_eq!(level.relight_chunks(game, &[[102, 100]]), Ok(0));
        assert_eq!(level.relight_chunks(game, &[[100, 100]]), Ok(9));
        assert_eq!(game.world.dirty_light.len(), 9);
        assert!(game.world.dirty_light.contains(&(100, 100)));
        // Asked again before the light update: nothing new is queued.
        assert_eq!(level.relight_chunks(game, &[[100, 100]]), Ok(0));
        assert_eq!(level.relight_chunks(game, &[[400, 400]]), Ok(0));
        assert_eq!(game.world.dirty_light.len(), 9);
    });
}

fn isolated_game(name: &str, test: impl FnOnce(&mut super::Game<'_>)) {
    let path = format!("game::native_chunk_relight_tests::{name}");
    if !run_fresh_cases(&path, "SUNRISE_TEST_CHUNK_OPERATIONS", &["level"]) {
        with_game(test);
    }
}

#[test]
#[ignore = "daily"]
fn large_relight_batches_admit_all_resident_columns() {
    isolated_game("large_relight_batches_admit_all_resident_columns", |game| {
        let level = game.level(game.dim.id()).unwrap();
        game.world.dirty_light.clear();
        // A rectangle of 4,160 resident columns, with no loaded neighbours outside its edge.
        let positions: Vec<[i32; 2]> = (100..164)
            .flat_map(|x| (100..165).map(move |z| [x, z]))
            .collect();
        for &[x, z] in &positions {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
        }
        assert_eq!(level.relight_chunks(game, &positions), Ok(4160));
        game.world
            .chunks
            .insert((500, 500), Arc::new(Chunk::empty(500, 500)));
        assert_eq!(level.relight_chunks(game, &[[500, 500]]), Ok(1));
        assert_eq!(game.world.dirty_light.len(), 4161);
    });
}

/// A handle of an earlier open of the world is `Stale`, for the level and for a chunk of it.
#[test]
#[ignore = "daily"]
fn a_stale_handle_is_refused_by_name() {
    with_game(|game| {
        roofed(game);
        let level = game.level(game.dim.id()).unwrap();
        assert_eq!(
            stale(level).relight_chunks(game, &[[100, 100]]),
            Err(ChunkRefusal::Stale)
        );
        let chunk = mod_api::__private::game::chunk_ref(stale(level), [100, 100]);
        assert_eq!(chunk.resend(game), Err(ChunkRefusal::Stale));
    });
}

#[test]
#[ignore = "daily"]
fn parked_relight_publishes_only_the_newest_light_on_its_own_level() {
    use super::Dim;
    use std::time::Duration;

    isolated_game(
        "parked_relight_publishes_only_the_newest_light_on_its_own_level",
        |game| {
            roofed(game);
            game.world.dirty_light.clear();
            game.streaming.paused = false;
            game.streaming.mesh = false;
            let overworld = game.level(Dim::Overworld.id()).unwrap();
            game.enter_level(Dim::Nether).unwrap();
            roofed(game);
            let nether = game.level(Dim::Nether.id()).unwrap();
            let glowstone = game
                .pack
                .assets
                .registry
                .default_state("minecraft:glowstone")
                .unwrap();
            let key = (100, 100);
            let lamp = [1608, 80, 1608];
            assert_eq!(overworld.relight_chunks(game, &[[100, 100]]), Ok(9));
            game.run_view_passes(&[Dim::Nether]).unwrap();
            {
                let parked = game.levels.parked.get_mut(&Dim::Overworld).unwrap();
                // Only the middle column has its full 3×3 region; ensure its old job has finished
                // computing before modifying the blocks, without publishing the result yet.
                assert!(parked.streaming.light_in_flight(key));
                parked.streaming.wait(Duration::from_secs(5));
                Arc::make_mut(parked.world.chunks.get_mut(&key).unwrap()).set(
                    lamp[0],
                    lamp[1],
                    lamp[2],
                    glowstone.0,
                );
            }
            assert_eq!(overworld.relight_chunks(game, &[[100, 100]]), Ok(1));
            game.run_view_passes(&[Dim::Nether]).unwrap();
            {
                let parked = game.levels.parked.get_mut(&Dim::Overworld).unwrap();
                assert!(
                    !parked.world.chunks.get(&key).unwrap().lit,
                    "the pre-edit snapshot must not publish"
                );
                parked.streaming.wait(Duration::from_secs(5));
            }
            game.run_view_passes(&[Dim::Nether]).unwrap();
            assert_eq!(game.dim, Dim::Nether);
            assert_eq!(
                overworld.brightness(game, LightLayer::Block, lamp),
                Some(15)
            );
            assert_eq!(nether.brightness(game, LightLayer::Block, lamp), Some(0));
        },
    );
}

#[test]
#[ignore = "daily"]
fn a_resend_resolves_the_parked_levels_loaded_chunk() {
    use super::Dim;

    isolated_game("a_resend_resolves_the_parked_levels_loaded_chunk", |game| {
        roofed(game);
        let level = game.level(Dim::Overworld.id()).unwrap();
        let loaded = game.chunk(level, [100, 100]).unwrap();
        let _packets = super::view_pass::tests::host_a_viewer(game);
        game.enter_level(Dim::Nether).unwrap();
        game.world
            .chunks
            .insert((400, 400), Arc::new(Chunk::empty(400, 400)));
        assert_eq!(loaded.resend(game), Ok(()));
        let unloaded = mod_api::__private::game::chunk_ref(level, [400, 400]);
        assert_eq!(unloaded.resend(game), Err(ChunkRefusal::NotLoaded));
        assert!(game.world.chunk_resends.is_empty());
        let parked = game.levels.parked_world(Dim::Overworld).unwrap();
        assert_eq!(parked.chunk_resends.len(), 1);
        assert!(parked.chunk_resends.contains(&(100, 100)));
    });
}

/// On a client both are `Client`.
#[test]
#[ignore = "daily"]
fn a_client_cannot_relight_or_resend() {
    with_client_game(|game| {
        roofed(game);
        let level = LevelRef::new(WorldEpoch::new(0), game.dim.key());
        assert_eq!(
            level.relight_chunks(game, &[[100, 100]]),
            Err(ChunkRefusal::Client)
        );
        let chunk = mod_api::__private::game::chunk_ref(level, [100, 100]);
        assert_eq!(chunk.resend(game), Err(ChunkRefusal::Client));
    });
}
