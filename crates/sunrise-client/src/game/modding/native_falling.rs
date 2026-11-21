//! `LevelRef::spawn_falling_block`: throw a block through the same entity
//! construction path as a block that starts to fall on its own.
use super::{Game, native_level_refs::spawnable_block_position};
use mod_api::{EntityRef, Falling, LevelRef, LevelWriteError};
use sunrise_core::{geometry::Vec3, position::BlockPos};
use sunrise_world::registry::BlockStateId;

impl Game<'_> {
    pub(super) fn level_spawn_falling_block(
        &mut self,
        level: LevelRef,
        state: BlockStateId,
        pos: [f64; 3],
        velocity: [f64; 3],
        falling: Falling,
    ) -> Result<Option<EntityRef>, LevelWriteError> {
        let world = self.native_level_world_result(level)?;
        if level.dimension() != self.dim.key() {
            return Err(LevelWriteError::Unavailable);
        }
        let [x, y, z] = pos.map(|v| v.floor() as i32);
        if !pos.iter().chain(&velocity).all(|v| v.is_finite())
            || !spawnable_block_position([x, y, z])
            || !world.loaded(x, z)
            || self.res.facts().is_air(state)
            || self.entity_ids_exhausted()
        {
            return Ok(None);
        }
        let id = self.add_fresh_falling_block(
            state,
            BlockPos::new(x, y, z),
            Vec3::new(pos[0], pos[1], pos[2]),
            Vec3::new(velocity[0], velocity[1], velocity[2]),
            falling.drop_item,
            falling
                .hurts
                .then_some(sunrise_client::falling_block::Damage::anvil().per_distance),
        );
        Ok(self
            .native_joins_flush(false, id)
            .then_some(EntityRef::new(level.epoch(), id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::FallingBlock;

    use std::sync::{Arc, Mutex};
    use sunrise_mod_runtime::attachments::{
        AttachmentType, Attachments, HasAttachmentLifetime, Holder, index::AttachmentIndex,
    };

    static VALUE: AttachmentType<i32> = AttachmentType {
        namespace: "test",
        name: "falling_lifetime",
        default: || 0,
        codec: None,
        copy_on_death: false,
        sync: false,
    };

    #[test]
    #[ignore = "daily"]
    fn falling_tick_preserves_its_owner_on_success_and_collision_error() {
        bound_falling_case(false, |game, _| {
            assert!(game.spawn_remote_projectile(
                "minecraft:falling_block",
                &sunrise_protocol::packets::play::AddEntity {
                    id: 72,
                    uuid: 72,
                    kind: 0,
                    position: [4.5, 70.0, 0.5],
                    velocity: [0.0; 3],
                    pitch: 0,
                    yaw: 0,
                    head_yaw: 0,
                    data: game.falling[0].state.0 as i32
                },
                (0.0, 0.0),
            ));
            let expected: Vec<_> = game.falling.iter().map(|f| (f.id, f.join)).collect();
            game.tick_one_falling(0, &[]).unwrap();
            assert_eq!(
                game.falling
                    .iter()
                    .map(|f| (f.id, f.join))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(game.falling[0].age, 1);
            assert!(game.falling[0].position.y < 70.0);
            game.tick_one_falling(1, &[]).unwrap();
            assert_eq!(
                game.falling
                    .iter()
                    .map(|f| (f.id, f.join))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!([game.falling[0].age, game.falling[1].age], [1, 1]);
            // A corrupt resident palette makes the real collision lookup fail.
            Arc::make_mut(game.world.chunks.get_mut(&(0, 0)).unwrap()).set(0, 69, 0, 65_535);
            let position = game.falling[0].position;
            assert!(game.tick_one_falling(0, &[]).is_err());
            assert_eq!(
                game.falling
                    .iter()
                    .map(|f| (f.id, f.join))
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(game.falling[0].position, position);
            assert_eq!(game.falling[0].age, 1);
        });
    }

    #[test]
    #[ignore = "daily"]
    fn falling_client_tick_commits_state_without_detaching_its_owner() {
        bound_falling_case(true, |game, _| {
            game.remote_projectile_data(
                71,
                &[(
                    8,
                    10,
                    sunrise_protocol::entity_data::Value::Pos([8, 90, -4]),
                )],
            );
            assert_eq!(game.falling[0].start_pos, BlockPos::new(8, 90, -4));
            for age in 1..=3 {
                let previous = game.falling[0].position;
                game.tick_remote_projectiles(|_, at| at);
                assert_eq!(game.falling[0].age, age);
                assert_eq!(game.falling[0].previous, previous);
                assert!(game.falling[0].position.y < previous.y);
                assert!(game.falling[0].velocity.y < 0.0);
            }
        });
    }

    #[test]
    #[ignore = "daily"]
    fn falling_parked_swap_preserves_state_and_owner_until_discard() {
        bound_falling_case(false, |game, index| {
            let home = game.dim;
            game.tick_one_falling(0, &[]).unwrap();
            let position = game.falling[0].position;
            game.enter_level(super::super::Dim::Nether).unwrap();
            assert!(game.falling.is_empty());
            assert_eq!(
                index
                    .lock()
                    .unwrap()
                    .snapshot(&("test".into(), "falling_lifetime".into()))
                    .len(),
                1
            );
            game.enter_level(home).unwrap();
            assert_eq!(game.falling.len(), 1);
            assert_eq!(game.falling[0].id, 71);
            assert_eq!(game.falling[0].age, 1);
            assert_eq!(game.falling[0].position, position);
        });
    }

    #[test]
    #[ignore = "daily"]
    fn falling_lifecycle_commands_unload_park_and_remote() {
        use super::super::{
            commands::Target,
            entity::taken_pass_tests::{LifecycleFixture, RemoteLifecycle, SlotIdentities},
        };
        let mut identities = SlotIdentities::default();
        LifecycleFixture {
            seed: |game, entries| {
                game.falling.clear();
                let state = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:sand")
                    .unwrap();
                for &(id, at) in entries {
                    assert!(game.spawn_remote_projectile(
                        "minecraft:falling_block",
                        &sunrise_protocol::packets::play::AddEntity {
                            id: id as i32,
                            uuid: u128::from(id),
                            kind: 0,
                            position: at,
                            velocity: [0.0; 3],
                            pitch: 0,
                            yaw: 0,
                            head_yaw: 0,
                            data: state.0 as i32
                        },
                        (0.0, 0.0),
                    ));
                }
            },
            ids: |game| game.falling.iter().map(|f| f.id).collect(),
            target: Target::Falling,
            unloads_with_chunk: true,
            remote: Some(RemoteLifecycle {
                prepare: |game| game.clear_remote_projectiles(),
                apply: |game, remove, _| game.remove_remote_projectile(remove),
            }),
        }
        .run_with(|game| {
            identities.observe(
                &mut game.falling,
                game.native_lifecycle.epoch().unwrap(),
                |f| f.id,
            );
        });
    }

    fn bound_falling_case(
        client: bool,
        exercise: impl FnOnce(&mut Game<'_>, &Arc<Mutex<AttachmentIndex<mod_api::EntityRef>>>),
    ) {
        let test = |game: &mut Game<'_>| {
            game.world
                .chunks
                .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
            let state = game
                .pack
                .assets
                .registry
                .default_state("minecraft:sand")
                .unwrap();
            let at = Vec3::new(0.5, 70.0, 0.5);
            game.falling.push(FallingBlock {
                on_ground: false,
                fire_ticks: 0,
                sync_velocity: false,
                attachment_lifetime: Default::default(),
                rotation: [0.0; 2],
                join: Default::default(),
                id: 71,
                state,
                start_pos: BlockPos::new(0, 70, 0),
                position: at,
                previous: at,
                velocity: Vec3::ZERO,
                age: 0,
                no_gravity: false,
                portal: Default::default(),
                drop_item: false,
                fall_distance: 0.0,
                stuck: Vec3::ZERO,
                damage: Default::default(),
                saved: None,
            });
            let id = game.falling[0].id;
            let holder = mod_api::EntityRef::new(game.native_lifecycle.epoch().unwrap(), id);
            let index = Arc::new(Mutex::new(AttachmentIndex::default()));
            let mut values = Attachments::default();
            VALUE.set(&mut values, Holder::Entity(id), 10).unwrap();
            let original = game.falling.update(0, |f| {
                f.attachment_lifetime().bind(&mut values, &index, holder);
                f.attachment_lifetime().registration().unwrap()
            });
            let key = ("test".into(), "falling_lifetime".into());
            let before = index.lock().unwrap().snapshot(&key);
            assert_eq!(before.len(), 1);
            exercise(game, &index);
            let current = game
                .falling
                .update(0, |f| f.attachment_lifetime().registration().unwrap());
            assert!(original.same_registration(&current));
            assert_eq!(index.lock().unwrap().snapshot(&key).len(), 1);
            assert_eq!(VALUE.get(&mut values, Holder::Entity(id)), Ok(10));
            // Exercise the production clear before any fixture cleanup can mask it.
            game.clear_remote_projectiles();
            assert!(game.falling.is_empty());
            assert!(index.lock().unwrap().snapshot(&key).is_empty());
            assert_eq!(before[0].entry.read(&VALUE), Ok(&10));
        };
        if client {
            super::super::native_entity_refs::tests::with_client_game(test);
        } else {
            super::super::native_entity_refs::tests::with_game(test);
        }
    }

    #[test]
    #[ignore = "daily"]
    fn falling_block_join_veto_uses_ordinary_admission() {
        struct Veto;
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for Veto {
            fn entity_join_level(
                &self,
                _: &mut dyn mod_api::ServerHost,
                event: &mut mod_api::EntityJoinLevelEvent<'_>,
            ) -> mod_api::Flow {
                if event.kind == "minecraft:falling_block" {
                    mod_api::Flow::Cancel
                } else {
                    mod_api::Flow::Continue
                }
            }
        }

        crate::game::in_own_process(
            "game::native_falling::tests::falling_block_join_veto_uses_ordinary_admission",
            || {
                let mods: mod_api::Mods =
                    vec![mod_api::ModEntry::new("veto", Box::new(Veto))].into();
                mod_platform::testing::with_mods(mods, || {
                    super::super::native_entity_refs::tests::with_game(|game| {
                        use super::super::api_values::TryIntoApi;

                        game.world
                            .chunks
                            .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
                        let level = game.level("minecraft:overworld").unwrap();
                        let state = game
                            .pack
                            .assets
                            .registry
                            .default_state("minecraft:sand")
                            .unwrap()
                            .try_into_api()
                            .unwrap();
                        let spawn = |game: &mut Game<'_>| {
                            level.spawn_falling_block(
                                game,
                                state,
                                [1.5, 70.0, 1.5],
                                [0.0; 3],
                                Falling::default(),
                            )
                        };
                        assert_eq!(spawn(game), Ok(None));
                        assert!(game.falling.is_empty());

                        let seal = game.transaction_open();
                        assert!(spawn(game).unwrap().is_some());
                        assert_eq!(game.falling.len(), 1);
                        game.transaction_close(&seal, true);
                        assert!(game.falling.is_empty());
                    });
                });
            },
        );
    }

    #[test]
    #[ignore = "daily"]
    fn falling_block_spawns_keep_live_motion_and_flags_past_the_old_quota() {
        crate::game::in_own_process(
            "game::native_falling::tests::falling_block_spawns_keep_live_motion_and_flags_past_the_old_quota",
            || {
                super::super::native_entity_refs::tests::with_game(|game| {
                    use super::super::api_values::TryIntoApi;

                    game.world
                        .chunks
                        .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
                    let level = game.level("minecraft:overworld").unwrap();
                    let sand = game
                        .pack
                        .assets
                        .registry
                        .default_state("minecraft:red_sand")
                        .unwrap();
                    let state = sand.try_into_api().unwrap();
                    let pos = [2.25, 70.5, 3.75];
                    let mut falling = Falling::default();
                    falling.drop_item = false;
                    falling.hurts = true;
                    assert_eq!(
                        level.spawn_falling_block(game, state, pos, [f64::NAN, 0.0, 0.0], falling),
                        Ok(None)
                    );
                    assert_eq!(
                        level.spawn_falling_block(
                            game,
                            state,
                            [2.25, 20_000_000.0, 3.75],
                            [0.0; 3],
                            falling
                        ),
                        Ok(None)
                    );
                    assert!(game.falling.is_empty());

                    let source = game
                        .pack
                        .assets
                        .registry
                        .default_state("minecraft:stone")
                        .unwrap();
                    assert!(game.world.set_stored(2, 70, 3, source));
                    let mut last = None;
                    for _ in 0..=1024 {
                        last = level
                            .spawn_falling_block(game, state, pos, [12.5, -8.0, 0.25], falling)
                            .unwrap();
                        assert!(last.is_some());
                    }
                    assert_eq!(game.falling.len(), 1025);
                    let thrown = &game.falling[1024];
                    assert_eq!(last, Some(EntityRef::new(level.epoch(), thrown.id)));
                    assert_eq!(thrown.state, sand);
                    assert_eq!(thrown.start_pos, BlockPos::new(2, 70, 3));
                    assert_eq!(thrown.position, Vec3::new(2.25, 70.5, 3.75));
                    assert_eq!(thrown.previous, thrown.position);
                    assert_eq!(thrown.velocity, Vec3::new(12.5, -8.0, 0.25));
                    assert!(!thrown.drop_item);
                    assert!(thrown.damage.hurt_entities);
                    assert_eq!(thrown.damage.per_distance, 2.0);
                    assert_eq!(thrown.damage.max, 40);
                    assert_eq!(game.world.block(2, 70, 3), source);
                })
            },
        );
    }
}
