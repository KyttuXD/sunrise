//! Relights and resends through the public server-level handles.

use super::native_entity_refs::tests::{run_fresh_cases, with_client_game, with_game};
use mod_api::{ChunkRefusal, LevelRef, WorldEpoch};
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

fn isolated_game(name: &str, test: impl FnOnce(&mut super::Game<'_>)) {
    let path = format!("game::native_chunk_relight_tests::{name}");
    if !run_fresh_cases(&path, "SUNRISE_TEST_CHUNK_OPERATIONS", &["level"]) {
        with_game(test);
    }
}

/// A relight is queued at once, nothing is lit during the call, and the count is the columns
/// queued (the loaded ring included); a chunk far from any loaded one queues none.
#[test]
#[ignore = "daily"]
fn a_mod_queues_the_chunks_it_wrote() {
    with_game(|game| {
        roofed(game);
        let level = game.level(game.dim.id()).unwrap();
        game.world.dirty_light.clear();
        assert_eq!(level.relight_chunks(game, &[[100, 100]]), Ok(9));
        assert_eq!(game.world.dirty_light.len(), 9);
        assert!(game.world.dirty_light.contains(&(100, 100)));
        // Asked again before the light update: nothing new is queued.
        assert_eq!(level.relight_chunks(game, &[[100, 100]]), Ok(0));
        assert_eq!(level.relight_chunks(game, &[[400, 400]]), Ok(0));
        assert_eq!(game.world.dirty_light.len(), 9);
    });
}

#[test]
#[ignore = "daily"]
fn large_relight_batches_admit_all_resident_columns() {
    isolated_game("large_relight_batches_admit_all_resident_columns", |game| {
        let level = game.level(game.dim.id()).unwrap();
        game.world.dirty_light.clear();
        // A rectangle of 4,160 resident columns, no neighbours outside its edge.
        let positions: Vec<[i32; 2]> = (100..164)
            .flat_map(|x| (100..165).map(move |z| [x, z]))
            .collect();
        for &[x, z] in &positions {
            game.world
                .chunks
                .insert((x, z), Arc::new(Chunk::empty(x, z)));
        }
        // Neither a large call nor an existing queue above the old capacity prevents admission.
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
fn operations_queue_only_on_the_owning_level() {
    isolated_game("operations_queue_only_on_the_owning_level", |game| {
        roofed(game);
        game.world.dirty_light.clear();
        let overworld = game.level("minecraft:overworld").unwrap();
        let chunk = game.chunk(overworld, [100, 100]).unwrap();
        game.enter_level(super::Dim::Nether).unwrap();
        // The same position is loaded here too; only the handle's level should be changed.
        roofed(game);
        game.world.dirty_light.clear();
        assert_eq!(overworld.relight_chunks(game, &[[100, 100]]), Ok(9));
        assert_eq!(chunk.resend(game), Ok(()));
        assert_eq!(game.dim, super::Dim::Nether);
        assert!(game.world.dirty_light.is_empty());
        assert!(game.world.chunk_resends.is_empty());

        let parked = &game.levels.parked[&super::Dim::Overworld];
        assert_eq!(parked.world.dirty_light.len(), 9);
        assert!(parked.world.dirty_light.contains(&(100, 100)));
        assert!(parked.world.chunk_resends.contains(&(100, 100)));
        assert!(parked.world.bulk_log.is_empty());

        // No connection or view names the Overworld. Its idle chunk pass still starts
        // the center's relight, while its rim waits for the missing neighbours.
        let streaming = &mut game
            .levels
            .parked
            .get_mut(&super::Dim::Overworld)
            .unwrap()
            .streaming;
        streaming.paused = false;
        streaming.remote = true; // Isolate resident lighting from generation in this fixture.
        streaming.mesh = false;
        game.run_chunk_passes(&[super::Dim::Nether]).unwrap();
        assert_eq!(game.dim, super::Dim::Overworld);
        assert!(!game.world.dirty_light.contains(&(100, 100)));
        assert!(game.world.dirty_light.contains(&(101, 100)));
        assert!(game.world.chunk_resends.contains(&(100, 100)));
        assert!(game.world.bulk_log.is_empty());
    });
}

/// A chunk that is not loaded is `NotLoaded`; a loaded one is queued to go out whole.
#[test]
#[ignore = "daily"]
fn a_chunk_is_resent_only_when_loaded() {
    with_game(|game| {
        roofed(game);
        let level = game.level(game.dim.id()).unwrap();
        let loaded = game.chunk(level, [100, 100]).unwrap();
        assert_eq!(loaded.resend(game), Ok(()));
        assert!(game.world.chunk_resends.contains(&(100, 100)));
        let unloaded = mod_api::__private::game::chunk_ref(level, [400, 400]);
        assert_eq!(unloaded.resend(game), Err(ChunkRefusal::NotLoaded));
        assert!(!game.world.chunk_resends.contains(&(400, 400)));
    });
}

#[test]
#[ignore = "daily"]
fn resend_waits_for_the_queued_light_to_land() {
    isolated_game("resend_waits_for_the_queued_light_to_land", |game| {
        roofed(game);
        game.streaming.paused = false;
        game.streaming.remote = true; // Isolate the single ready light job.
        game.streaming.mesh = false;
        let level = game.level(game.dim.id()).unwrap();
        let key = (100, 100);
        assert_eq!(level.relight_chunks(game, &[[key.0, key.1]]), Ok(9));
        assert_eq!(
            game.chunk(level, [key.0, key.1]).unwrap().resend(game),
            Ok(())
        );

        let _ = game.streaming.update(&mut game.world);
        assert!(!game.streaming.is_lit(key.0, key.1));
        assert!(game.world.chunk_resends.contains(&key));
        assert!(!game.world.bulk_log.contains(&key));

        game.streaming.wait(std::time::Duration::from_secs(5));
        let _ = game.streaming.update(&mut game.world);
        assert!(game.streaming.is_lit(key.0, key.1));
        assert!(!game.world.chunk_resends.contains(&key));
        assert!(game.world.bulk_log.contains(&key));
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
