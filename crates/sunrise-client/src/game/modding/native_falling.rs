//! Falling blocks for native server mods (docs/modding/src/how-to/world/spawning.md,
//! "Spawning from a level"): `LevelRef::spawn_falling_block` and the canonical
//! `FallingBlockEntity` construction and admission shared with `LevelEvent::FallingBlock`.
use super::{Dim, Game};
use sunrise_core::geometry::Vec3;
use sunrise_world::registry::BlockStateId;

impl Game<'_> {
    /// `LevelRef::spawn_falling_block`: spawn in the named open server level,
    /// leaving its block at `pos` alone. Admission does not require a loaded
    /// chunk: `ServerLevel.addFreshEntity` does not use `/summon`'s placement checks.
    pub(super) fn level_spawn_falling_block(
        &mut self,
        level: mod_api::LevelRef,
        state: BlockStateId,
        pos: [f64; 3],
        velocity: [f64; 3],
        falling: mod_api::Falling,
    ) -> Result<Option<mod_api::EntityRef>, mod_api::LevelWriteError> {
        self.native_level_world_result(level)?;
        if !pos.iter().chain(&velocity).all(|v| v.is_finite()) {
            return Ok(None);
        }
        let dim = match level.dimension() {
            mod_api::DimensionKey::OVERWORLD => Dim::Overworld,
            mod_api::DimensionKey::NETHER => Dim::Nether,
            mod_api::DimensionKey::END => Dim::End,
            key => Dim::Extra(key),
        };
        self.with_level_in_place(dim, |game| {
            use sunrise_client::falling_block::Damage;
            let mut damage = Damage::falling(&game.res.facts().get(state).name);
            if falling.hurts {
                damage = Damage {
                    cancel_drop: damage.cancel_drop,
                    ..Damage::anvil()
                };
            }
            game.spawn_falling_block_entity(
                state,
                Vec3::new(pos[0], pos[1], pos[2]),
                Vec3::new(velocity[0], velocity[1], velocity[2]),
                falling.drop_item,
                damage,
            )
            .map(|id| mod_api::EntityRef::new(level.epoch(), id))
        })
        .ok_or(mod_api::LevelWriteError::Stale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::game::FallingBlock;
    use std::sync::{Arc, Mutex};
    use sunrise_core::position::BlockPos;
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

    fn with_falling_game(test: impl FnOnce(&mut Game<'_>)) {
        use mod_api::__private::{ModPhase, PhaseOwner};

        super::super::native_entity_refs::tests::with_game(|game| {
            let _world = PhaseOwner::scope(ModPhase::World);
            test(game);
        });
    }

    #[test]
    #[ignore = "daily"]
    fn level_spawn_falling_block_preserves_state_flags_and_exact_velocity() {
        use crate::game::api_values::TryIntoApi;
        use mod_api::{
            BlockState, BlockStateError, Falling, LevelRef, LevelWriteError, WorldEpoch,
        };

        with_falling_game(|game| {
            game.world
                .chunks
                .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
            let level = game.level(game.dim.id()).unwrap();
            let stone = game
                .pack
                .assets
                .registry
                .default_state("minecraft:stone")
                .unwrap();
            game.world.set(1, 70, 2, stone).unwrap();
            let sand = game
                .pack
                .assets
                .registry
                .default_state("minecraft:sand")
                .unwrap();
            let anvil = game
                .pack
                .assets
                .registry
                .default_state("minecraft:anvil")
                .unwrap();
            let air = game
                .pack
                .assets
                .registry
                .default_state("minecraft:air")
                .unwrap();
            let api_sand: BlockState = sand.try_into_api().unwrap();
            let api_anvil: BlockState = anvil.try_into_api().unwrap();
            let api_air: BlockState = air.try_into_api().unwrap();
            let invalid: BlockState =
                mod_api::__private::block_state_from_ordinal(api_sand.block(), u16::MAX);

            // Typed handle boundary rejects unknown states and wrong phases without unwinding.
            assert_eq!(
                level.spawn_falling_block(
                    game,
                    invalid,
                    [1.5, 70.0, 2.5],
                    [0.0; 3],
                    Falling::default(),
                ),
                Err(LevelWriteError::InvalidState(BlockStateError::UnknownState))
            );
            {
                let _setup =
                    mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::Setup);
                assert_eq!(
                    level.spawn_falling_block(
                        game,
                        api_sand,
                        [1.5, 70.0, 2.5],
                        [0.0; 3],
                        Falling::default(),
                    ),
                    Err(LevelWriteError::Unavailable)
                );
            }

            // Live velocity is kept unclamped (past the old 3.9 clamp), and flags are preserved.
            let mut flags = Falling::default();
            flags.drop_item = false;
            flags.hurts = true;
            let thrown = level
                .spawn_falling_block(game, api_sand, [1.5, 70.0, 2.5], [0.5, 9.0, -9.0], flags)
                .unwrap()
                .unwrap();
            assert_eq!(
                game.entity_kind(thrown)
                    .and_then(|kind| kind.id(game))
                    .as_deref(),
                Some("minecraft:falling_block")
            );
            assert_eq!(game.entity_position(thrown), Some([1.5, 70.0, 2.5]));
            assert_eq!(game.entity_velocity(thrown), Some([0.5, 9.0, -9.0]));
            assert_eq!(game.world.block(1, 70, 2), stone);
            let block = game.falling.iter().find(|f| f.id == thrown.id()).unwrap();
            assert_eq!(block.state, sand);
            assert_eq!(block.start_pos, BlockPos::new(1, 70, 2));
            assert_eq!(block.velocity, Vec3::new(0.5, 9.0, -9.0));
            assert!(!block.drop_item);
            assert!(block.damage.hurt_entities);
            assert_eq!(block.damage.per_distance, 2.0);
            assert_eq!(block.damage.max, 40);

            // Anvil keeps its vanilla hurt defaults even with `hurts: false`.
            let anvil_ref = level
                .spawn_falling_block(
                    game,
                    api_anvil,
                    [2.5, 70.0, 2.5],
                    [0.0; 3],
                    Falling::default(),
                )
                .unwrap()
                .unwrap();
            let anvil_row = game
                .falling
                .iter()
                .find(|f| f.id == anvil_ref.id())
                .unwrap();
            assert!(anvil_row.drop_item);
            assert!(anvil_row.damage.hurt_entities);
            assert_eq!(anvil_row.damage.per_distance, 2.0);
            assert_eq!(anvil_row.damage.max, 40);

            // Air spawns and is discarded on its first tick (FallingBlockEntity.tick).
            let air_ref = level
                .spawn_falling_block(
                    game,
                    api_air,
                    [3.5, 70.0, 2.5],
                    [0.0; 3],
                    Falling::default(),
                )
                .unwrap()
                .unwrap();
            let air_index = game
                .falling
                .iter()
                .position(|f| f.id == air_ref.id())
                .unwrap();
            assert_eq!(game.falling[air_index].state, air);
            game.tick_one_falling(air_index, &[]).unwrap();
            assert!(!game.falling.iter().any(|f| f.id == air_ref.id()));

            // No 1,024 per-level quota.
            for i in 0..1030 {
                let spawned = game
                    .level_spawn_falling_block(
                        level,
                        sand,
                        [0.5 + f64::from((i % 16) as u8), 70.0, 0.5],
                        [0.0; 3],
                        Falling::default(),
                    )
                    .unwrap();
                assert!(spawned.is_some(), "falling block {i} should spawn");
            }
            assert!(game.falling.len() >= 1030);

            // Stale handles and non-finite coordinates or velocity leave no new entity.
            let count = game.falling.len();
            let stale = LevelRef::new(WorldEpoch::new(0), game.dim.key());
            assert_eq!(
                stale.spawn_falling_block(
                    game,
                    api_sand,
                    [1.5, 70.0, 1.5],
                    [0.0; 3],
                    Falling::default(),
                ),
                Err(LevelWriteError::Stale)
            );
            for (pos, vel) in [
                ([f64::NAN, 70.0, 1.5], [0.0; 3]),
                ([1.5, f64::INFINITY, 1.5], [0.0; 3]),
                ([1.5, 70.0, 1.5], [0.0, f64::NAN, 0.0]),
                ([1.5, 70.0, 1.5], [f64::INFINITY, 0.0, 0.0]),
            ] {
                assert_eq!(
                    level.spawn_falling_block(game, api_sand, pos, vel, Falling::default()),
                    Ok(None),
                    "pos={pos:?} vel={vel:?}"
                );
            }
            assert_eq!(game.falling.len(), count);
        });

        // Client side refuses with Unavailable.
        super::super::native_entity_refs::tests::with_client_game(|game| {
            let epoch = game.native_lifecycle.epoch().unwrap();
            let level = LevelRef::new(epoch, game.dim.key());
            let sand = game
                .pack
                .assets
                .registry
                .default_state("minecraft:sand")
                .unwrap();
            assert_eq!(
                game.level_spawn_falling_block(
                    level,
                    sand,
                    [1.5, 70.0, 1.5],
                    [0.0; 3],
                    Falling::default(),
                ),
                Err(LevelWriteError::Unavailable)
            );
        });
    }

    #[test]
    #[ignore = "daily"]
    fn fresh_fall_does_not_require_summon_bounds_or_a_loaded_chunk() {
        use mod_api::Falling;
        use sunrise_client::join_seq::JoinSeq;

        with_falling_game(|game| {
            let level = game.level(game.dim.id()).unwrap();
            let host: &dyn mod_api::Host = game;
            let sand = host.block_state_id("minecraft:sand").unwrap();
            game.world
                .chunks
                .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));

            let above_summon_bounds = [0.5, 20_000_000.0, 0.5];
            let high = level
                .spawn_falling_block(
                    game,
                    sand,
                    above_summon_bounds,
                    [0.0; 3],
                    Falling::default(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(game.entity_position(high), Some(above_summon_bounds));

            let unloaded = [1000.5, 70.0, 0.5];
            assert!(!game.world.loaded(1000, 0));
            let hidden = level
                .spawn_falling_block(game, sand, unloaded, [0.0; 3], Falling::default())
                .unwrap()
                .unwrap();
            assert_eq!(game.entity_position(hidden), Some(unloaded));
            assert!(!game.world.loaded(1000, 0));
            let query = |game: &Game<'_>| {
                game.level_entities_in(level, [1000.0, 70.0, 0.0], [1001.0, 71.0, 1.0])
                    .unwrap()
            };
            let visit_all = |game: &Game<'_>| {
                let mut found = false;
                let _ = game
                    .level_visit_all_entities(level, &mut |entity| {
                        found |= entity == hidden;
                        std::ops::ControlFlow::Continue(())
                    })
                    .unwrap();
                found
            };
            let uuid = mod_api::EntityUuid(game.entity_uuid(hidden).unwrap());
            assert_eq!(game.entity(hidden.id()), None);
            assert_eq!(game.entity_by_uuid(uuid), None);
            assert!(query(game).is_empty());
            assert!(!visit_all(game));
            game.begin_ordered_ticks();
            game.run_ordered_ticks(JoinSeq::MAX, true).unwrap();
            let row = game.falling.iter().find(|f| f.id == hidden.id()).unwrap();
            assert_eq!(row.age, 0, "a hidden entity does not tick");

            game.world.chunks.insert(
                (62, 0),
                Arc::new(sunrise_client::chunk::Chunk::empty(62, 0)),
            );
            assert_eq!(game.entity(hidden.id()), Some(hidden));
            assert_eq!(game.entity_by_uuid(uuid), Some(hidden));
            assert_eq!(query(game), [hidden]);
            assert!(visit_all(game));
        });
    }

    #[test]
    #[ignore = "daily"]
    fn hidden_falling_spawn_saves_and_reopens_alongside_existing_entities() {
        use sunrise_client::save::{LevelShape, SaveDir};
        use sunrise_core::nbt::Tag;

        let root = tempfile::tempdir().unwrap();
        let key = (62, 0);
        let at = [1000.5_f64, 70.0, 0.5];
        let make_save = || {
            Arc::new(SaveDir::new(
                root.path().to_path_buf(),
                LevelShape::OVERWORLD,
                Some(Default::default()),
            ))
        };
        with_falling_game(|game| {
            game.save = make_save();
            let registry = game.pack.assets.registry;
            let gravel = registry.default_state("minecraft:gravel").unwrap();
            let sand = registry.default_state("minecraft:sand").unwrap();
            let existing = Tag::compound([
                ("id", Tag::String("minecraft:falling_block".into())),
                ("UUID", Tag::IntArray(vec![32, 33, 34, 35])),
                ("BlockState", game.state_to_nbt(gravel)),
                (
                    "Pos",
                    Tag::List(at.map(|value| Tag::Double(value.to_bits())).to_vec()),
                ),
                ("Time", Tag::Int(8)),
            ]);
            game.save
                .save_entities(vec![(key, vec![existing])])
                .unwrap();
            let level = game.level(game.dim.id()).unwrap();
            let fresh = game
                .level_spawn_falling_block(
                    level,
                    sand,
                    at,
                    [0.0, -1.5, 0.0],
                    mod_api::Falling::default(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(game.entity_position(fresh), Some(at));
            assert!(!game.world.chunks.contains_key(&key));
            game.save_entities(true).unwrap();
            assert!(
                !game.world.chunks.contains_key(&key),
                "saving entities did not load terrain"
            );
            assert_eq!(
                game.entity_position(fresh),
                None,
                "the hidden section unloaded after saving"
            );
        });

        with_falling_game(|game| {
            game.save = make_save();
            let tags = game.save.load_entities(key.0, key.1).unwrap();
            assert_eq!(tags.len(), 2);
            game.world.chunks.insert(
                key,
                Arc::new(sunrise_client::chunk::Chunk::empty(key.0, key.1)),
            );
            game.world.queue_entity_nbt(key, tags);
            game.spawn_loaded_entities();
            let registry = game.pack.assets.registry;
            let sand = registry.default_state("minecraft:sand").unwrap();
            let gravel = registry.default_state("minecraft:gravel").unwrap();
            assert_eq!(game.falling.len(), 2);
            let fresh = game.falling.iter().find(|row| row.state == sand).unwrap();
            assert_eq!(fresh.position.components(), at);
            assert_eq!(fresh.velocity, Vec3::new(0.0, -1.5, 0.0));
            let old = game.falling.iter().find(|row| row.state == gravel).unwrap();
            assert_eq!(old.age, 8);
            assert_eq!(old.saved.as_ref().unwrap().0, [32, 33, 34, 35]);
        });
    }

    #[test]
    #[ignore = "daily"]
    fn discarded_falling_block_does_not_rejoin_before_the_removal_sweep() {
        use mod_api::{
            EntityJoinLevelEvent, EntityLeaveLevelEvent, Falling, Flow, Hooks, ServerHost,
        };

        struct Listener(Arc<Mutex<Vec<&'static str>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Listener {
            fn entity_join_level(
                &self,
                _: &mut dyn ServerHost,
                event: &mut EntityJoinLevelEvent<'_>,
            ) -> Flow {
                match event.kind {
                    "minecraft:falling_block" => self.0.lock().unwrap().push("falling join"),
                    "minecraft:item" => self.0.lock().unwrap().push("item join"),
                    _ => {}
                }
                Flow::Continue
            }
            fn entity_leave_level(
                &self,
                _: &mut dyn ServerHost,
                event: &mut EntityLeaveLevelEvent<'_>,
            ) {
                if event.kind == "minecraft:falling_block" {
                    self.0.lock().unwrap().push("falling leave");
                }
            }
        }

        let events = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "observe-fall-discard",
            Box::new(Listener(events.clone())),
        )]
        .into();
        with_falling_game(|game| {
            mod_platform::testing::with_mods(mods, || {
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
                let level = game.level(game.dim.id()).unwrap();
                let sand = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:sand")
                    .unwrap();
                let entity = game
                    .level_spawn_falling_block(
                        level,
                        sand,
                        [1.5, 70.0, 1.5],
                        [0.0; 3],
                        Falling::default(),
                    )
                    .unwrap()
                    .unwrap();
                assert!(entity.discard(game));
                assert_eq!(
                    game.falling[0].id,
                    entity.id(),
                    "the physical row still waits for the sweep"
                );
                game.spawn_at_location(
                    Vec3::new(2.5, 70.0, 1.5),
                    0.0,
                    sunrise_client::items::ItemStack::new("minecraft:dirt", 1),
                );
                assert_eq!(
                    *events.lock().unwrap(),
                    ["falling join", "falling leave", "item join"]
                );
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn parked_falling_spawn_uses_the_destination_journal() {
        use mod_api::{EntityJoinLevelEvent, Falling, Flow, Hooks, ServerHost};

        struct Listener(Arc<Mutex<Vec<String>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Listener {
            fn entity_join_level(
                &self,
                _: &mut dyn ServerHost,
                event: &mut EntityJoinLevelEvent<'_>,
            ) -> Flow {
                if event.kind == "minecraft:falling_block" {
                    self.0.lock().unwrap().push(event.dimension.to_owned());
                }
                Flow::Continue
            }
        }

        let heard = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "parked-falling-join",
            Box::new(Listener(heard.clone())),
        )]
        .into();
        with_falling_game(|game| {
            mod_platform::testing::with_mods(mods, || {
                let home = game.dim;
                let level = game.level(home.id()).unwrap();
                let host: &dyn mod_api::Host = game;
                let sand = host.block_state_id("minecraft:sand").unwrap();
                let home_seal = game.transaction_open();
                game.enter_level(Dim::Nether).unwrap();
                let at = [1.5, 70.0, 1.5];
                let pending = level
                    .spawn_falling_block(game, sand, at, [0.0; 3], Falling::default())
                    .unwrap()
                    .unwrap();
                assert_eq!(game.dim, Dim::Nether);
                assert!(game.falling.is_empty());
                assert!(heard.lock().unwrap().is_empty());
                game.enter_level(home).unwrap();
                assert_eq!(game.entity_position(pending), Some(at));
                game.transaction_close(&home_seal, false);
                assert!(game.falling.is_empty());

                game.enter_level(Dim::Nether).unwrap();
                let caller_seal = game.transaction_open();
                let joined = level
                    .spawn_falling_block(game, sand, at, [0.0; 3], Falling::default())
                    .unwrap()
                    .unwrap();
                assert_eq!(game.dim, Dim::Nether);
                assert!(game.falling.is_empty());
                assert_eq!(*heard.lock().unwrap(), [home.id()]);
                game.transaction_close(&caller_seal, false);
                game.enter_level(home).unwrap();
                assert_eq!(game.entity_position(joined), Some(at));
                assert_eq!(game.falling.len(), 1);
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn natural_falling_join_veto_runs_before_the_next_scheduled_tick() {
        use mod_api::{BlockUpdateFlags, EntityJoinLevelEvent, Flow, Hooks, ServerHost};
        use sunrise_core::ticks::TickPriority;

        const FIRST: BlockPos = BlockPos::new(1, 70, 1);
        const SECOND: BlockPos = BlockPos::new(3, 70, 1);
        struct Listener(Arc<Mutex<Vec<[f64; 3]>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Listener {
            fn entity_join_level(
                &self,
                host: &mut dyn ServerHost,
                event: &mut EntityJoinLevelEvent<'_>,
            ) -> Flow {
                if event.kind != "minecraft:falling_block" {
                    return Flow::Continue;
                }
                self.0.lock().unwrap().push(event.pos);
                let level = host.level(event.dimension).unwrap();
                let stone = host.block_state_id("minecraft:stone").unwrap();
                assert_eq!(
                    level.set_block(
                        host,
                        [SECOND.x, SECOND.y - 1, SECOND.z],
                        stone,
                        BlockUpdateFlags::NONE,
                    ),
                    Ok(true)
                );
                Flow::Cancel
            }
        }

        let joins = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "natural-falling-veto",
            Box::new(Listener(joins.clone())),
        )]
        .into();
        with_falling_game(|game| {
            mod_platform::testing::with_mods(mods, || {
                for x in -1..=1 {
                    for z in -1..=1 {
                        let mut chunk = sunrise_client::chunk::Chunk::empty(x, z);
                        chunk.lit = true;
                        game.world.chunks.insert((x, z), Arc::new(chunk));
                    }
                }
                game.tickets
                    .update(Some(sunrise_client::ticket_levels::Sources {
                        forced: vec![(0, 0)],
                        ..Default::default()
                    }));
                let registry = game.pack.assets.registry;
                let sand = registry.default_state("minecraft:sand").unwrap();
                let air = registry.default_state("minecraft:air").unwrap();
                let stone = registry.default_state("minecraft:stone").unwrap();
                for pos in [FIRST, SECOND] {
                    game.world.set(pos.x, pos.y, pos.z, sand).unwrap();
                }
                game.with_level(|b, level, ticks| {
                    ticks
                        .add_chunk(FIRST.chunk(), Default::default(), Default::default())
                        .unwrap();
                    for (pos, priority) in
                        [(FIRST, TickPriority::High), (SECOND, TickPriority::Normal)]
                    {
                        ticks.schedule_block(level.game_time(), pos, b.block(sand), 0, priority);
                    }
                });

                game.tick_scheduled().unwrap();
                assert_eq!(game.world.block(FIRST.x, FIRST.y, FIRST.z), air);
                assert_eq!(game.world.block(SECOND.x, SECOND.y, SECOND.z), sand);
                assert_eq!(game.world.block(SECOND.x, SECOND.y - 1, SECOND.z), stone);
                assert!(game.falling.is_empty());
                game.native_joins_flush(false, 0);
                assert_eq!(*joins.lock().unwrap(), [[1.5, 70.0, 1.5]]);
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn toss_listener_gets_the_falling_join_veto_before_its_spawn_returns() {
        use mod_api::{EntityJoinLevelEvent, Falling, Flow, Hooks, ItemTossEvent, ServerHost};

        struct Listener(Arc<Mutex<Vec<&'static str>>>);
        #[mod_api::__subscriptions]
        impl Hooks for Listener {
            fn item_toss(&self, host: &mut dyn ServerHost, _: &mut ItemTossEvent) -> Flow {
                let level = host.level("minecraft:overworld").unwrap();
                let sand = host.block_state_id("minecraft:sand").unwrap();
                assert_eq!(
                    level.spawn_falling_block(
                        host,
                        sand,
                        [1.5, 70.0, 1.5],
                        [0.0; 3],
                        Falling::default()
                    ),
                    Ok(None)
                );
                self.0.lock().unwrap().push("spawn returned");
                Flow::Continue
            }
            fn entity_join_level(
                &self,
                _: &mut dyn ServerHost,
                event: &mut EntityJoinLevelEvent<'_>,
            ) -> Flow {
                match event.kind {
                    "minecraft:falling_block" => {
                        self.0.lock().unwrap().push("falling join");
                        Flow::Cancel
                    }
                    "minecraft:item" => {
                        self.0.lock().unwrap().push("item join");
                        Flow::Continue
                    }
                    _ => Flow::Continue,
                }
            }
        }

        let order = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "toss-falling-veto",
            Box::new(Listener(order.clone())),
        )]
        .into();
        with_falling_game(|game| {
            mod_platform::testing::with_mods(mods, || {
                let item = sunrise_client::items::ItemStack::new("minecraft:dirt", 1);
                let tossed = game.toss_item(item, [0.5, 70.0, 0.5], [0.0; 3], None);
                assert!(tossed.is_some());
                assert!(game.falling.is_empty());
                game.native_joins_flush(false, 0);
                assert_eq!(
                    *order.lock().unwrap(),
                    ["falling join", "spawn returned", "item join"]
                );
            });
        });
    }

    #[test]
    #[ignore = "daily"]
    fn level_spawn_falling_block_joins_cancel_and_roll_back_in_transactions() {
        use mod_api::{
            EntityConstructingEvent, EntityJoinLevelEvent, Falling, Flow, Holder, HostSide,
            ServerHost,
        };
        use std::sync::atomic::{AtomicBool, Ordering};

        struct Listener {
            heard: Arc<Mutex<Vec<String>>>,
            cancel: Arc<AtomicBool>,
        }
        #[mod_api::__subscriptions]
        impl mod_api::Hooks for Listener {
            fn entity_constructing(&self, _: HostSide<'_>, event: &mut EntityConstructingEvent) {
                if let Holder::Entity(_) = event.holder {
                    self.heard
                        .lock()
                        .unwrap()
                        .push(format!("construct {}", event.kind));
                }
            }
            fn entity_join_level(
                &self,
                _: &mut dyn ServerHost,
                event: &mut EntityJoinLevelEvent<'_>,
            ) -> Flow {
                self.heard
                    .lock()
                    .unwrap()
                    .push(format!("join {} {}", event.kind, event.loaded_from_disk));
                if self.cancel.load(Ordering::Relaxed) {
                    Flow::Cancel
                } else {
                    Flow::Continue
                }
            }
        }

        let heard = Arc::new(Mutex::new(Vec::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "falling-listener",
            Box::new(Listener {
                heard: heard.clone(),
                cancel: cancel.clone(),
            }),
        )]
        .into();

        with_falling_game(|game| {
            mod_platform::testing::with_mods(mods, || {
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
                let level = game.level(game.dim.id()).unwrap();
                let sand = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:sand")
                    .unwrap();

                // Normal spawn runs construct + join and keeps the falling block.
                let kept = game
                    .level_spawn_falling_block(
                        level,
                        sand,
                        [1.5, 70.0, 1.5],
                        [0.0; 3],
                        Falling::default(),
                    )
                    .unwrap()
                    .unwrap();
                assert!(game.falling.iter().any(|f| f.id == kept.id()));
                game.native_joins_flush(false, 0);
                assert_eq!(
                    *heard.lock().unwrap(),
                    [
                        "construct minecraft:falling_block",
                        "join minecraft:falling_block false",
                    ]
                );
                heard.lock().unwrap().clear();

                // Cancelled join removes the falling block and returns Ok(None).
                cancel.store(true, Ordering::Relaxed);
                let count = game.falling.len();
                assert_eq!(
                    game.level_spawn_falling_block(
                        level,
                        sand,
                        [2.5, 70.0, 1.5],
                        [0.0; 3],
                        Falling::default(),
                    ),
                    Ok(None)
                );
                assert_eq!(game.falling.len(), count);
                assert_eq!(
                    *heard.lock().unwrap(),
                    [
                        "construct minecraft:falling_block",
                        "join minecraft:falling_block false",
                    ]
                );
                heard.lock().unwrap().clear();
                cancel.store(false, Ordering::Relaxed);

                // Aborted transaction removes the falling block and never announces join.
                let seal = game.transaction_open();
                let aborted = game
                    .level_spawn_falling_block(
                        level,
                        sand,
                        [3.5, 70.0, 1.5],
                        [0.0; 3],
                        Falling::default(),
                    )
                    .unwrap()
                    .unwrap();
                assert!(game.falling.iter().any(|f| f.id == aborted.id()));
                assert_eq!(
                    *heard.lock().unwrap(),
                    ["construct minecraft:falling_block"]
                );
                game.transaction_close(&seal, false);
                assert!(!game.falling.iter().any(|f| f.id == aborted.id()));
                assert_eq!(
                    *heard.lock().unwrap(),
                    ["construct minecraft:falling_block"]
                );
                heard.lock().unwrap().clear();

                // Committed transaction announces join at commit and keeps the falling block.
                let seal = game.transaction_open();
                let committed = game
                    .level_spawn_falling_block(
                        level,
                        sand,
                        [4.5, 70.0, 1.5],
                        [0.0; 3],
                        Falling::default(),
                    )
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    *heard.lock().unwrap(),
                    ["construct minecraft:falling_block"]
                );
                game.transaction_close(&seal, true);
                assert!(game.falling.iter().any(|f| f.id == committed.id()));
                assert_eq!(
                    *heard.lock().unwrap(),
                    [
                        "construct minecraft:falling_block",
                        "join minecraft:falling_block false",
                    ]
                );
            });
        });
    }
}
