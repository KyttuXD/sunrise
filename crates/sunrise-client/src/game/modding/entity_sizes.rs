//! Entity dimensions at `Entity.refreshDimensions` and Player's pose collision queries.
use super::{Game, entity_sections::PlayerSectionScope};
use mod_api::{EntityRef, Pose};
use sunrise_client::{
    motion::{MotionTick, MotionWorld, PlayerMotion},
    pose::{Dimensions, Pose as MotionPose, SizeAnswer},
};
use sunrise_core::geometry::Aabb;

impl Game<'_> {
    fn resolve_entity_size(
        &self,
        id: u32,
        pose: Pose,
        old: [f32; 3],
        vanilla: [f32; 3],
    ) -> Option<SizeAnswer> {
        if !mod_api::__private::listened(mod_api::HookId::EntitySize) {
            return None;
        }
        let entity = EntityRef::new(self.native_lifecycle.epoch()?, id);
        let mut size = vanilla;
        self.native_entity_size(entity, pose, old, &mut size);
        Some(SizeAnswer {
            pose,
            vanilla,
            size: (size.map(f32::to_bits) != vanilla.map(f32::to_bits)).then_some(size),
        })
    }

    /// Refresh mobs whose current pose or vanilla dimensions changed.
    pub(super) fn native_mob_sizes(&mut self) {
        if !mod_api::__private::listened(mod_api::HookId::EntitySize) {
            return;
        }
        for i in 0..self.mobs.len() {
            let m = &self.mobs[i];
            let (id, pose, vanilla, previous) = (m.id, m.pose(), m.vanilla_dims(), m.size_answer);
            if previous.is_some_and(|answer| answer.asked(pose, vanilla)) {
                continue;
            }
            let old = previous.map_or(vanilla, |answer| answer.effective());
            if let Some(answer) = self.resolve_entity_size(id, pose, old, vanilla) {
                self.mobs[i].size_answer = Some(answer);
            }
        }
    }

    /// Refresh the server's stored current box before its connection tick uses it.
    pub(in crate::game) fn refresh_guest_size(&mut self, conn: u32) {
        if !mod_api::__private::listened(mod_api::HookId::EntitySize) {
            return;
        }
        let Some(guest) = self.lan.as_ref().and_then(|host| host.hub.guest(conn)) else {
            return;
        };
        let Some(body) = guest.motion.as_deref() else {
            return;
        };
        let pose = body.pose.into();
        let vanilla = body.pose.dimensions(body.scale).to_array();
        if body
            .size_answer
            .is_some_and(|answer| answer.asked(pose, vanilla))
        {
            return;
        }
        let old = body
            .size_answer
            .map_or(vanilla, |answer| answer.effective());
        if let Some(answer) = self.resolve_entity_size(guest.entity_id as u32, pose, old, vanilla)
            && let Some(body) = self
                .lan
                .as_mut()
                .and_then(|host| host.hub.guests.motion_mut(conn))
        {
            body.size_answer = Some(answer);
            body.dimensions = Dimensions::from_array(answer.effective());
        }
    }
}

/// No detached body for real ticks. Bot replay lends a separate simulation body but its
/// listener still reads the owning level and the real entity through the ordinary Host.
pub(in crate::game) struct GamePlayerMotion<'g, 'a, 's> {
    pub(in crate::game) game: &'g mut Game<'a>,
    pub(in crate::game) entities: &'s [Aabb],
    pub(in crate::game) sections: Option<PlayerSectionScope>,
    pub(in crate::game) detached_body: Option<&'s mut PlayerMotion>,
}

