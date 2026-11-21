//! Client crack state belongs to one level and expires independently of new updates.
use super::{Game, Mining};

pub(super) struct ClientCrack {
    pub pos: [i32; 3],
    pub stage: u8,
    pub updated_at: i64,
    pub positions: Vec<[i32; 3]>,
}

impl Game<'_> {
    /// Preserve the stage's stored display answer across prediction updates.
    pub(super) fn set_client_mining(&mut self, mut mining: Option<Mining>) {
        if let Some(next) = &mut mining
            && let Some(old) = self.mining.as_mut().filter(|old| old.pos == next.pos)
        {
            next.crack = old.crack.take();
        }
        self.mining = mining;
        self.update_local_crack();
    }

    pub(super) fn update_local_crack(&mut self) {
        if !self.native_window() {
            return;
        }
        let Some(mining) = self.mining.as_mut() else {
            return;
        };
        let Some(stage) = mining.stage() else {
            mining.crack = None;
            return;
        };
        let pos = mining.pos;
        if mining
            .crack
            .as_ref()
            .is_some_and(|crack| crack.stage == stage)
        {
            return;
        }
        // Mark this stage before dispatch, so a reentrant update cannot ask twice.
        mining.crack = Some(ClientCrack {
            pos,
            stage,
            updated_at: self.world.game_time,
            positions: Vec::new(),
        });
        self.with_break_progress_positions(pos, stage, |game, positions| {
            if let Some(mining) = &mut game.mining
                && mining.pos == pos
                && mining.stage() == Some(stage)
                && let Some(crack) = &mut mining.crack
            {
                crack.positions = positions;
            }
        });
    }

    pub(super) fn expire_client_cracks(&mut self) {
        let now = self.world.game_time;
        self.destroying
            .retain(|_, crack| now.saturating_sub(crack.updated_at) <= 400);
    }

    pub(super) fn reset_client_world(&mut self, seed: i64) {
        self.destroying.clear();
        self.mining = None;
        self.world = Game::world_for_mods(self.res.facts().air, seed);
    }
}

#[cfg(test)]
mod tests {
    use crate::game::{
        Mining, native_client_session::tests::logged_client,
        native_entity_refs::tests::with_client_game,
    };
    use mod_api::{
        BlockPart, BlockPos, BreakProgressPositionsEvent, ClientHost, Hooks, ModEntry, Registrar,
        RegistrationContext,
    };
    use std::sync::{Arc, Mutex};
    use sunrise_client::chunk::Chunk;

