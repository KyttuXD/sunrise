use super::*;
use mod_api::{
    self, BlockPart, Flow, Hooks, HostSide, ItemAfterVerticalCollisionEvent, Registrar,
    RegistrationContext,
};
use std::sync::Mutex;
use sunrise_client::{chunk::Chunk, join_seq::Joined as _};

type Contact = (bool, u32, [i32; 3], [f64; 3], u32);

#[derive(Clone, Default)]
struct Probe {
    contacts: Arc<Mutex<Vec<Contact>>>,
}

#[mod_api::sunrise_part]
impl BlockPart for Probe {
    fn item_after_vertical_collision(
        &self,
        _: HostSide,
        event: &mut ItemAfterVerticalCollisionEvent<'_>,
    ) -> Flow {
        let stack = event.stack.as_ref().expect("active item stack");
        self.contacts.lock().unwrap().push((
            event.client,
            event.entity,
            event.block_pos,
            event.item_pos,
            stack.count(),
        ));
        if stack.count() == 1 {
            return Flow::Cancel;
        }
        *event.stack = event.stack.take().unwrap().shrink(2);
        Flow::Continue
    }
}

#[mod_api::__subscriptions]
impl Hooks for Probe {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        let key = r.block_key("minecraft:stone");
        r.block_part(key, self.clone());
    }
}

#[test]
#[ignore = "daily"]
fn the_active_stack_survives_a_partial_contact_and_consumption_removes_the_entity() {
    crate::game::in_own_process(
        "game::item_contact_tests::the_active_stack_survives_a_partial_contact_and_consumption_removes_the_entity",
        the_active_stack_survives_a_partial_contact_and_consumption_removes_the_entity_isolated,
    );
}

fn the_active_stack_survives_a_partial_contact_and_consumption_removes_the_entity_isolated() {
    use crate::game::native_entity_refs::tests::with_game;
    let prepare = |game: &mut Game<'_>| {
        game.options.dedicated = Some(Vec::new());
        game.native_lifecycle
            .begin_open(Some(mod_api::Authority::IntegratedServer));
        game.world = World::new(game.res.facts().air, 1);
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        game.world.set(4, 63, 4, stone).unwrap();
    };

    // The centre of the item is outside the block, while its box overlaps it.
    let landing = [3.99, 64.0, 4.5];
    let mut input = ItemStack::new("minecraft:iron_ingot", 13);
    input
        .data_mut()
        .set_other("fixture:opaque", Some(sunrise_core::nbt::Tag::Long(99)));
    let spawn = |game: &mut Game<'_>, count| {
        game.dropped.clear();
        let mut stack = input.clone();
        stack.count = count;
        game.spawn_item(stack, [landing[0], 64.1, landing[2]], [0.0, -0.3, 0.0], 20);
        game.ticked_entities_through = game.next_entity;
        game.dropped[0].clone()
    };
    let mut vanilla = None;
    with_game(|game| {
        prepare(game);
        let before = spawn(game, 13);
        game.tick_items();
        let item = game.dropped[0].clone();
        assert_eq!(item.id, before.id);
        assert_eq!(item.position, Vec3::new(landing[0], landing[1], landing[2]));
        assert_eq!(item.velocity, Vec3::ZERO);
        assert_eq!(item.stack, input);
        assert_eq!(item.age, 1);

        vanilla = Some(item);
    });
    let vanilla = vanilla.unwrap();

    let probe = Probe::default();
    let mods: mod_api::Mods =
        vec![mod_api::ModEntry::new("contact", Box::new(probe.clone()))].into();
    mod_platform::testing::with_mods(mods.clone(), || {
        crate::game::api_values::install_parts(&mods).unwrap();
        with_game(|game| {
            prepare(game);
            let old = spawn(game, 13);
            game.tick_items();
            let item = &game.dropped[0];
            assert_eq!(item.id, old.id);
            assert_eq!(item.join_seq(), old.join_seq());
            assert_eq!(item.position, vanilla.position);
            assert_eq!(item.velocity, vanilla.velocity);
            let mut expected = input.clone();
            expected.count = 11;
            assert_eq!(item.stack, expected);
            assert_eq!(
                probe.contacts.lock().unwrap().as_slice(),
                &[(false, old.id, [4, 63, 4], landing, 13)]
            );

            for count in [1, 2] {
                spawn(game, count);
                game.tick_items();
                assert!(game.dropped.is_empty(), "consumed stack of {count}");
            }
        });
    });
}

type BounceContact = (bool, i32, String, [i32; 3]);

