use super::*;
use mod_api::__private::game::{CallbackOwner, DeferredLandingAction};
use sunrise_client::landing::{LandingPause, PlayerLandingEntity};

impl Game<'_> {
    /// The level owns `fallOn` effects before damage and after the borrowed move yielded.
    pub(in crate::game) fn apply_landing(
        &mut self,
        id: u32,
        player: bool,
        paused: &mut LandingPause,
    ) {
        if let Some(trample) = paused.fall.trample {
            self.trample_farmland(id, player, trample);
        }
        if !sunrise_client::block_contact::fall_listens(paused.state) {
            return;
        }
        let mut actions = Vec::new();
        let mut level = DamageLevel::of(
            &mut self.world,
            self.pack.assets.registry,
            Some(world_items!(self)),
            Some(&self.pack.block_registries),
            &self.damage_hooks,
        );
        if player {
            if let Some(p) = self.player.as_mut().filter(|p| p.entity_id as u32 == id) {
                let immune = p.may_fly;
                paused.dispatch(
                    &mut level,
                    &mut PlayerLandingEntity(p),
                    immune,
                    &mut actions,
                );
            }
        } else if let Some(mob) = self.mobs.iter_mut().find(|m| m.id == id && !m.removed) {
            mob.dispatch_landing_part(paused, &mut level, &mut actions);
        }
        self.apply_fall_actions(actions);
    }

    pub(in crate::game) fn apply_fall_actions(&mut self, actions: Vec<DeferredLandingAction>) {
        for action in actions {
            if action.at_site() {
                self.run_landing_action(action);
            } else {
                sunrise_client::block_contact::defer_landing([action]);
            }
        }
    }

    pub(super) fn run_landing_action(&mut self, action: DeferredLandingAction) {
        if let Some(owner) = action.owner() {
            let _ = CallbackOwner::call_landing_callback(owner, self, |host| action.run(host));
        } else {
            action.run(self);
        }
    }
}

impl HitLevel<'_> {
    /// `Block.fallOn`: lends a scoped `LandingEntity` on `state` at `pos` after falling
    /// `distance` blocks. Writable distance and multiplier feed `causeFallDamage` before
    /// sounds and hurt. A zero multiplier prevents both. A creative or fall-immune entity
    /// keeps its immunity. Players, mobs and dropped items land here (items with zero
    /// multiplier). `FallOnEvent::at_landing` actions run after the loan is released
    /// and before damage; `after_landing` actions run after the entity batch.
    pub(in crate::game) fn native_fall_on(&mut self, call: super::fire::FallOnCall<'_>) {
        self.fire_fall_on(call);
    }
    /// `Block.bounceOn`: lends a scoped `LandingEntity` bouncing off `state` at `pos`
    /// after falling `fall_distance` blocks. `movement` is the movement the bounce leaves
    /// it, before vanilla sets it: change it to change what the entity keeps. Deferred
    /// server-host work queued with `BounceOnEvent::after_bounce` runs once the entity
    /// batch finishes. Players and mobs bounce here; dropped items and the other entities
    /// do not yet.
    pub(in crate::game) fn native_bounce_on(&mut self, call: super::fire::BounceOnCall<'_>) {
        self.fire_bounce_on(call);
    }
}

#[cfg(test)]
mod landing_tests {
    use super::*;
    use crate::game::api_values::TryIntoApi;
    use mod_api::{
        BlockPart, BounceOnEvent, FallOnEvent, Hooks, LandingEntity, LandingEntityAccess,
        LevelRandomHost, ModEntry, Registrar,
    };
    use std::sync::{Arc, Mutex};

    struct TestEntity {
        id: i32,
        kind: &'static str,
        position: [f64; 3],
        velocity: [f64; 3],
        fall_distance: f64,
        on_ground: bool,
        crouching: bool,
        suppressing_bounce: bool,
        dimensions: [f32; 2],
    }

    impl LandingEntityAccess for TestEntity {
        fn id(&self) -> i32 {
            self.id
        }
        fn kind(&self) -> &str {
            self.kind
        }
        fn position(&self) -> [f64; 3] {
            self.position
        }
        fn velocity(&self) -> [f64; 3] {
            self.velocity
        }
        fn fall_distance(&self) -> f64 {
            self.fall_distance
        }
        fn on_ground(&self) -> bool {
            self.on_ground
        }
        fn is_crouching(&self) -> bool {
            self.crouching
        }
        fn is_suppressing_bounce(&self) -> bool {
            self.suppressing_bounce
        }
        fn dimensions(&self) -> [f32; 2] {
            self.dimensions
        }
        fn set_velocity(&mut self, velocity: [f64; 3]) -> bool {
            self.velocity = velocity;
            true
        }
        fn reset_fall_distance(&mut self) {
            self.fall_distance = 0.0;
        }
    }