    struct ExtraPositions {
        calls: Arc<Mutex<Vec<u8>>>,
        extra: Vec<[i32; 3]>,
        reenter: bool,
    }
    #[mod_api::sunrise_part]
    impl BlockPart for ExtraPositions {
        fn break_progress_positions(
            &self,
            host: &mut dyn ClientHost,
            event: &mut BreakProgressPositionsEvent<'_>,
        ) {
            let count = {
                let mut calls = self.calls.lock().unwrap();
                calls.push(event.progress);
                calls.len()
            };
            let p = event.pos;
            // Bound the fixture's recursion so a regression fails the assertion
            // rather than overflowing the test process's stack.
            if self.reenter && count == 1 {
                assert!(event.level.set_client_break_progress(
                    host,
                    -1,
                    [p.x, p.y, p.z],
                    i32::from(event.progress),
                ));
            }
            for position in [
                BlockPos::new(p.x + 1, p.y, p.z),
                BlockPos::new(p.x, p.y, p.z + 1),
                BlockPos::new(p.x + 1, p.y, p.z + 1),
            ] {
                (event.add_position)(position);
            }
            for &[x, y, z] in &self.extra {
                (event.add_position)(BlockPos::new(x, y, z));
            }
        }
    }
    struct Register {
        calls: Arc<Mutex<Vec<u8>>>,
        extra: Vec<[i32; 3]>,
        reenter: bool,
    }
    #[mod_api::__subscriptions]
    impl Hooks for Register {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let key = r.block_key("minecraft:stone");
            r.block_part(
                key,
                ExtraPositions {
                    calls: self.calls.clone(),
                    extra: self.extra.clone(),
                    reenter: self.reenter,
                },
            );
        }
    }

    #[test]
    #[ignore = "daily"]
    fn crack_positions_are_scoped_stored_and_cleared_for_both_sources() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods = vec![ModEntry::new(
            "cracks",
            Box::new(Register {
                calls: calls.clone(),
                extra: Vec::new(),
                reenter: false,
            }),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_client_game(|game| {
                let _server = logged_client(game);
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                let dirt = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:dirt")
                    .unwrap();
                let pos = [4, 64, 4];
                let four = [[4, 64, 4], [4, 64, 5], [5, 64, 4], [5, 64, 5]];
                for p in four {
                    game.world.set(p[0], p[1], p[2], stone).unwrap();
                }
                game.settings.improved_transparency = false;
                let camera = glam::DVec3::new(4.5, 65.0, 4.5);
                game.destroy_block_progress(10, pos, 2);
                let rendered = game.cracks(camera);
                assert_eq!(rendered.len(), 4);
                let sprite = &game.gui.atlas.sprites["block/destroy_stage_2"];
                assert!(rendered.iter().all(|(_, uv)| *uv == [sprite.u0, sprite.v0]));
                let mut expected = four.map(|p| (p, 2)).to_vec();
                expected.sort_unstable();
                let mut drawn = game.crack_stages(camera);
                drawn.sort_unstable();
                assert_eq!(drawn, expected);
                for _ in 0..3 {
                    assert_eq!(game.crack_stages(camera).len(), 4);
                }
                game.destroy_block_progress(10, pos, 2);
                assert_eq!(
                    *calls.lock().unwrap(),
                    [2],
                    "frames and unchanged stages never redispatch"
                );
                game.destroy_block_progress(10, pos, 7);
                assert!(
                    game.crack_stages(camera)
                        .iter()
                        .all(|(_, stage)| *stage == 7)
                );
                let rendered = game.cracks(camera);
                assert_eq!(rendered.len(), 4);
                let sprite = &game.gui.atlas.sprites["block/destroy_stage_7"];
                assert!(rendered.iter().all(|(_, uv)| *uv == [sprite.u0, sprite.v0]));
                assert_eq!(*calls.lock().unwrap(), [2, 7]);
                for clear in [-1, 10] {
                    game.destroy_block_progress(10, pos, clear);
                    assert!(game.crack_stages(camera).is_empty());
                }
                let mining = |progress| {
                    Some(Mining {
                        pos,
                        progress,
                        per_tick: 0.0,
                        ticks: 1,
                        crack: None,
                    })
                };
                game.set_client_mining(mining(0.35));
                assert_eq!(
                    game.crack_stages(camera).len(),
                    4,
                    "local prediction includes the extras"
                );
                assert_eq!(game.cracks(camera).len(), 4);
                assert!(
                    game.crack_stages(camera)
                        .iter()
                        .all(|(_, stage)| *stage == 3)
                );
                game.set_client_mining(mining(0.39));
                assert_eq!(*calls.lock().unwrap(), [2, 7, 3]);
                game.set_client_mining(mining(0.45));
                assert!(
                    game.crack_stages(camera)
                        .iter()
                        .all(|(_, stage)| *stage == 4)
                );
                game.set_client_mining(None);
                assert!(game.crack_stages(camera).is_empty());
                game.world.set(9, 64, 4, dirt).unwrap();
                game.destroy_block_progress(11, [9, 64, 4], 1);
                assert_eq!(game.crack_stages(camera), [([9, 64, 4], 1)]);
                assert_eq!(
                    *calls.lock().unwrap(),
                    [2, 7, 3, 4],
                    "an unsubscribed state is not dispatched"
                );
                game.destroy_block_progress(11, [9, 64, 4], -1);
                game.destroy_block_progress(10, pos, 5);
                game.world.game_time += 100;
                let calls_before = calls.lock().unwrap().len();
                game.destroy_block_progress(10, pos, 5);
                assert_eq!(calls.lock().unwrap().len(), calls_before);
                game.world.game_time += 400;
                game.expire_client_cracks();
                assert_eq!(game.destroying.len(), 1, "400 ticks is still live");
                game.world.game_time += 1;
                game.expire_client_cracks();
                assert!(
                    game.destroying.is_empty(),
                    "expiry drops the stored extras too"
                );
                game.destroy_block_progress(10, pos, 0);
                game.set_client_mining(mining(0.25));
                game.reset_client_world(0);
                assert!(game.destroying.is_empty());
                assert!(game.mining.is_none());
                assert!(game.crack_stages(camera).is_empty());
            })
        });
    }

    #[test]
    #[ignore = "daily"]
    fn supplied_cracks_follow_camera_culling_and_keep_the_highest_live_stage() {
        let origin = [4, 64, 4];
        let distant = [40, 64, 4];
        // 72 distinct extras in view, including cells more than eight blocks from the origin.
        let patch = (5..=13).flat_map(|x| (4..=11).map(move |z| [x, 64, z]));
        let expected = [origin, [4, 64, 5]]
            .into_iter()
            .chain(patch)
            .collect::<Vec<_>>();
        let mods = vec![ModEntry::new(
            "wide-cracks",
            Box::new(Register {
                calls: Arc::new(Mutex::new(Vec::new())),
                extra: expected
                    .iter()
                    .copied()
                    .chain([origin, distant, [i32::MIN, i32::MAX, i32::MIN]])
                    .collect(),
                reenter: false,
            }),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_client_game(|game| {
                let _server = logged_client(game);
                for cx in [0, 2] {
                    game.world
                        .chunks
                        .insert((cx, 0), Arc::new(Chunk::empty(cx, 0)));
                }
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                for &[x, y, z] in expected.iter().chain([&distant]) {
                    game.world.set(x, y, z, stone).unwrap();
                }
                game.settings.improved_transparency = false;
                let camera = glam::DVec3::new(4.5, 64.5, 4.5);
                game.destroy_block_progress(1, origin, 6);
                game.destroy_block_progress(2, origin, 3);
                for (breaker, stage) in [(1, 6), (2, 3)] {
                    let rendered = game.cracks(camera);
                    let mut drawn: Vec<_> = rendered.iter().map(|(pos, _)| *pos).collect();
                    drawn.sort_unstable();
                    assert_eq!(drawn, expected);
                    let sprite = &game.gui.atlas.sprites[&format!("block/destroy_stage_{stage}")];
                    let uv = [sprite.u0, sprite.v0];
                    assert!(rendered.iter().all(|(_, actual)| *actual == uv));
                    let moved_camera = glam::DVec3::new(40.5, 64.5, 4.5);
                    assert!(game.cracks(moved_camera).contains(&(distant, uv)));
                    game.destroy_block_progress(breaker, origin, -1);
                }
                assert!(game.cracks(camera).is_empty());
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn reentrant_same_stage_client_crack_update_dispatches_once() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mods = vec![ModEntry::new(
            "reentrant-cracks",
            Box::new(Register {
                calls: calls.clone(),
                extra: Vec::new(),
                reenter: true,
            }),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_client_game(|game| {
                let _server = logged_client(game);
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                let pos = [4, 64, 4];
                game.world.set(pos[0], pos[1], pos[2], stone).unwrap();
                game.destroy_block_progress(-1, pos, 3);
                assert_eq!(*calls.lock().unwrap(), [3]);
                let crack = &game.destroying[&-1];
                assert_eq!((crack.pos, crack.stage), (pos, 3));
                assert_eq!(crack.positions, [[5, 64, 4], [4, 64, 5], [5, 64, 5]]);
            })
        });
    }

    #[test]
    #[ignore = "daily"]
    fn client_cracks_clear_expire_and_do_not_cross_worlds() {
        super::super::native_entity_refs::tests::with_client_game(|game| {
            game.destroy_block_progress(-10, [1, 64, 1], 4);
            game.destroy_block_progress(-11, [2, 64, 1], 9);
            assert_eq!(game.destroying.len(), 2);
            game.destroy_block_progress(-10, [1, 64, 1], -1);
            assert!(!game.destroying.contains_key(&-10));
            assert!(game.destroying.contains_key(&-11));
            game.world.game_time += 401;
            game.tick().unwrap();
            assert!(game.destroying.is_empty());
            game.destroy_block_progress(-11, [2, 64, 1], 3);
            game.reset_client_world(0);
            assert!(game.destroying.is_empty());
        });
    }
}