#[derive(Clone, Default)]
struct BounceProbe {
    calls: Arc<Mutex<Vec<BounceContact>>>,
    bounced: Arc<Mutex<Vec<(i32, [i32; 3])>>>,
}

#[mod_api::sunrise_part]
impl BlockPart for BounceProbe {
    fn bounce_restitution(
        &self,
        level: mod_api::LevelSide<'_>,
        event: &mut mod_api::BounceRestitutionEvent<'_>,
    ) {
        let (client, host) = match level {
            mod_api::LevelSide::Client(host) => (true, host),
            mod_api::LevelSide::Server(host) => (false, host),
        };
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        assert_eq!(host.block_state(pos), Some(event.state));
        self.calls
            .lock()
            .unwrap()
            .push((client, event.entity, event.kind.into(), pos));
        event.factor = 0.67_f32;
    }

    fn bounce_on(
        &self,
        _: &mut dyn mod_api::LevelRandomHost,
        event: &mut mod_api::BounceOnEvent<'_>,
    ) {
        self.bounced
            .lock()
            .unwrap()
            .push((event.entity, [event.pos.x, event.pos.y, event.pos.z]));
    }
}

#[mod_api::__subscriptions]
impl Hooks for BounceProbe {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        for block in [
            "minecraft:glass",
            "minecraft:white_carpet",
            "minecraft:honey_block",
        ] {
            let key = r.block_key(block);
            r.block_part(key, self.clone());
        }
    }
}

#[test]
#[ignore = "daily"]
fn item_restitution_uses_the_legacy_contact_on_both_simulating_sides() {
    crate::game::in_own_process(
        "game::item_contact_tests::item_restitution_uses_the_legacy_contact_on_both_simulating_sides",
        || {
            use crate::game::native_entity_refs::tests::{with_client_game, with_game};
            let part = BounceProbe::default();
            let mods: mod_api::Mods =
                vec![mod_api::ModEntry::new("bounce", Box::new(part.clone()))].into();
            mod_platform::testing::with_mods(mods.clone(), || {
                crate::game::api_values::install_parts(&mods).unwrap();
                crate::game::native_hooks::install_block_contact_hooks();
                let run = |game: &mut Game<'_>, client: bool| {
                    let registry = game.pack.assets.registry;
                    let block = |name| registry.default_state(name).unwrap();
                    let glass = block("minecraft:glass");
                    let carpet = block("minecraft:white_carpet");
                    let stone = block("minecraft:stone");
                    let slime = block("minecraft:slime_block");
                    let honey = block("minecraft:honey_block");
                    let cases = [
                        // The item's centre lies outside the block; its moved footprint does not.
                        (
                            glass,
                            None,
                            None,
                            3.99,
                            64.25,
                            -0.96,
                            64.0,
                            0.5278801619529518_f64,
                            true,
                        ),
                        // The carpet supports the box, but getOnPosLegacy addresses the glass below it.
                        (
                            glass,
                            Some(carpet),
                            None,
                            4.5,
                            64.3125,
                            -0.96,
                            64.0625,
                            0.5278801619529518_f64,
                            true,
                        ),
                        (
                            stone,
                            Some(carpet),
                            None,
                            4.5,
                            64.3125,
                            -0.96,
                            64.0625,
                            0.0_f64,
                            false,
                        ),
                        (
                            slime,
                            None,
                            None,
                            4.5,
                            64.25,
                            -0.96,
                            64.0,
                            0.7878808308504466_f64,
                            false,
                        ),
                        (
                            honey, None, None, 4.5, 64.25, -0.96, 63.9375, 0.0_f64, false,
                        ),
                        (glass, None, None, 4.5, 64.03125, 0.0, 64.0, 0.0_f64, false),
                        // A ceiling collision addresses the carpet cell but never queries its factor.
                        (
                            stone,
                            Some(carpet),
                            Some(glass),
                            4.5,
                            64.5,
                            1.0,
                            64.75,
                            -0.0_f64,
                            false,
                        ),
                    ];
                    for (floor, above, ceiling, x, y, vy, landed_y, expected_y, calls) in cases {
                        game.world = World::new(game.res.facts().air, 1);
                        game.world
                            .chunks
                            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                        game.world.set(4, 63, 4, floor).unwrap();
                        if let Some(above) = above {
                            game.world.set(4, 64, 4, above).unwrap();
                        }
                        if let Some(ceiling) = ceiling {
                            game.world.set(4, 65, 4, ceiling).unwrap();
                        }
                        game.dropped.clear();
                        part.calls.lock().unwrap().clear();
                        part.bounced.lock().unwrap().clear();
                        game.persist.sync_body_position.clear();
                        game.spawn_item(
                            ItemStack::new("minecraft:iron_ingot", 1),
                            [x, y, 4.5],
                            [0.0, vy, 0.0],
                            20,
                        );
                        let mut item = game.dropped[0].clone();
                        let dim = game.dim.id();
                        let result =
                            sunrise_client::block_contact::in_level(game.tick, Some(dim), || {
                                game.tick_item_with_block_contacts(
                                    &mut item,
                                    None,
                                    &[],
                                    client,
                                    &mut |_| {},
                                )
                            });
                        assert_eq!(result, (true, false));
                        assert_eq!(item.position.y, landed_y);
                        assert_eq!(
                            item.velocity.y.to_bits(),
                            expected_y.to_bits(),
                            "floor {floor:?}, start {y}"
                        );
                        assert_eq!(
                            game.persist.sync_body_position.contains(&item.id),
                            !client && expected_y > 0.0
                        );
                        let bounced = part.bounced.lock().unwrap();
                        if !client && calls {
                            assert_eq!(*bounced, [(item.id as i32, [4, 63, 4])]);
                        } else {
                            assert!(
                                bounced.is_empty(),
                                "unexpected bounce callback: {bounced:?}"
                            );
                        }
                        let got = part.calls.lock().unwrap();
                        if calls {
                            assert_eq!(
                                *got,
                                [(
                                    client,
                                    item.id as i32,
                                    "minecraft:item".to_owned(),
                                    [4, 63, 4]
                                )]
                            );
                        } else {
                            assert!(got.is_empty(), "unexpected query: {got:?}");
                        }
                    }
                };
                with_game(|game| run(game, false));
                with_client_game(|game| run(game, true));
            });
        },
    );
}