    #[derive(Clone, Default)]
    struct LandingPart {
        target: Arc<Mutex<Option<(mod_api::BlockState, mod_api::BlockState)>>>,
        observed: Arc<Mutex<Vec<String>>>,
    }

    #[mod_api::sunrise_part]
    impl BlockPart for LandingPart {
        fn fall_on(&self, _: &mut dyn LevelRandomHost, event: &mut FallOnEvent<'_>) {
            let (first, second) = self.target.lock().unwrap().unwrap();
            assert_eq!(event.entity.id(), 7);
            assert_eq!(event.entity.kind(), "minecraft:player");
            assert_eq!(event.entity.block_position(), [1, 65, 1]);
            assert!(event.entity.is_crouching());
            assert!(event.entity.is_suppressing_bounce());
            assert_eq!(event.entity.dimensions(), [0.6, 1.5]);
            assert!(!event.entity.set_velocity([f64::NAN, 0.0, 0.0]));
            assert!(event.entity.set_velocity([0.25, 0.5, -0.25]));
            event.entity.reset_fall_distance();
            event.distance = 12.0;
            event.damage_multiplier = 0.5;
            let pos = [event.pos.x, event.pos.y, event.pos.z];
            let dim = event.dimension.to_owned();
            let observed = self.observed.clone();
            event.after_landing(move |host| {
                let level = host.level(&dim).unwrap();
                let flags = BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS;
                let _ = level.set_block(host, pos, first, flags);
                observed.lock().unwrap().push("first".into());
            });
            let dim2 = event.dimension.to_owned();
            let observed2 = self.observed.clone();
            event.after_landing(move |host| {
                let level = host.level(&dim2).unwrap();
                // No unchanged-block replay gate: this callback sees `first` set by the
                // preceding callback and performs its own local check.
                if level.block_state(host, pos) == Some(first) {
                    let flags = BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS;
                    let _ = level.set_block(host, pos, second, flags);
                    observed2.lock().unwrap().push("second".into());
                }
            });
        }

        fn bounce_on(&self, _: &mut dyn LevelRandomHost, event: &mut BounceOnEvent<'_>) {
            assert_eq!(event.entity.id(), 7);
            assert!(!event.entity.is_suppressing_bounce());
            assert_eq!(event.entity.velocity(), [0.0, -1.0, 0.0]);
            event.movement[1] = 0.75;
            let observed = self.observed.clone();
            event.after_bounce(move |_host| {
                observed.lock().unwrap().push("bounced".into());
            });
        }
    }