impl MotionTick for GamePlayerMotion<'_, '_, '_> {
    fn with_motion<T>(
        &mut self,
        step: impl FnOnce(&mut MotionWorld<'_, '_, '_>) -> sunrise_client::Result<T>,
    ) -> sunrise_client::Result<T> {
        let game = &mut *self.game;
        let side = game_side!(game);
        let server = game.is_logical_server();
        let _sections = self
            .sections
            .map(|scope| scope.begin(&mut game.native_lifecycle));
        let player = self
            .detached_body
            .as_deref_mut()
            .or(game.player.as_mut())
            .ok_or_else(|| std::io::Error::other("motion tick without a player"))?;
        step(&mut MotionWorld {
            side: &side,
            entities: self.entities,
            player,
            scene: &mut game.world,
            assets: &game.pack.assets,
            collision_binding: game.collision,
            resources: game.motion,
            health: &game.health_resources,
            tag_items: server.then(|| world_items!(game)),
            loop_context: &game.loop_context,
            damage_hooks: &game.damage_hooks,
            block_registries: server.then_some(&game.pack.block_registries),
        })
    }

    fn has_size_hook(&self) -> bool {
        mod_api::__private::listened(mod_api::HookId::EntitySize)
    }

    fn resolve_size(
        &self,
        pose: MotionPose,
        old: Dimensions,
        vanilla: Dimensions,
    ) -> Option<SizeAnswer> {
        let id = match &self.detached_body {
            Some(body) => body.entity_id,
            None => self.game.own_entity_id(),
        };
        self.game
            .resolve_entity_size(id as u32, pose.into(), old.to_array(), vanilla.to_array())
    }
}

#[cfg(test)]
mod tests {
    use crate::game::native_entity_refs::tests::with_game;
    use mod_api::{EntitySizeEvent, Hooks, Host, Pose};
    use std::sync::{Arc, Mutex};

    type Calls = Arc<Mutex<Vec<(Pose, [f32; 3], [f32; 3])>>>;

    struct Grow(Calls);