#[test]
#[ignore = "daily"]
fn tnt_cart_fall_dispatches_restitution_only_if_the_cart_survives() {
    use crate::game::minecarts::{CartKind, Minecart};
    crate::game::in_own_process(
        "game::item_contact_tests::tnt_cart_fall_dispatches_restitution_only_if_the_cart_survives",
        || {
            let part = BounceProbe::default();
            let mods: mod_api::Mods =
                vec![mod_api::ModEntry::new("bounce", Box::new(part.clone()))].into();
            mod_platform::testing::with_mods(mods.clone(), || {
                crate::game::api_values::install_parts(&mods).unwrap();
                crate::game::native_hooks::install_block_contact_hooks();
                crate::game::native_entity_refs::tests::with_game(|game| {
                    let glass = game
                        .pack
                        .assets
                        .registry
                        .default_state("minecraft:glass")
                        .unwrap();
                    use crate::game::piston_push::Lid;
                    use mod_api::MoveCause;
                    use sunrise_core::geometry::Aabb;

                    for mover in [
                        None,
                        Some(MoveCause::SelfMovement),
                        Some(MoveCause::Piston),
                        Some(MoveCause::ShulkerBox),
                    ] {
                        for (id, fall, survives) in [(71, 0.0, true), (72, 3.5, false)] {
                            game.world = World::new(game.res.facts().air, 1);
                            game.world
                                .chunks
                                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                            game.world.set(4, 63, 4, glass).unwrap();
                            game.carts.clear();
                            let mut cart =
                                Minecart::new(id, CartKind::Tnt, Vec3::new(4.5, 64.25, 4.5));
                            cart.fall_distance = fall;
                            cart.velocity = Vec3::new(0.0, -1.0, 0.0);
                            game.carts.push(cart);
                            part.calls.lock().unwrap().clear();
                            let mut tick_cart = None;
                            sunrise_client::block_contact::in_level(
                                game.tick,
                                Some(game.dim.id()),
                                || {
                                    match mover {
                                        None => {
                                            // The tick moves a clone of its joined resident row.
                                            let mut cart = game.carts[0].clone();
                                            assert_eq!(
                                                game.move_cart(
                                                    &mut cart,
                                                    Vec3::new(0.0, -0.5, 0.0)
                                                ),
                                                survives
                                            );
                                            tick_cart = Some(cart);
                                        }
                                        Some(MoveCause::ShulkerBox) => game.lid_push(
                                            Lid::Box,
                                            Aabb::new(
                                                Vec3::new(4.0, 64.5, 4.0),
                                                Vec3::new(5.0, 65.0, 5.0),
                                            ),
                                            Vec3::new(0.0, -0.5, 0.0),
                                        ),
                                        Some(cause) => {
                                            let entity = game.entity(id).unwrap();
                                            assert!(
                                                entity
                                                    .move_by(game, [0.0, -0.5, 0.0], cause)
                                                    .is_some()
                                            );
                                        }
                                    }
                                },
                            );
                            assert_eq!(
                                game.carts.iter().any(|cart| cart.id == id),
                                survives,
                                "{mover:?}"
                            );
                            let calls = part.calls.lock().unwrap();
                            if survives {
                                let cart = tick_cart.as_ref().unwrap_or(&game.carts[0]);
                                assert_eq!(cart.position.y.to_bits(), 64.0_f64.to_bits());
                                assert_eq!(
                                    *calls,
                                    [(
                                        false,
                                        id as i32,
                                        "minecraft:tnt_minecart".to_owned(),
                                        [4, 63, 4]
                                    )]
                                );
                                assert!(cart.velocity.y > 0.0);
                            } else {
                                assert!(calls.is_empty(), "{mover:?}");
                            }
                        }
                    }
                });
            });
        },
    );
}

