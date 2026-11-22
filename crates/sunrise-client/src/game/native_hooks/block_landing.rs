use super::*;
use sunrise_core::geometry::Vec3;

impl GameLevel<'_> {
    /// `Block.fallOn`: lends a scoped `LandingEntity` and `&mut dyn LevelWriteHost` on
    /// `state` at `pos` after falling `distance` blocks. Writable distance and multiplier
    /// feed `causeFallDamage` before sounds and hurt. A zero multiplier prevents both. A
    /// creative or fall-immune entity keeps its immunity. Players, mobs and dropped items
    /// land here (items with zero multiplier). Deferred server-host work queued with
    /// `FallOnEvent::after_landing` runs once the entity batch finishes.
    pub(in crate::game) fn native_fall_on(
        &mut self,
        ticks: &mut Ticks,
        call: super::fire::FallOnCall<'_>,
    ) {
        self.fire_fall_on(ticks, call);
    }
    /// `Block.bounceOn`: lends a scoped `LandingEntity` and `&mut dyn LevelWriteHost`
    /// bouncing off `state` at `pos` after falling `fall_distance` blocks. `movement` is
    /// the movement the bounce leaves it, before `GameEvent.BOUNCE` and vanilla setting
    /// it: change it to change what the entity keeps. Deferred server-host work queued
    /// with `BounceOnEvent::after_bounce` runs once the entity batch finishes. Players
    /// and mobs bounce here; dropped items and the other entities do not yet.
    pub(in crate::game) fn native_bounce_on(
        &mut self,
        ticks: &mut Ticks,
        call: super::fire::BounceOnCall<'_>,
    ) {
        self.fire_bounce_on(ticks, call);
    }
}

/// Call-scoped entity view lent to `fall_on` / `bounce_on` while `Game::with_level`
/// holds `&mut Game`. Edits (`set_velocity`, `reset_fall_distance`) are applied back
/// to the live entity immediately when the level call returns (including on unwind).
struct LandingSnapshot {
    id: i32,
    kind: String,
    position: [f64; 3],
    velocity: [f64; 3],
    fall_distance: f64,
    on_ground: bool,
    crouching: bool,
    suppressing_bounce: bool,
    dimensions: [f32; 2],
    velocity_changed: bool,
    fall_reset: bool,
}

impl LandingSnapshot {
    fn from_player(player: &sunrise_client::motion::PlayerMotion) -> Self {
        Self {
            id: player.entity_id,
            kind: "minecraft:player".to_owned(),
            position: [player.position.x, player.position.y, player.position.z],
            velocity: [player.velocity.x, player.velocity.y, player.velocity.z],
            fall_distance: player.fall_distance,
            on_ground: player.ground.on_ground,
            crouching: player.pose == sunrise_client::pose::Pose::Crouching,
            suppressing_bounce: player.shift_key_down,
            dimensions: [player.dimensions.width, player.dimensions.height],
            velocity_changed: false,
            fall_reset: false,
        }
    }

    fn from_mob(mob: &sunrise_client::mobs::Mob) -> Self {
        Self {
            id: mob.id as i32,
            kind: mob.kind.id.clone(),
            position: [mob.position.x, mob.position.y, mob.position.z],
            velocity: [mob.velocity.x, mob.velocity.y, mob.velocity.z],
            fall_distance: mob.fall_distance,
            on_ground: mob.on_ground,
            crouching: mob.crouching || mob.avatar_pose == sunrise_client::pose::Pose::Crouching,
            suppressing_bounce: false,
            dimensions: [mob.width(), mob.height()],
            velocity_changed: false,
            fall_reset: false,
        }
    }

    fn apply_to_player(&self, player: &mut sunrise_client::motion::PlayerMotion) {
        if self.velocity_changed {
            player.velocity = Vec3::new(self.velocity[0], self.velocity[1], self.velocity[2]);
        }
        if self.fall_reset {
            player.fall_distance = 0.0;
        }
    }

    fn apply_to_mob(&self, mob: &mut sunrise_client::mobs::Mob) {
        if self.velocity_changed {
            mob.velocity = Vec3::new(self.velocity[0], self.velocity[1], self.velocity[2]);
        }
        if self.fall_reset {
            mob.fall_distance = 0.0;
        }
    }
}