    #[mod_api::__subscriptions]
    impl Hooks for Grow {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            self.0
                .lock()
                .unwrap()
                .push((event.pose, event.old_size, *event.size));
            event.size[0] *= 2.0;
            event.size[1] *= 2.0;
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_listeners_size_sizes_the_mobs_box_until_vanilla_changes_it() {
        let calls = Calls::default();
        let mods = vec![mod_api::ModEntry::new(
            "grow",
            Box::new(Grow(calls.clone())),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                let id = game
                    .spawn_mob_spawned(
                        "cow",
                        [1.0, 70.0, 3.0],
                        crate::game::creatures::SpawnSite::Plain,
                    )
                    .unwrap();
                let i = game.mobs.iter().position(|m| m.id == id).unwrap();
                let vanilla = game.mobs[i].vanilla_dims();
                game.native_mob_sizes();
                let grown = [vanilla[0] * 2.0, vanilla[1] * 2.0, vanilla[2]];
                let m = &game.mobs[i];
                assert_eq!((m.width(), m.height()), (grown[0], grown[1]));
                assert_eq!(m.eye_height(), f64::from(vanilla[2]));
                assert!((m.bounds().size().y - f64::from(grown[1])).abs() < 1e-9);
                // Asked once for a pose and vanilla size, from the constructor's.
                game.native_mob_sizes();
                assert_eq!(
                    calls.lock().unwrap().as_slice(),
                    &[(m_pose(game, i), vanilla, vanilla)]
                );
                // Grown down to a baby: asked again, from the size it had.
                sunrise_client::mobs::finalize::set_baby(&mut game.mobs[i], true);
                game.native_mob_sizes();
                let calls = calls.lock().unwrap();
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[1].1, grown);
                assert_eq!(calls[1].2, game.mobs[i].vanilla_dims());
            })
        });
    }

    struct SetSize(Arc<Mutex<[f32; 3]>>);

    #[mod_api::__subscriptions]
    impl Hooks for SetSize {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            *event.size = *self.0.lock().unwrap();
        }
    }

    #[test]
    #[ignore = "daily"]
    fn invalid_size_answers_leave_the_mobs_default_box() {
        let size = Arc::new(Mutex::new([1.0; 3]));
        let mods = vec![mod_api::ModEntry::new(
            "set_size",
            Box::new(SetSize(size.clone())),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                let id = game
                    .spawn_mob_spawned(
                        "cow",
                        [1.0, 70.0, 3.0],
                        crate::game::creatures::SpawnSite::Plain,
                    )
                    .unwrap();
                let i = game.mobs.iter().position(|m| m.id == id).unwrap();
                let vanilla = game.mobs[i].vanilla_dims();
                let default_dims = game.mobs[i].dims();
                for invalid in [
                    f32::NAN,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    -1.0,
                    32.001,
                    1e30,
                ] {
                    for axis in 0..3 {
                        let mut answer = vanilla;
                        answer[axis] = invalid;
                        *size.lock().unwrap() = answer;
                        game.mobs[i].size_answer = None;
                        game.native_mob_sizes();
                        let mob = &game.mobs[i];
                        assert_eq!(mob.size_answer.unwrap().effective(), vanilla, "{answer:?}");
                        assert_eq!(mob.dims(), default_dims, "{answer:?}");
                    }
                }
                // The reported crash input must never become a pathfinding extent.
                *size.lock().unwrap() = [1e30; 3];
                game.mobs[i].size_answer = None;
                game.native_mob_sizes();
                assert_eq!(game.mobs[i].dims(), default_dims);
                // Both endpoints are valid: zero-size entities are vanilla too.
                for accepted in [0.0, 32.0] {
                    *size.lock().unwrap() = [accepted; 3];
                    game.mobs[i].size_answer = None;
                    game.native_mob_sizes();
                    assert_eq!(
                        game.mobs[i].dims(),
                        (accepted.into(), accepted.into(), accepted.into())
                    );
                }
                *size.lock().unwrap() = [0.0; 3];
                game.mobs[i].size_answer = None;
                for _ in 0..4 {
                    game.native_mob_sizes();
                    let mob = &game.mobs[i];
                    let path_mob = mob.path_mob();
                    assert_eq!((path_mob.width, path_mob.height), (0.0, 0.0));
                    let world = game.mob_world_base(&[]);
                    assert!(world.no_collision(mob.bounds()));
                    use sunrise_client::mobs::path::{Evaluator, Finder};
                    for evaluator in [Evaluator::Walk, Evaluator::Swim { breach: true }] {
                        let finder = Finder::new(&world, &path_mob, evaluator);
                        let _ = finder.walk_path_type(1, 70, 3);
                        let _ = finder.find(&[[3, 70, 3]], 8.0, 0, 32);
                    }
                }
            })
        });
    }

    #[test]
    #[ignore = "daily"]
    fn partial_collision_sizes_complete_navigation() {
        // Resource loading and the shared mod fixture lock are not navigation.
        // Start the deadline only once the fixture is ready, while keeping it
        // outside the worker so a saturated collision loop still fails boundedly.
        let (ready, started) = std::sync::mpsc::channel();
        let (done, result) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let size = Arc::new(Mutex::new([0.0; 3]));
            let mods = vec![mod_api::ModEntry::new(
                "set_size",
                Box::new(SetSize(size.clone())),
            )]
            .into();
            mod_platform::testing::with_mods(mods, || {
                with_game(|game| {
                    ready.send(()).unwrap();
                    use sunrise_client::mobs::path::{Evaluator, Finder, PathType};
                    game.world
                        .chunks
                        .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
                    let registry = &game.pack.assets.registry;
                    let stone = registry.default_state("minecraft:stone").unwrap();
                    let fence = registry.default_state("minecraft:oak_fence").unwrap();
                    for x in 0..4 {
                        for z in 0..4 {
                            game.world.set(x, 69, z, stone).unwrap();
                        }
                    }
                    game.world.set(0, 70, 0, fence).unwrap();
                    // At the block corner, even subnormal widths remain represented
                    // in the box's x/z extents; the fence post is not on the route.
                    let id = game
                        .spawn_mob_spawned(
                            "cow",
                            [0.0, 70.0, 0.0],
                            crate::game::creatures::SpawnSite::Plain,
                        )
                        .unwrap();
                    let i = game.mobs.iter().position(|m| m.id == id).unwrap();
                    let vanilla = game.mobs[i].vanilla_dims();
                    for answer in [[0.0; 3], [1e-30; 3], [1e-45; 3], vanilla] {
                        *size.lock().unwrap() = answer;
                        game.mobs[i].size_answer = None;
                        game.native_mob_sizes();
                        let mut mob = game.mobs[i].path_mob();
                        mob.on_ground = true;
                        let world = game.mob_world_base(&[]);
                        let finder = Finder::new(&world, &mob, Evaluator::Walk);
                        assert_eq!(finder.walk_path_type(0, 70, 0), PathType::Fence);
                        let path = finder.find(&[[2, 70, 0]], 8.0, 0, 32);
                        assert!(path.is_some(), "{answer:?}");
                    }
                    let id = game
                        .spawn_mob_spawned(
                            "silverfish",
                            [0.99, 70.0, 0.921591],
                            crate::game::creatures::SpawnSite::Plain,
                        )
                        .unwrap();
                    let i = game.mobs.iter().position(|m| m.id == id).unwrap();
                    game.mobs[i].extra.model_scale = 0.0625;
                    *size.lock().unwrap() = game.mobs[i].vanilla_dims();
                    game.native_mob_sizes();
                    let mut mob = game.mobs[i].path_mob();
                    mob.on_ground = true;
                    assert!((mob.width - 0.025).abs() < 1e-8);
                    assert!((mob.height - 0.01875).abs() < 1e-8);
                    let world = game.mob_world_base(&[]);
                    let finder = Finder::new(&world, &mob, Evaluator::Walk);
                    assert_eq!(finder.walk_path_type(0, 70, 0), PathType::Fence);
                    assert!(finder.find(&[[-1, 70, 0]], 8.0, 0, 32).is_some());
                });
            });
            done.send(()).unwrap();
        });
        started.recv().expect("navigation fixture must initialize");
        result
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("partial-collision navigation must finish within 30 seconds");
        worker.join().unwrap();
    }

    struct ShortCrouch(Arc<Mutex<Vec<(Pose, [f32; 3])>>>);

    #[mod_api::__subscriptions]
    impl Hooks for ShortCrouch {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            self.0.lock().unwrap().push((event.pose, *event.size));
            if event.pose == Pose::Crouching {
                event.size[1] = event.size[0] / 0.6 * 0.8;
                event.size[2] = event.size[0];
            }
        }
    }

    #[test]
    #[ignore = "daily"]
    fn a_pose_fit_queries_only_reached_candidates_and_uses_the_live_scale() {
        use super::GamePlayerMotion;
        use sunrise_client::{
            motion::{MotionTick, PlayerMotion},
            pose::Pose as MotionPose,
        };
        use sunrise_core::geometry::Vec3;
        for subscribed in [false, true] {
            let calls = Arc::new(Mutex::new(Vec::new()));
            let mods: mod_api::Mods = if subscribed {
                vec![mod_api::ModEntry::new(
                    "short_crouch",
                    Box::new(ShortCrouch(calls.clone())),
                )]
                .into()
            } else {
                Vec::new().into()
            };
            mod_platform::testing::with_mods(mods, || {
                with_game(|game| {
                    game.world
                        .chunks
                        .insert((0, 0), Arc::new(sunrise_client::chunk::Chunk::empty(0, 0)));
                    let stone = game
                        .pack
                        .assets
                        .registry
                        .default_state("minecraft:stone")
                        .unwrap();
                    game.world.set(0, 65, 0, stone).unwrap();
                    let mut body =
                        PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap();
                    body.entity_id = 1;
                    body.shift_key_down = true;
                    let mut motion = GamePlayerMotion {
                        game,
                        entities: &[],
                        sections: None,
                        detached_body: Some(&mut body),
                    };
                    motion.update_local_crouching().unwrap();
                    motion.update_player_pose().unwrap();
                    let body = motion.detached_body.as_deref_mut().unwrap();
                    assert_eq!(body.local_crouching, subscribed);
                    assert_eq!(
                        body.pose,
                        if subscribed {
                            MotionPose::Crouching
                        } else {
                            MotionPose::Swimming
                        }
                    );
                    if subscribed {
                        assert_eq!(body.dimensions.height, 0.8);
                        assert_eq!(
                            *calls.lock().unwrap(),
                            vec![
                                (Pose::Crouching, [0.6, 1.5, 1.27]),
                                (Pose::Swimming, [0.6, 0.6, 0.4]),
                                (Pose::Crouching, [0.6, 1.5, 1.27]),
                            ]
                        );
                        let key = body.attributes.key("minecraft:scale").unwrap();
                        let handle = body.attributes.get_instance(Some(key)).unwrap().unwrap();
                        body.attributes
                            .edit(&handle, |value, observer| {
                                value.set_base_value(0.5, observer)
                            })
                            .unwrap();
                        assert!(motion.can_fit_pose(MotionPose::Crouching).unwrap());
                        assert_eq!(
                            calls.lock().unwrap().last(),
                            Some(&(Pose::Crouching, [0.3, 0.75, 1.27 * 0.5]))
                        );
                    } else {
                        assert!(calls.lock().unwrap().is_empty());
                    }
                })
            });
        }
    }

    fn m_pose(game: &crate::game::Game<'_>, i: usize) -> Pose {
        game.mobs[i].pose()
    }
}