#[test]
#[ignore = "daily"]
fn item_support_survives_idle_then_clears_on_an_airborne_move() {
    crate::game::in_own_process(
        "game::item_contact_tests::item_support_survives_idle_then_clears_on_an_airborne_move",
        || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                let air = game.res.facts().air;
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                let support = BlockPos::new(4, 63, 4);
                game.world = World::new(air, 1);
                game.world
                    .chunks
                    .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                game.world
                    .set(support.x, support.y, support.z, stone)
                    .unwrap();
                game.spawn_item(
                    ItemStack::new("minecraft:iron_ingot", 1),
                    [4.5, 64.25, 4.5],
                    [1.0, -0.96, 0.0],
                    20,
                );
                let mut item = game.dropped[0].clone();
                let tick = |game: &mut Game<'_>, item: &mut ItemEntity| {
                    game.tick_item_with_block_contacts(item, None, &[], false, &mut |_| {})
                };
                assert_eq!(tick(game, &mut item), (true, false));
                assert_eq!(item.position, Vec3::new(5.5, 64.0, 4.5));
                // Only the old horizontal footprint had a supporting block.
                assert!(item.ground.on_ground);
                assert_eq!(item.ground.supporting_block, Some(support));

                game.world
                    .set(support.x, support.y, support.z, air)
                    .unwrap();
                item.velocity = Vec3::ZERO;
                // The next tick must not be one of the four-tick movement updates.
                item.ticks = item.id.wrapping_neg() & 3;
                assert_eq!(tick(game, &mut item), (true, false));
                assert_eq!(item.position, Vec3::new(5.5, 64.0, 4.5));
                assert_eq!(item.ground.supporting_block, Some(support));

                item.velocity = Vec3::new(0.1, 0.2, 0.0);
                assert_eq!(tick(game, &mut item), (true, false));
                assert!(item.position.y > 64.0);
                assert!(!item.ground.on_ground);
                assert_eq!(item.ground.supporting_block, None);
            });
        },
    );
}

type StepCall = (bool, i32, [i32; 3], bool);

#[derive(Clone, Default)]
struct StepProbe {
    calls: Arc<Mutex<Vec<StepCall>>>,
}

#[mod_api::sunrise_part]
impl BlockPart for StepProbe {
    fn step_on(&self, level: mod_api::LevelRandomSide<'_>, event: &mut mod_api::StepOnEvent<'_>) {
        let (client, host): (bool, &dyn mod_api::LevelHost) = match level {
            mod_api::LevelRandomSide::Client(host) => (true, host),
            mod_api::LevelRandomSide::Server(host) => (false, host),
        };
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        assert_eq!(host.block_state(pos), Some(event.state));
        let first = {
            let mut calls = self.calls.lock().unwrap();
            let first = !calls.iter().any(|call| call.1 == event.entity);
            calls.push((client, event.entity, pos, event.stepping_carefully));
            first
        };
        event.movement[0] = 0.25;
        if !client {
            let write = match event.kind {
                "minecraft:armor_stand" => mod_api::EntityWrite::Push([0.25, 0.0, 0.0]),
                "minecraft:item" => mod_api::EntityWrite::StuckIn([0.5, 0.25, 0.5]),
                "minecraft:oak_boat" if first => mod_api::EntityWrite::SetFireTicks(40),
                _ => return,
            };
            assert!(event.write_entity(write));
        }
    }