    struct LandingMod(LandingPart);
    #[mod_api::__subscriptions]
    impl Hooks for LandingMod {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let farmland = r.block_key("minecraft:farmland");
            r.block_part(farmland, self.0.clone());
            let slime = r.block_key("minecraft:slime_block");
            r.block_part(slime, self.0.clone());
        }
    }

    #[derive(Clone)]
    struct PanickingPart;
    #[mod_api::sunrise_part]
    impl BlockPart for PanickingPart {
        fn fall_on(&self, _: &mut dyn LevelRandomHost, event: &mut FallOnEvent<'_>) {
            event.after_landing(|_host| panic!("landing callback boom"));
        }
    }

    struct PanickingMod;
    #[mod_api::__subscriptions]
    impl Hooks for PanickingMod {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let farmland = r.block_key("minecraft:farmland");
            r.block_part(farmland, PanickingPart);
        }
    }

    #[derive(Clone)]
    struct KickOnLanding;

    #[mod_api::sunrise_part]
    impl BlockPart for KickOnLanding {
        fn fall_on(&self, _: &mut dyn LevelRandomHost, event: &mut FallOnEvent<'_>) {
            assert_eq!(event.entity.kind(), "minecraft:player");
            assert!(event.entity.set_velocity([0.5, 0.0, 0.0]));
        }
    }

    #[mod_api::__subscriptions]
    impl Hooks for KickOnLanding {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let stone = r.block_key("minecraft:stone");
            r.block_part(stone, self.clone());
        }
    }

    #[test]
    #[ignore = "daily"]
    fn harmless_player_landing_publishes_motion_to_the_controlling_client() {
        use crate::game::native_item_places::tests::open_to_lan;
        use crate::game::native_sound_refs::tests::{guest, packets};
        use sunrise_client::{chunk::Chunk, lan::guest::InboxItem, motion::PlayerMotion};
        use sunrise_core::geometry::Vec3;
        use sunrise_protocol::packets::{Empty, play, play::Serverbound as S};

        let mods: mod_api::Mods =
            vec![ModEntry::new("landing_kick", Box::new(KickOnLanding))].into();
        mod_platform::testing::with_mods(mods.clone(), || {
            crate::game::api_values::install_parts(&mods).unwrap();
            let _ = sunrise_client::block_contact::HOOKS.set(super::super::BLOCK_CONTACT_HOOKS);
            crate::game::native_entity_refs::tests::with_game(|game| {
                open_to_lan(game);
                game.streaming
                    .insert_remote(&mut game.world, Chunk::empty(0, 0));
                let stone = game
                    .pack
                    .assets
                    .registry
                    .default_state("minecraft:stone")
                    .unwrap();
                game.world.set(1, 64, 1, stone).unwrap();
                let at = [1.5, 65.25, 1.5];
                let (mut player, rx) = guest(20, 20, at);
                let mut body =
                    PlayerMotion::new(game.motion, Vec3::new(at[0], at[1], at[2])).unwrap();
                body.fall_distance = 1.0;
                player.motion = Some(Box::new(body));
                player.loaded = true;
                player.loading = false;
                player.load_timeout = 0;
                player.first_good = at;
                let health = player.health;
                player.inbox.push_back(InboxItem::Packet(
                    S::MovePlayerPos(play::MovePlayerPos {
                        x: 1.5,
                        y: 65.0,
                        z: 1.5,
                        flags: 1,
                    }),
                    None,
                ));
                player
                    .inbox
                    .push_back(InboxItem::Packet(S::ClientTickEnd(Empty), None));
                game.lan.as_mut().unwrap().hub.guests.push(player);
                game.entity_section_joined(20, at);
                game.tick_lan_packets();
                assert_eq!(
                    game.lan
                        .as_ref()
                        .unwrap()
                        .hub
                        .guest(20)
                        .unwrap()
                        .moves
                        .len(),
                    1
                );
                let _ = packets(&rx);
                game.guest_server_tick(20, true);
                let owner = game.lan.as_ref().unwrap().hub.guest(20).unwrap();
                assert_eq!(owner.health, health);
                assert_eq!(owner.position, [1.5, 65.0, 1.5]);
                assert!(packets(&rx).iter().any(|packet| {
                    matches!(packet, play::Clientbound::SetEntityMotion(p) if p.id == 20 && p.velocity[0] > 0.0)
                }));
            });
        });
    }

    /// `fall_on` and `bounce_on` lend the scoped falling entity directly and queue owned
    /// native callbacks that run in order at batch end without an unchanged-block gate;
    /// a panicking mod's deferred callback is disabled for the session and reported via
    /// the callback panic notice queue without blocking other mods.
    #[test]
    #[ignore = "daily"]
    fn fall_on_and_bounce_on_lend_entity_and_contain_deferred_panics() {
        let part = LandingPart::default();
        let mods: mod_api::Mods = vec![
            ModEntry::new("bad_landing", Box::new(PanickingMod)),
            ModEntry::new("good_landing", Box::new(LandingMod(part.clone()))),
        ]
        .into();
        crate::game::api_values::install_parts(&mods).unwrap();
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                let state = |name| game.pack.assets.registry.default_state(name).unwrap();
                let (farmland, stone, dirt) = (
                    state("minecraft:farmland"),
                    state("minecraft:stone"),
                    state("minecraft:dirt"),
                );
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                game.world.set(1, 64, 1, farmland).unwrap();
                *part.target.lock().unwrap() =
                    Some((stone.try_into_api().unwrap(), dirt.try_into_api().unwrap()));

                let mut entity = TestEntity {
                    id: 7,
                    kind: "minecraft:player",
                    position: [1.5, 65.0, 1.5],
                    velocity: [0.0, -0.8, 0.0],
                    fall_distance: 5.0,
                    on_ground: true,
                    crouching: true,
                    suppressing_bounce: true,
                    dimensions: [0.6, 1.5],
                };
                let mut distance = 5.0;
                let mut multiplier = 1.0;
                let mut deferred = Vec::new();
                let mut level = sunrise_client::health::TestLevel::new(0);
                HitLevel(level.level()).native_fall_on(super::super::fire::FallOnCall {
                    dimension: "minecraft:overworld",
                    entity: LandingEntity::new(&mut entity),
                    state: farmland,
                    pos: BlockPos::new(1, 64, 1),
                    distance: &mut distance,
                    damage_multiplier: &mut multiplier,
                    deferred: mod_api::__private::LandingDeferred::new(&mut deferred),
                });
                assert_eq!((distance, multiplier), (12.0, 0.5));
                assert_eq!(entity.velocity, [0.25, 0.5, -0.25]);
                assert_eq!(entity.fall_distance, 0.0);
                assert_eq!(game.world.block(1, 64, 1), farmland, "deferred until drain");

                let _ = mod_api::__private::game::take_callback_panic_notices();
                sunrise_client::block_contact::defer_landing(deferred);
                game.drain_entity_inside();

                let notices = mod_api::__private::game::take_callback_panic_notices();
                assert!(
                    notices
                        .iter()
                        .any(|n| n.mod_id == "bad_landing" && n.kind == "block landing"),
                    "panicking deferred landing callback must queue an error notice: {notices:?}"
                );
                assert_eq!(
                    game.world.block(1, 64, 1),
                    dirt,
                    "good mod's ordered deferred callbacks ran even after bad_landing panicked"
                );
                assert_eq!(
                    *part.observed.lock().unwrap(),
                    vec!["first".to_owned(), "second".to_owned()]
                );

                // A second landing from bad_landing stays disabled (no second panic notice),
                // while good_landing still runs.
                game.world.set(1, 64, 1, farmland).unwrap();
                let mut entity_second = TestEntity {
                    id: 7,
                    kind: "minecraft:player",
                    position: [1.5, 65.0, 1.5],
                    velocity: [0.0, -0.8, 0.0],
                    fall_distance: 5.0,
                    on_ground: true,
                    crouching: true,
                    suppressing_bounce: true,
                    dimensions: [0.6, 1.5],
                };
                let mut distance2 = 5.0;
                let mut multiplier2 = 1.0;
                let mut deferred2 = Vec::new();
                HitLevel(level.level()).native_fall_on(super::super::fire::FallOnCall {
                    dimension: "minecraft:overworld",
                    entity: LandingEntity::new(&mut entity_second),
                    state: farmland,
                    pos: BlockPos::new(1, 64, 1),
                    distance: &mut distance2,
                    damage_multiplier: &mut multiplier2,
                    deferred: mod_api::__private::LandingDeferred::new(&mut deferred2),
                });
                sunrise_client::block_contact::defer_landing(deferred2);
                game.drain_entity_inside();
                assert!(
                    mod_api::__private::game::take_callback_panic_notices().is_empty(),
                    "disabled bad_landing callback must not fire or report a second panic"
                );
                assert_eq!(game.world.block(1, 64, 1), dirt);

                use sunrise_client::{
                    block_contact, health::LentLevel, mobs::Mob, parallel::TickThread,
                };
                use sunrise_core::geometry::Vec3;
                let slime = state("minecraft:slime_block");
                game.world.set(1, 64, 1, slime).unwrap();
                let kind = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                let mut mob = Mob::new(7, kind, Vec3::new(1.5, 65.0, 1.5), 0.0);
                mob.supporting = Some([1, 64, 1]);
                mob.velocity = Vec3::new(0.0, -1.0, 0.0);
                let _ = block_contact::HOOKS.set(super::super::BLOCK_CONTACT_HOOKS);
                let mut lent = LentLevel::take(&mut game.world);
                block_contact::in_level(TickThread::mint(), Some(game.dim.id()), || {
                    let world = game.mob_world_base(&[]);
                    let mut level = DamageLevel::lent(
                        &game.world,
                        game.pack.assets.registry,
                        &mut lent,
                        Some(game.items()),
                        Some(&game.pack.block_registries),
                        &game.damage_hooks,
                    );
                    mob.restitute_movement(
                        &world,
                        &mut level,
                        Vec3::new(0.0, -1.0, 0.0),
                        Vec3::ZERO,
                    );
                });
                lent.give_back(&mut game.world);
                assert_eq!(mob.velocity, Vec3::new(0.0, 0.75, 0.0));
                game.drain_entity_inside();
                assert_eq!(
                    *part.observed.lock().unwrap(),
                    vec![
                        "first".to_owned(),
                        "second".to_owned(),
                        "first".to_owned(),
                        "second".to_owned(),
                        "bounced".to_owned(),
                    ]
                );
            });
        });
    }
}