impl mod_api::LandingEntityAccess for LandingSnapshot {
    fn id(&self) -> i32 {
        self.id
    }
    fn kind(&self) -> &str {
        &self.kind
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
        if !velocity.iter().all(|v| v.is_finite()) {
            return false;
        }
        self.velocity = velocity;
        self.velocity_changed = true;
        true
    }
    fn reset_fall_distance(&mut self) {
        self.fall_distance = 0.0;
        self.fall_reset = true;
    }
}

impl Game<'_> {
    fn authoritative_landing_dimension(&self) -> Option<&'static str> {
        if !self.native_authoritative() || sunrise_core::tick_phase::in_tick_phase() {
            return None;
        }
        Some(sunrise_client::block_contact::current_level().unwrap_or(self.dim.id()))
    }

    /// Run `BlockPart::fall_on` at the landing site with `&mut dyn LevelWriteHost`
    /// and flush its level events before `causeFallDamage` and `HIT_GROUND`.
    pub(in crate::game) fn run_fall_on(
        &mut self,
        entity: &mut dyn mod_api::LandingEntityAccess,
        state: BlockStateId,
        pos: BlockPos,
        distance: f64,
        multiplier: f32,
    ) -> (f64, f32) {
        let Some(dimension) = self.authoritative_landing_dimension() else {
            return (distance, multiplier);
        };
        if !crate::game::api_values::block_state::block_listens(mod_api::HookId::FallOn, state) {
            return (distance, multiplier);
        }
        let mut answer_distance = distance;
        let mut answer = multiplier;
        let mut deferred = Vec::new();
        self.with_level(|_, level, ticks| {
            level.native_fall_on(
                ticks,
                super::fire::FallOnCall {
                    dimension,
                    entity: mod_api::LandingEntity::new(entity),
                    state,
                    pos,
                    distance: &mut answer_distance,
                    damage_multiplier: &mut answer,
                    deferred: mod_api::__private::LandingDeferred::new(&mut deferred),
                },
            );
        });
        let _ = self.process_level_events();
        sunrise_client::block_contact::defer_landing(deferred);
        (
            if answer_distance.is_finite() {
                answer_distance
            } else {
                distance
            },
            if answer.is_finite() {
                answer.max(0.0)
            } else {
                multiplier
            },
        )
    }

    /// Run `BlockPart::bounce_on` at the bounce site with `&mut dyn LevelWriteHost`
    /// and flush its level events before `GameEvent.BOUNCE` and velocity update.
    pub(in crate::game) fn run_bounce_on(
        &mut self,
        entity: &mut dyn mod_api::LandingEntityAccess,
        state: BlockStateId,
        pos: BlockPos,
        fall_distance: f64,
        movement: Vec3,
    ) -> Vec3 {
        let Some(dimension) = self.authoritative_landing_dimension() else {
            return movement;
        };
        if !crate::game::api_values::block_state::block_listens(mod_api::HookId::BounceOn, state) {
            return movement;
        }
        let mut answer = [movement.x, movement.y, movement.z];
        let mut deferred = Vec::new();
        self.with_level(|_, level, ticks| {
            level.native_bounce_on(
                ticks,
                super::fire::BounceOnCall {
                    dimension,
                    entity: mod_api::LandingEntity::new(entity),
                    state,
                    pos,
                    fall_distance,
                    movement: &mut answer,
                    deferred: mod_api::__private::LandingDeferred::new(&mut deferred),
                },
            );
        });
        let _ = self.process_level_events();
        sunrise_client::block_contact::defer_landing(deferred);
        if answer.iter().all(|v| v.is_finite()) {
            Vec3::new(answer[0], answer[1], answer[2])
        } else {
            movement
        }
    }

    /// Run `BlockPart::fall_on` for the currently active `self.player` while the
    /// player's `MotionWorld` borrow is suspended.
    pub(in crate::game) fn mod_fall_on_player(
        &mut self,
        state: BlockStateId,
        pos: BlockPos,
        distance: f64,
        multiplier: f32,
    ) -> (f64, f32) {
        let Some(player) = self.player.as_ref() else {
            return (distance, multiplier);
        };
        let initial_multiplier = if player.may_fly { 0.0 } else { multiplier };
        let mut snapshot = LandingSnapshot::from_player(player);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_fall_on(&mut snapshot, state, pos, distance, initial_multiplier)
        }));
        if let Some(player) = self.player.as_mut() {
            snapshot.apply_to_player(player);
        }
        match outcome {
            Ok(answer) => answer,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Run `BlockPart::bounce_on` for the currently active `self.player`.
    pub(in crate::game) fn mod_bounce_on_player(
        &mut self,
        state: BlockStateId,
        pos: BlockPos,
        fall_distance: f64,
        movement: Vec3,
    ) -> Vec3 {
        let Some(player) = self.player.as_ref() else {
            return movement;
        };
        let mut snapshot = LandingSnapshot::from_player(player);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_bounce_on(&mut snapshot, state, pos, fall_distance, movement)
        }));
        if let Some(player) = self.player.as_mut() {
            snapshot.apply_to_player(player);
        }
        match outcome {
            Ok(answer) => answer,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Run `BlockPart::fall_on` for mob `(id, join)` while `self.mobs` is restored.
    pub(in crate::game) fn mod_fall_on_mob(
        &mut self,
        id: u32,
        join: sunrise_client::join_seq::JoinSeq,
        state: BlockStateId,
        pos: BlockPos,
        distance: f64,
        multiplier: f32,
    ) -> (f64, f32) {
        let Some(mob) = self
            .mobs
            .iter()
            .find(|m| m.id == id && m.join == join && !m.removed)
        else {
            return (distance, multiplier);
        };
        let initial_multiplier = if mob.kind.flags.fall_immune {
            0.0
        } else {
            multiplier
        };
        let mut snapshot = LandingSnapshot::from_mob(mob);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_fall_on(&mut snapshot, state, pos, distance, initial_multiplier)
        }));
        if let Some(index) = self
            .mobs
            .iter()
            .position(|m| m.id == id && m.join == join && !m.removed)
        {
            self.mobs.update(index, |mob| snapshot.apply_to_mob(mob));
        }
        match outcome {
            Ok(answer) => answer,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Run `BlockPart::bounce_on` for mob `(id, join)` while `self.mobs` is restored.
    pub(in crate::game) fn mod_bounce_on_mob(
        &mut self,
        id: u32,
        join: sunrise_client::join_seq::JoinSeq,
        state: BlockStateId,
        pos: BlockPos,
        fall_distance: f64,
        movement: Vec3,
    ) -> Vec3 {
        let Some(mob) = self
            .mobs
            .iter()
            .find(|m| m.id == id && m.join == join && !m.removed)
        else {
            return movement;
        };
        let mut snapshot = LandingSnapshot::from_mob(mob);
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.run_bounce_on(&mut snapshot, state, pos, fall_distance, movement)
        }));
        if let Some(index) = self
            .mobs
            .iter()
            .position(|m| m.id == id && m.join == join && !m.removed)
        {
            self.mobs.update(index, |mob| snapshot.apply_to_mob(mob));
        }
        match outcome {
            Ok(answer) => answer,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

#[cfg(test)]
mod landing_tests {
    use super::*;
    use crate::game::api_values::TryIntoApi;
    use mod_api::{
        BlockPart, BounceOnEvent, FallOnEvent, Hooks, LandingEntity, LandingEntityAccess,
        LevelWriteHost, ModEntry, Registrar,
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
        fn fall_on(&self, _: &mut dyn LevelWriteHost, event: &mut FallOnEvent<'_>) {
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

        fn bounce_on(&self, _: &mut dyn LevelWriteHost, event: &mut BounceOnEvent<'_>) {
            assert_eq!(event.entity.id(), 7);
            assert!(!event.entity.is_suppressing_bounce());
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
        }
    }

    #[derive(Clone)]
    struct PanickingPart;
    #[mod_api::sunrise_part]
    impl BlockPart for PanickingPart {
        fn fall_on(&self, _: &mut dyn LevelWriteHost, event: &mut FallOnEvent<'_>) {
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
                game.with_level(|_, level, ticks| {
                    level.native_fall_on(
                        ticks,
                        super::super::fire::FallOnCall {
                            dimension: "minecraft:overworld",
                            entity: LandingEntity::new(&mut entity),
                            state: farmland,
                            pos: BlockPos::new(1, 64, 1),
                            distance: &mut distance,
                            damage_multiplier: &mut multiplier,
                            deferred: mod_api::__private::LandingDeferred::new(&mut deferred),
                        },
                    );
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
                game.with_level(|_, level, ticks| {
                    level.native_fall_on(
                        ticks,
                        super::super::fire::FallOnCall {
                            dimension: "minecraft:overworld",
                            entity: LandingEntity::new(&mut entity_second),
                            state: farmland,
                            pos: BlockPos::new(1, 64, 1),
                            distance: &mut distance2,
                            damage_multiplier: &mut multiplier2,
                            deferred: mod_api::__private::LandingDeferred::new(&mut deferred2),
                        },
                    );
                });
                sunrise_client::block_contact::defer_landing(deferred2);
                game.drain_entity_inside();
                assert!(
                    mod_api::__private::game::take_callback_panic_notices().is_empty(),
                    "disabled bad_landing callback must not fire or report a second panic"
                );
                assert_eq!(game.world.block(1, 64, 1), dirt);

                let mut entity_bounce = TestEntity {
                    suppressing_bounce: false,
                    ..entity_second
                };
                let mut movement = [0.0, 0.4, 0.0];
                let mut bounce_deferred = Vec::new();
                game.with_level(|_, level, ticks| {
                    level.native_bounce_on(
                        ticks,
                        super::super::fire::BounceOnCall {
                            dimension: "minecraft:overworld",
                            entity: LandingEntity::new(&mut entity_bounce),
                            state: farmland,
                            pos: BlockPos::new(1, 64, 1),
                            fall_distance: 4.0,
                            movement: &mut movement,
                            deferred: mod_api::__private::LandingDeferred::new(
                                &mut bounce_deferred,
                            ),
                        },
                    );
                });
                assert_eq!(movement, [0.0, 0.75, 0.0]);
                sunrise_client::block_contact::defer_landing(bounce_deferred);
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

    #[derive(Clone, Default)]
    struct ImmediateWritePart {
        fall_replacement: Arc<Mutex<Option<mod_api::BlockState>>>,
        bounce_replacement: Arc<Mutex<Option<mod_api::BlockState>>>,
        seen_fall_states: Arc<Mutex<Vec<mod_api::BlockState>>>,
    }

    #[mod_api::sunrise_part]
    impl BlockPart for ImmediateWritePart {
        fn fall_on(&self, host: &mut dyn LevelWriteHost, event: &mut FallOnEvent<'_>) {
            self.seen_fall_states.lock().unwrap().push(event.state);
            event.damage_multiplier = 0.5;
            if let Some(next) = *self.fall_replacement.lock().unwrap() {
                let pos = [event.pos.x, event.pos.y, event.pos.z];
                let flags = BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS;
                assert!(host.set_block(pos, next, flags));
            }
        }

        fn bounce_on(&self, host: &mut dyn LevelWriteHost, event: &mut BounceOnEvent<'_>) {
            event.movement[1] = 0.9;
            if let Some(next) = *self.bounce_replacement.lock().unwrap() {
                let pos = [event.pos.x, event.pos.y, event.pos.z];
                let flags = BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS;
                assert!(host.set_block(pos, next, flags));
            }
        }
    }

    struct ImmediateWriteMod(ImmediateWritePart);
    #[mod_api::__subscriptions]
    impl Hooks for ImmediateWriteMod {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let stone = r.block_key("minecraft:stone");
            let slime = r.block_key("minecraft:slime_block");
            r.block_part(stone, self.0.clone());
            r.block_part(slime, self.0.clone());
        }
    }

    /// `fall_on` and `bounce_on` lend `&mut dyn LevelWriteHost` directly at the site so
    /// block writes run before `causeFallDamage`, `HIT_GROUND`, and `GameEvent.BOUNCE`.
    #[test]
    #[ignore = "daily"]
    fn fall_on_and_bounce_on_write_level_before_fall_damage_hit_ground_and_bounce_event() {
        let part = ImmediateWritePart::default();
        let mods: mod_api::Mods = vec![ModEntry::new(
            "immediate_landing",
            Box::new(ImmediateWriteMod(part.clone())),
        )]
        .into();
        crate::game::api_values::install_parts(&mods).unwrap();
        let _ = sunrise_client::block_contact::HOOKS.set(super::super::BLOCK_CONTACT_HOOKS);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                let state = |name| game.pack.assets.registry.default_state(name).unwrap();
                let (stone, dirt, slime) = (
                    state("minecraft:stone"),
                    state("minecraft:dirt"),
                    state("minecraft:slime_block"),
                );
                game.world.chunks.insert(
                    (0, 0),
                    std::sync::Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)),
                );
                let fall_pos = BlockPos::new(1, 64, 1);
                let bounce_pos = BlockPos::new(2, 64, 2);
                game.world
                    .set(fall_pos.x, fall_pos.y, fall_pos.z, stone)
                    .unwrap();
                game.world
                    .set(bounce_pos.x, bounce_pos.y, bounce_pos.z, slime)
                    .unwrap();
                *part.fall_replacement.lock().unwrap() = Some(dirt.try_into_api().unwrap());
                *part.bounce_replacement.lock().unwrap() = Some(stone.try_into_api().unwrap());

                let mut player = sunrise_client::motion::PlayerMotion::new(
                    game.motion,
                    Vec3::new(1.5, 65.0, 1.5),
                )
                .unwrap();
                player.fall_distance = 7.0;
                player.ground.on_ground = false;
                player.ground.supporting_block = Some(fall_pos);
                game.player = Some(player);

                let paused = {
                    let side = game_side!(game);
                    let player = game.player.as_mut().unwrap();
                    sunrise_client::motion::MotionWorld {
                        side: &side,
                        entities: &[],
                        player,
                        scene: &mut game.world,
                        assets: &game.pack.assets,
                        collision_binding: game.collision,
                        resources: game.motion,
                        health: &game.health_resources,
                        loop_context: &game.loop_context,
                        damage_hooks: &game.damage_hooks,
                        tag_items: Some(world_items!(game)),
                        block_registries: Some(&game.pack.block_registries),
                    }
                    .check_guest_fall_damage(-1.0, true, stone, fall_pos)
                    .unwrap()
                };
                let Some(sunrise_world::block_landing::FallPause::ModFall(pending)) = paused else {
                    panic!("expected FallPause::ModFall, got {paused:?}");
                };
                let (distance, multiplier) = game.mod_fall_on_player(
                    pending.state,
                    pending.position,
                    pending.distance,
                    pending.multiplier,
                );
                assert_eq!(
                    game.world.block(fall_pos.x, fall_pos.y, fall_pos.z),
                    dirt,
                    "fall_on write must apply at the landing site before causeFallDamage and HIT_GROUND"
                );
                assert_eq!((distance, multiplier), (7.0, 0.5));

                {
                    let side = game_side!(game);
                    let player = game.player.as_mut().unwrap();
                    sunrise_client::motion::MotionWorld {
                        side: &side,
                        entities: &[],
                        player,
                        scene: &mut game.world,
                        assets: &game.pack.assets,
                        collision_binding: game.collision,
                        resources: game.motion,
                        health: &game.health_resources,
                        loop_context: &game.loop_context,
                        damage_hooks: &game.damage_hooks,
                        tag_items: Some(world_items!(game)),
                        block_registries: Some(&game.pack.block_registries),
                    }
                    .finish_mod_fall(pending, distance, multiplier)
                    .unwrap();
                }

                let events = &game.player.as_ref().unwrap().events;
                assert!(
                    events.iter().any(|event| matches!(
                        event,
                        sunrise_client::motion::MotionEvent::Fall(
                            sunrise_client::landing::FallEvent::HitGround { state, .. }
                        ) if *state == dirt
                    )),
                    "HIT_GROUND must observe the replacement block written by fall_on: {events:?}"
                );
                assert!(
                    events.iter().any(|event| matches!(
                        event,
                        sunrise_client::motion::MotionEvent::Fall(
                            sunrise_client::landing::FallEvent::Damage { amount, .. }
                        ) if (*amount - 2.0).abs() < 1e-4
                    )),
                    "causeFallDamage must apply the 0.5 multiplier from fall_on: {events:?}"
                );

                let bounced =
                    game.mod_bounce_on_player(slime, bounce_pos, 4.0, Vec3::new(0.0, 0.5, 0.0));
                assert_eq!(bounced, Vec3::new(0.0, 0.9, 0.0));
                assert_eq!(
                    game.world.block(bounce_pos.x, bounce_pos.y, bounce_pos.z),
                    stone,
                    "bounce_on write must apply immediately at the bounce site"
                );
            });
        });
    }

    #[derive(Clone, Default)]
    struct FarmlandOnlyPart {
        seen: Arc<Mutex<Vec<(mod_api::BlockState, Option<mod_api::BlockState>)>>>,
    }

    #[mod_api::sunrise_part]
    impl BlockPart for FarmlandOnlyPart {
        fn fall_on(&self, host: &mut dyn LevelWriteHost, event: &mut FallOnEvent<'_>) {
            let live = host.block_state([event.pos.x, event.pos.y, event.pos.z]);
            self.seen.lock().unwrap().push((event.state, live));
            event.damage_multiplier = 0.0;
        }
    }

    struct FarmlandOnlyMod(FarmlandOnlyPart);
    #[mod_api::__subscriptions]
    impl Hooks for FarmlandOnlyMod {
        fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
            let farmland = r.block_key("minecraft:farmland");
            r.block_part(farmland, self.0.clone());
        }
    }

    /// After a mob tramples farmland to dirt, `fall_on` must still dispatch to the
    /// captured landed farmland state while the world cell and fall sound see dirt.
    #[test]
    #[ignore = "daily"]
    fn mob_farmland_trample_lends_farmland_state_to_fall_on_while_sound_reads_dirt() {
        let part = FarmlandOnlyPart::default();
        let mods: mod_api::Mods = vec![ModEntry::new(
            "farmland_trample_fall_on",
            Box::new(FarmlandOnlyMod(part.clone())),
        )]
        .into();
        crate::game::api_values::install_parts(&mods).unwrap();
        let _ = sunrise_client::block_contact::HOOKS.set(super::super::BLOCK_CONTACT_HOOKS);
        mod_platform::testing::with_mods(mods, || {
            crate::game::native_entity_refs::tests::with_game(|game| {
                game.options.dedicated = Some(Vec::new());
                let state = |name| game.pack.assets.registry.default_state(name).unwrap();
                let (farmland, dirt) = (state("minecraft:farmland"), state("minecraft:dirt"));
                for cx in -1..=1 {
                    for cz in -1..=1 {
                        let mut chunk = sunrise_client::chunk::Chunk::empty(cx, cz);
                        chunk.lit = true;
                        game.world
                            .chunks
                            .insert((cx, cz), std::sync::Arc::new(chunk));
                    }
                }
                game.world.set(1, 63, 1, farmland).unwrap();

                let pig = game.res.mob_types().get("minecraft:pig").unwrap().clone();
                let mut mob =
                    sunrise_client::mobs::Mob::new(42, pig, Vec3::new(1.5, 64.125, 1.5), 0.0);
                mob.ai.goals = Default::default();
                mob.ai.targets = Default::default();
                mob.fall_distance = 6.0;
                mob.velocity.y = -0.5;
                let start_health = mob.health;
                game.mobs.push(mob);
                game.entity_section_joined(42, [1.5, 64.125, 1.5]);

                game.tick_creature_entities().unwrap();

                assert_eq!(
                    game.world.block(1, 63, 1),
                    dirt,
                    "mob landing must have trampled farmland to dirt"
                );
                let seen = part.seen.lock().unwrap().clone();
                assert_eq!(
                    seen,
                    vec![(
                        farmland.try_into_api().unwrap(),
                        Some(dirt.try_into_api().unwrap()),
                    )],
                    "farmland fall_on part must receive captured farmland state while live level already holds dirt"
                );
                let mob = game.mob_object(42).unwrap();
                assert_eq!(
                    mob.health, start_health,
                    "farmland fall_on multiplier = 0.0 must suppress mob fall damage"
                );
            });
        });
    }
}