    fn entity_inside(
        &self,
        _: &mut dyn mod_api::LevelRandomHost,
        event: &mut mod_api::EntityInsideEvent<'_>,
    ) {
        // A later inside write wins over the step; settle then drags this movement.
        event.run_after(
            mod_api::InsideEffect::Freeze,
            mod_api::EntityWrite::SetMovement([0.5, 0.0, 0.0]),
        );
    }

    fn item_after_vertical_collision(
        &self,
        _: HostSide<'_>,
        event: &mut ItemAfterVerticalCollisionEvent<'_>,
    ) -> Flow {
        if event.stack.as_ref().is_some_and(|stack| stack.count() == 1) {
            Flow::Cancel
        } else {
            Flow::Continue
        }
    }
}

#[mod_api::__subscriptions]
impl Hooks for StepProbe {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        let stone = r.block_key("minecraft:stone");
        let cobweb = r.block_key("minecraft:cobweb");
        r.block_part(stone, self.clone());
        r.block_part(cobweb, self.clone());
    }
}

#[test]
#[ignore = "daily"]
fn step_on_edits_grounded_bodies_before_inside_effects_and_settle() {
    crate::game::in_own_process(
        "game::item_contact_tests::step_on_edits_grounded_bodies_before_inside_effects_and_settle",
        || {
            use crate::game::native_entity_refs::tests::{with_client_game, with_game};
            use sunrise_client::motion::{MotionWorld, PlayerMotion};

            let part = StepProbe::default();
            let mods: mod_api::Mods =
                vec![mod_api::ModEntry::new("step", Box::new(part.clone()))].into();
            mod_platform::testing::with_mods(mods.clone(), || {
                crate::game::api_values::install_parts(&mods).unwrap();
                crate::game::native_hooks::install_block_contact_hooks();
                let run = |game: &mut Game<'_>, client: bool| {
                    let registry = game.pack.assets.registry;
                    let stone = registry.default_state("minecraft:stone").unwrap();
                    let web = registry.default_state("minecraft:cobweb").unwrap();
                    let air = game.res.facts().air;
                    let floor = [4, 63, 4];
                    game.world = World::new(air, 1);
                    game.world
                        .chunks
                        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                    game.world.set(4, 63, 4, stone).unwrap();
                    if !client {
                        game.world.set(4, 64, 4, web).unwrap();
                    }
                    let spawn = |game: &mut Game<'_>, count, height| {
                        game.dropped.clear();
                        game.spawn_item(
                            ItemStack::new("minecraft:iron_ingot", count),
                            [4.5, height, 4.5],
                            [0.0, -0.96, 0.0],
                            20,
                        );
                        game.dropped[0].clone()
                    };
                    let tick = |game: &mut Game<'_>, item: &mut ItemEntity, escape| {
                        sunrise_client::block_contact::in_level(
                            sunrise_client::parallel::TickThread::mint(),
                            Some("minecraft:overworld"),
                            || {
                                game.tick_item_with_block_contacts(
                                    item,
                                    escape,
                                    &[],
                                    client,
                                    &mut |_| {},
                                )
                            },
                        )
                    };
                    let clear_calls = || std::mem::take(&mut *part.calls.lock().unwrap());
                    let mut item = spawn(game, 2, 64.25);
                    assert_eq!(tick(game, &mut item, None), (true, false));
                    assert_eq!(item.position.y, 64.0);
                    let settled = if client { 0.25 } else { 0.5 } * f64::from(0.98_f32 * 0.6_f32);
                    assert_eq!(item.velocity.x, settled);
                    assert_eq!(clear_calls(), [(client, item.id as i32, floor, false)]);

                    game.world.set(4, 64, 4, air).unwrap();
                    item.velocity = Vec3::ZERO;
                    item.ticks = item.id.wrapping_neg() & 3;
                    assert_eq!(tick(game, &mut item, None), (true, false));
                    assert_eq!(item.position, Vec3::new(4.5, 64.0, 4.5));
                    assert_eq!(item.velocity.x, 0.25); // no movement drag on an idle tick
                    assert_eq!(clear_calls(), [(client, item.id as i32, floor, false)]);
                    assert_eq!(tick(game, &mut item, None), (true, false));
                    let displacement = if client { 0.25 } else { 0.125 };
                    assert_eq!(item.position.x, 4.5 + displacement);
                    assert_eq!(clear_calls(), [(client, item.id as i32, floor, false)]);
                    if !client {
                        // The actual next move spent the preceding multiplier and
                        // the stone Part installed a fresh one on its own step.
                        assert_eq!(item.stuck, Vec3::new(0.5, 0.25, 0.5));
                    }

                    assert_eq!(tick(game, &mut item, Some(0.1)), (true, false));
                    let mut airborne = spawn(game, 2, 69.0);
                    assert_eq!(tick(game, &mut airborne, None), (true, false));
                    let mut consumed = spawn(game, 1, 64.25);
                    assert_eq!(tick(game, &mut consumed, None), (false, true));
                    assert!(clear_calls().is_empty());

                    let position = Vec3::new(4.5, 64.0, 4.5);
                    let mut player = PlayerMotion::new(game.motion, position).unwrap();
                    player.entity_id = 77;
                    player.ground.on_ground = true;
                    player.shift_key_down = true;
                    let side = game_side!(game);
                    let mut motion = MotionWorld {
                        side: &side,
                        entities: &[],
                        player: &mut player,
                        scene: &mut game.world,
                        assets: &game.pack.assets,
                        collision_binding: game.collision,
                        resources: game.motion,
                        health: &game.health_resources,
                        loop_context: &game.loop_context,
                        damage_hooks: &game.damage_hooks,
                        tag_items: None,
                        block_registries: Some(&game.pack.block_registries),
                    };
                    if client {
                        motion.apply_effects_from_blocks().unwrap();
                    } else {
                        motion.server_stuck_in_blocks(position).unwrap();
                        assert!(clear_calls().is_empty());
                        motion.server_block_damage(position).unwrap();
                    }
                    assert_eq!(player.velocity.x, 0.25);
                    assert_eq!(clear_calls(), [(client, 77, floor, true)]);

                    if !client {
                        game.tickets.update(Some(crate::ticket_levels::Sources {
                            forced: vec![(0, 0)],
                            ..Default::default()
                        }));
                        let mut stand = sunrise_client::entities::ArmorStand::new(
                            78,
                            Vec3::new(4.5, 64.25, 4.5),
                            0.0,
                        );
                        stand.velocity.y = -0.5;
                        game.armor_stands.push(stand);
                        game.tick_armor_stands().unwrap();
                        assert_eq!(game.armor_stands[0].position.y, 64.0);
                        assert_eq!(game.armor_stands[0].velocity.x, 0.5);
                        assert_eq!(clear_calls(), [(false, 78, floor, false)]);

                        use crate::game::native_entity_refs::tests::{
                            bind_acting_guest, bind_local_player,
                        };
                        let seat = Vec3::new(8.5, 64.0, 4.5);
                        game.world.set(8, 63, 4, stone).unwrap();
                        game.player = Some(PlayerMotion::new(game.motion, seat).unwrap());
                        bind_local_player(game, 80, 1);
                        game.lan_acting = Some(2);
                        game.lan_acting_name = Some("boat_step".into());
                        bind_acting_guest(game);
                        game.lan_acting = None;
                        game.lan.as_mut().unwrap().hub.guests.update(0, |guest| {
                            guest.vehicle = Some(79);
                        });
                        let mut boat = crate::game::boats::Boat::new(79, "oak".into(), seat, 0.0);
                        boat.ground.on_ground = true;
                        game.boats.push(boat);
                        game.tick_boats();
                        assert_eq!(game.boats[0].fire_ticks, 40);
                        assert_eq!(clear_calls(), [(false, 79, [8, 63, 4], false); 2]);
                    }
                };
                with_game(|game| run(game, false));
                with_client_game(|game| run(game, true));
            });
        },
    );
}

#[derive(Clone, Default)]
struct StepWrites(Arc<Mutex<Vec<[[f64; 3]; 2]>>>);

#[mod_api::sunrise_part]
impl BlockPart for StepWrites {
    fn step_on(&self, level: mod_api::LevelRandomSide<'_>, event: &mut mod_api::StepOnEvent<'_>) {
        self.0.lock().unwrap().push(event.bounds);
        let hurt = mod_api::EntityWrite::Hurt { amount: 0.5 };
        match level {
            mod_api::LevelRandomSide::Server(level) => {
                let state = level.block_state_id("minecraft:glass").unwrap();
                assert!(event.write_entity(hurt));
                assert!(event.request(mod_api::LandingWrite::SetBlock(state)));
            }
            mod_api::LevelRandomSide::Client(_) => {
                assert!(!event.write_entity(hurt));
                assert!(!event.request(mod_api::LandingWrite::DestroyBlock { drop: false }));
            }
        }
    }
}

#[mod_api::__subscriptions]
impl Hooks for StepWrites {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        let stone = r.block_key("minecraft:stone");
        r.block_part(stone, self.clone());
    }
}

#[test]
#[ignore = "daily"]
fn step_on_hurts_the_stepping_body_and_guards_its_block_writes() {
    crate::game::in_own_process(
        "game::item_contact_tests::step_on_hurts_the_stepping_body_and_guards_its_block_writes",
        || {
            use crate::game::native_entity_refs::tests::{with_client_game, with_game};
            let part = StepWrites::default();
            let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
                "step_writes",
                Box::new(part.clone()),
            )]
            .into();
            mod_platform::testing::with_mods(mods.clone(), || {
                crate::game::api_values::install_parts(&mods).unwrap();
                crate::game::native_hooks::install_block_contact_hooks();
                let run = |game: &mut Game<'_>, client, change_first| {
                    let registry = game.pack.assets.registry;
                    let stone = registry.default_state("minecraft:stone").unwrap();
                    let glass = registry.default_state("minecraft:glass").unwrap();
                    let dirt = registry.default_state("minecraft:dirt").unwrap();
                    game.world
                        .chunks
                        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                    game.world.set(4, 63, 4, stone).unwrap();
                    game.dropped.clear();
                    game.spawn_item(
                        ItemStack::new("minecraft:iron_ingot", 2),
                        [4.5, 64.25, 4.5],
                        [0.0, -0.96, 0.0],
                        20,
                    );
                    let mut item = game.dropped[0].clone();
                    let before = item.health;
                    sunrise_client::block_contact::in_level(game.tick, Some(game.dim.id()), || {
                        assert_eq!(
                            game.tick_item_with_block_contacts(
                                &mut item,
                                None,
                                &[],
                                client,
                                &mut |_| {}
                            ),
                            (true, false)
                        );
                    });
                    assert_eq!(item.health, before - i32::from(!client));
                    assert_eq!(
                        game.world.block(4, 63, 4),
                        stone,
                        "block write stays queued"
                    );
                    if change_first {
                        game.world.set(4, 63, 4, dirt).unwrap();
                    }
                    game.drain_entity_inside();
                    let kept = if change_first {
                        dirt
                    } else if client {
                        stone
                    } else {
                        glass
                    };
                    assert_eq!(game.world.block(4, 63, 4), kept);
                    if !client && !change_first {
                        game.world.set(4, 63, 4, stone).unwrap();
                        let mut stack = ItemStack::new("minecraft:shulker_box", 1);
                        let contents: Vec<_> = (0..27)
                            .map(|slot| {
                                (
                                    slot,
                                    ItemStack::new("minecraft:diamond", u32::from(slot) + 1),
                                )
                            })
                            .collect();
                        stack.data_mut().container = contents.clone();
                        game.dropped.clear();
                        game.spawn_item(stack, [4.5, 64.25, 4.5], [0.0, -0.96, 0.0], 20);
                        game.dropped[0].health = 1;
                        let mut item = game.dropped.remove(0);
                        sunrise_client::block_contact::in_level(
                            game.tick,
                            Some(game.dim.id()),
                            || {
                                assert_eq!(
                                    game.tick_item_with_block_contacts(
                                        &mut item,
                                        None,
                                        &[],
                                        false,
                                        &mut |_| {}
                                    ),
                                    (false, true),
                                );
                            },
                        );
                        assert_eq!(game.dropped.len(), contents.len());
                        for (drop, (_, stack)) in game.dropped.iter().zip(contents) {
                            assert_eq!(drop.stack, stack);
                            assert_eq!(drop.position, item.position);
                        }
                        game.drain_entity_inside();
                    }
                };
                with_game(|game| {
                    game.options.dedicated = Some(Vec::new());
                    run(game, false, false);
                    run(game, false, true);
                });
                with_client_game(|game| run(game, true, false));
                assert_eq!(
                    *part.0.lock().unwrap(),
                    [[[4.375, 64.0, 4.375], [4.625, 64.25, 4.625]]; 4]
                );
            });
        },
    );
}

const ACCEPT_BEFORE: u8 = 0;
const REJECT: u8 = 1;
const ACCEPT_AFTER: u8 = 2;

#[derive(Clone, Copy)]
struct StepAnswer<const ORDER: u8>;

#[mod_api::sunrise_part]
impl<const ORDER: u8> BlockPart for StepAnswer<ORDER> {
    fn step_on(&self, level: mod_api::LevelRandomSide<'_>, event: &mut mod_api::StepOnEvent<'_>) {
        use mod_api::{EntityWrite, LandingWrite};
        let mod_api::LevelRandomSide::Server(level) = level else {
            return;
        };
        match ORDER {
            ACCEPT_BEFORE => {
                event.movement[0] = 0.25;
                let push = if event.pos.x == 9 { f64::MAX } else { 0.5 };
                event.write_entity(EntityWrite::Push([push, 0.0, 0.0]));
                event.request(LandingWrite::SetBlock(
                    level.block_state_id("minecraft:glass").unwrap(),
                ));
            }
            REJECT => {
                assert_eq!(event.movement[0], 0.25);
                event.movement[0] = 3.0;
                event.write_entity(EntityWrite::Hurt { amount: 10.0 });
                event.request(LandingWrite::DestroyBlock { drop: false });
                match event.pos.x {
                    4 => {
                        let unknown = mod_api::__private::block_state_from_ordinal(
                            event.state.block(),
                            u16::MAX,
                        );
                        event.request(LandingWrite::SetBlock(unknown));
                    }
                    5 => {
                        event.write_entity(EntityWrite::SetMovement([f64::INFINITY, 0.0, 0.0]));
                    }
                    6 => {
                        event.write_entity(EntityWrite::Push([f64::NAN, 0.0, 0.0]));
                    }
                    8 => {
                        event.write_entity(EntityWrite::Push([f64::MAX, 0.0, 0.0]));
                        event.write_entity(EntityWrite::Push([f64::MAX, 0.0, 0.0]));
                    }
                    9 => {
                        event.write_entity(EntityWrite::Push([f64::MAX, 0.0, 0.0]));
                    }
                    _ => event.movement[0] = f64::NAN,
                }
            }
            ACCEPT_AFTER => {
                assert_eq!(event.movement[0], 0.25);
                event.movement[0] += 0.75;
                event.write_entity(EntityWrite::Push([0.5, 0.0, 0.0]));
            }
            _ => unreachable!(),
        }
    }
}

#[mod_api::__subscriptions]
impl<const ORDER: u8> Hooks for StepAnswer<ORDER> {
    fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
        let stone = r.block_key("minecraft:stone");
        r.block_part(stone, *self);
    }
}

#[test]
#[ignore = "daily"]
fn step_on_refuses_only_the_invalid_part_outputs() {
    crate::game::in_own_process(
        "game::item_contact_tests::step_on_refuses_only_the_invalid_part_outputs",
        || {
            let mods: mod_api::Mods = vec![
                mod_api::ModEntry::new("a_before", Box::new(StepAnswer::<ACCEPT_BEFORE>)),
                mod_api::ModEntry::new("b_invalid", Box::new(StepAnswer::<REJECT>)),
                mod_api::ModEntry::new("c_after", Box::new(StepAnswer::<ACCEPT_AFTER>)),
            ]
            .into();
            mod_platform::testing::with_mods(mods.clone(), || {
                crate::game::api_values::install_parts(&mods).unwrap();
                crate::game::native_hooks::install_block_contact_hooks();
                crate::game::native_entity_refs::tests::with_game(|game| {
                    game.options.dedicated = Some(Vec::new());
                    let registry = game.pack.assets.registry;
                    let stone = registry.default_state("minecraft:stone").unwrap();
                    let glass = registry.default_state("minecraft:glass").unwrap();
                    game.world
                        .chunks
                        .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
                    for x in 4..=9 {
                        game.world.set(x, 63, 4, stone).unwrap();
                        game.spawn_item(
                            ItemStack::new("minecraft:iron_ingot", 1),
                            [f64::from(x) + 0.5, 64.25, 4.5],
                            [0.0, -0.96, 0.0],
                            20,
                        );
                        let mut item = game.dropped.remove(0);
                        sunrise_client::block_contact::in_level(
                            game.tick,
                            Some(game.dim.id()),
                            || {
                                assert_eq!(
                                    game.tick_item_with_block_contacts(
                                        &mut item,
                                        None,
                                        &[],
                                        false,
                                        &mut |_| {}
                                    ),
                                    (true, false),
                                );
                            },
                        );
                        assert_eq!(item.health, 5);
                        let written = if x == 9 { f64::MAX } else { 2.0 };
                        assert_eq!(item.velocity.x, written * f64::from(0.98_f32 * 0.6_f32));
                        assert_eq!(game.world.block(x, 63, 4), stone);
                        game.drain_entity_inside();
                        assert_eq!(game.world.block(x, 63, 4), glass);
                    }
                });
            });
        },
    );
}
