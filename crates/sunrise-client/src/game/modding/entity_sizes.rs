//! `entity_size` (`EntityEvent.Size`): vanilla's `Entity.refreshDimensions` lets a listener
//! change the size it just computed, whenever the pose changes, when `minecraft:scale` changes,
//! and from the constructor; a player's pose fit checks (`LocalPlayer.aiStep` crouching and
//! `Player.updatePlayerPose` / `canPlayerFitWithinBlocksAndEntitiesWhen`) resolve only the
//! candidate poses vanilla actually tests, on demand, with a read-only `Host`.
//!
//! Sunrise computes a mob's size from its state (`Mob::dims`) and keeps a player's in its body
//! (`PlayerMotion::refresh_dimensions`), storing the current pose's answer on the entity
//! (`pose::SizeAnswer`) for as long as that pose and vanilla size hold. The server's mobs are
//! asked before and after their ticks; players resolve candidate poses on demand during
//! `update_local_crouching` and `update_player_pose`, and current-pose changes outside motion
//! (initial tick, sleeping, dead, spectator, or an external scale/pose edit) are resolved without
//! precomputing inactive poses.

use super::Game;
use mod_api::{EntityRef, Pose};
use sunrise_client::motion::{PlayerMotion, no_player_collision_read};
use sunrise_client::pose::{Dimensions, Pose as MotionPose, SizeAnswer};
use sunrise_core::geometry::{Aabb, Vec3};

impl Game<'_> {
    /// Ask `entity_size` for `entity` in `pose` with vanilla dimensions `vanilla` and prior size
    /// `old`. Skips event construction when no mod subscribes. Valid finite non-negative answers
    /// are accepted without a synthetic upper cap; invalid answers are reported and keep vanilla.
    fn ask_entity_size_with_old(
        &self,
        id: u32,
        pose: Pose,
        vanilla: [f32; 3],
        old: [f32; 3],
    ) -> Option<SizeAnswer> {
        if !mod_api::__private::listened(mod_api::HookId::EntitySize) {
            return None;
        }
        let entity = EntityRef::new(self.native_lifecycle.epoch()?, id);
        let mut size = vanilla;
        self.native_entity_size(entity, pose, old, &mut size);
        // Accept any finite non-negative dimensions (matching vanilla `EntityDimensions` and
        // NeoForge `EntityEvent.Size`, with no synthetic upper cap). Reject non-finite or
        // negative values loudly rather than silently reverting or poisoning AABBs.
        let valid = size.iter().all(|v| v.is_finite() && *v >= 0.0);
        if !valid {
            log::warn!(
                "entity_size: ignoring invalid dimensions {size:?} for entity {id} in pose {pose:?}; keeping vanilla {vanilla:?}"
            );
        }
        Some(SizeAnswer {
            pose,
            vanilla,
            size: (valid && size != vanilla).then_some(size),
        })
    }

    /// Ask `entity_size` for an entity whose vanilla size in `pose` is `vanilla`, unless
    /// `previous` was already asked for both. `None`: nothing to ask, keep `previous`.
    fn ask_entity_size(
        &self,
        id: u32,
        pose: Pose,
        vanilla: [f32; 3],
        previous: Option<SizeAnswer>,
    ) -> Option<SizeAnswer> {
        if previous.is_some_and(|answer| answer.asked(pose, vanilla)) {
            return None;
        }
        // `getOldSize`: the size it had (the last answer's, or vanilla's on the first ask).
        let old = previous.map_or(vanilla, |answer| answer.effective());
        self.ask_entity_size_with_old(id, pose, vanilla, old)
    }

    /// The server's mobs whose pose or size changed since they were last asked.
    pub(super) fn native_mob_sizes(&mut self) {
        if !mod_api::__private::listened(mod_api::HookId::EntitySize) {
            return;
        }
        for i in 0..self.mobs.len() {
            let m = &self.mobs[i];
            let (id, pose, vanilla, previous) = (m.id, m.pose(), m.vanilla_dims(), m.size_answer);
            if let Some(answer) = self.ask_entity_size(id, pose, vanilla, previous)
                && let Some(m) = self.mobs.get_mut(i)
                && m.id == id
            {
                m.size_answer = Some(answer);
            }
        }
    }

    /// Resolve `entity_size` for `body` in `pose` at live scale, reusing `body.size_answer` when
    /// it already answered `(pose, vanilla)` and skipping event construction without subscribers.
    fn resolve_body_pose_size(
        &self,
        id: u32,
        body: &PlayerMotion,
        pose: MotionPose,
    ) -> sunrise_client::Result<(Dimensions, Option<SizeAnswer>)> {
        let scale = body.computed_scale()?;
        let vanilla = pose.dimensions(scale);
        let vanilla_arr = vanilla.to_array();
        let api_pose: Pose = pose.into();
        if let Some(answer) = body
            .size_answer
            .filter(|answer| answer.asked(api_pose, vanilla_arr))
        {
            return Ok((Dimensions::from_array(answer.effective()), Some(answer)));
        }
        if !mod_api::__private::listened(mod_api::HookId::EntitySize) {
            return Ok((vanilla, None));
        }
        let old = body.size_answer.map_or_else(
            || {
                if pose == body.pose {
                    vanilla_arr
                } else {
                    body.dimensions.to_array()
                }
            },
            |answer| answer.effective(),
        );
        let answer = self.ask_entity_size_with_old(id, api_pose, vanilla_arr, old);
        let dims = answer.map_or(vanilla, |answer| Dimensions::from_array(answer.effective()));
        Ok((dims, answer))
    }

    /// `Player.canPlayerFitWithinBlocksAndEntitiesWhen(pose)` using on-demand size resolution.
    fn body_can_fit_pose(
        &self,
        id: u32,
        body: &PlayerMotion,
        entities: &[Aabb],
        pose: MotionPose,
    ) -> sunrise_client::Result<(bool, Option<SizeAnswer>)> {
        let (dims, answer) = self.resolve_body_pose_size(id, body, pose)?;
        let bounds = dims
            .bounds(body.position)
            .deflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7));
        let fits = no_player_collision_read(body, &self.world, self.collision, entities, bounds)?;
        Ok((fits, answer))
    }

    /// `LocalPlayer.aiStep` crouching fit check, querying only the candidate poses vanilla tests
    /// (never when flying or swimming; `Standing` only if `Crouching` fits and neither shift nor
    /// sleeping short-circuits it).
    fn decide_body_local_crouching(
        &self,
        id: u32,
        body: &PlayerMotion,
        entities: &[Aabb],
    ) -> sunrise_client::Result<bool> {
        if body.flying || body.swimming {
            return Ok(false);
        }
        let (crouch_fits, _) = self.body_can_fit_pose(id, body, entities, MotionPose::Crouching)?;
        if !crouch_fits {
            return Ok(false);
        }
        if body.shift_key_down {
            return Ok(true);
        }
        if body.sleeping {
            return Ok(false);
        }
        let (stand_fits, _) = self.body_can_fit_pose(id, body, entities, MotionPose::Standing)?;
        Ok(!stand_fits)
    }

    /// Update `body.local_crouching` using on-demand candidate pose resolution.
    pub(in crate::game) fn update_body_local_crouching(
        &self,
        id: u32,
        body: &mut PlayerMotion,
        entities: &[Aabb],
    ) -> sunrise_client::Result<()> {
        body.local_crouching = self.decide_body_local_crouching(id, body, entities)?;
        Ok(())
    }

    /// Update `self.player.local_crouching` using on-demand candidate pose resolution.
    pub(in crate::game) fn update_local_crouching_with_sizes(
        &mut self,
        id: u32,
        entities: &[Aabb],
    ) -> sunrise_client::Result<()> {
        let Some(body) = self.player.as_ref() else {
            return Ok(());
        };
        let crouching = self.decide_body_local_crouching(id, body, entities)?;
        if let Some(body) = self.player.as_mut() {
            body.local_crouching = crouching;
        }
        Ok(())
    }

    /// `Player.updatePlayerPose` fit decision, querying only the candidate poses vanilla tests
    /// (`Swimming`, then `desired` when neither spectator nor passenger, then `Crouching` only if
    /// `desired` did not fit).
    fn decide_body_pose(
        &self,
        id: u32,
        body: &PlayerMotion,
        entities: &[Aabb],
        passenger: bool,
    ) -> sunrise_client::Result<Option<(MotionPose, Option<SizeAnswer>)>> {
        let (fits_swimming, swimming_answer) =
            self.body_can_fit_pose(id, body, entities, MotionPose::Swimming)?;
        if !fits_swimming {
            return Ok(None);
        }
        let desired = body.desired_pose();
        if body.spectator || passenger || body.is_passenger {
            return Ok(Some((desired, None)));
        }
        if desired == MotionPose::Swimming {
            return Ok(Some((MotionPose::Swimming, swimming_answer)));
        }
        let (fits_desired, desired_answer) = self.body_can_fit_pose(id, body, entities, desired)?;
        if fits_desired {
            return Ok(Some((desired, desired_answer)));
        }
        if desired == MotionPose::Crouching {
            return Ok(Some((MotionPose::Swimming, swimming_answer)));
        }
        let (fits_crouching, crouching_answer) =
            self.body_can_fit_pose(id, body, entities, MotionPose::Crouching)?;
        Ok(Some(if fits_crouching {
            (MotionPose::Crouching, crouching_answer)
        } else {
            (MotionPose::Swimming, swimming_answer)
        }))
    }

    fn apply_body_size(body: &mut PlayerMotion, answer: SizeAnswer) {
        body.size_answer = Some(answer);
        body.dimensions = Dimensions::from_array(answer.effective());
    }

    fn apply_pose_transition(
        body: &mut PlayerMotion,
        chosen: Option<(MotionPose, Option<SizeAnswer>)>,
        passenger: bool,
    ) -> sunrise_client::Result<Option<SizeAnswer>> {
        let mut resolved = None;
        if let Some((pose, answer)) = chosen {
            if body.pose != pose {
                body.pose = pose;
                body.refresh_dimensions()?;
            }
            resolved = answer;
        }
        // Passenger sit-up fixup (`pose.rs::passenger_sits_up`, #4509): a cushion seat clips the
        // feet into the floor so neither standing nor crawling fits; clear a prior swim.
        if passenger && body.pose == MotionPose::Swimming && !body.sleeping {
            body.pose = MotionPose::Standing;
            body.refresh_dimensions()?;
            resolved = None;
        }
        Ok(resolved)
    }

    fn resolve_current_body_size(
        &self,
        id: u32,
        body: &PlayerMotion,
        pre_resolved: Option<SizeAnswer>,
    ) -> Option<SizeAnswer> {
        let vanilla = body.pose.dimensions(body.scale).to_array();
        let api_pose: Pose = body.pose.into();
        if let Some(answer) = pre_resolved.filter(|answer| answer.asked(api_pose, vanilla)) {
            return Some(answer);
        }
        self.ask_entity_size(id, api_pose, vanilla, body.size_answer)
    }

    /// Run `Player.updatePlayerPose` (and a passenger's swim sit-up fixup) for `body`, resolving
    /// candidate and current-pose dimensions on demand.
    pub(in crate::game) fn update_body_pose(
        &self,
        id: u32,
        body: &mut PlayerMotion,
        entities: &[Aabb],
        passenger: bool,
    ) -> sunrise_client::Result<()> {
        let chosen = self.decide_body_pose(id, body, entities, passenger)?;
        let resolved = Self::apply_pose_transition(body, chosen, passenger)?;
        if let Some(answer) = self.resolve_current_body_size(id, body, resolved) {
            Self::apply_body_size(body, answer);
        }
        Ok(())
    }

    /// Run `Player.updatePlayerPose` (and a passenger's swim sit-up fixup) for `self.player`,
    /// resolving candidate and current-pose dimensions on demand.
    pub(in crate::game) fn update_player_pose_with_sizes(
        &mut self,
        id: u32,
        entities: &[Aabb],
        passenger: bool,
    ) -> sunrise_client::Result<()> {
        let Some(body) = self.player.as_ref() else {
            return Ok(());
        };
        let chosen = self.decide_body_pose(id, body, entities, passenger)?;
        let resolved = match self.player.as_mut() {
            Some(body) => Self::apply_pose_transition(body, chosen, passenger)?,
            None => return Ok(()),
        };
        let answer = self
            .player
            .as_ref()
            .and_then(|body| self.resolve_current_body_size(id, body, resolved));
        if let (Some(body), Some(answer)) = (self.player.as_mut(), answer) {
            Self::apply_body_size(body, answer);
        }
        Ok(())
    }

    /// A body's pose or size changed since it was last asked: ask, and size it by the answer.
    fn ask_body_size(&self, id: u32, body: &PlayerMotion) -> Option<SizeAnswer> {
        let vanilla = body.pose.dimensions(body.scale).to_array();
        self.ask_entity_size(id, body.pose.into(), vanilla, body.size_answer)
    }

    /// The client's own body, after its tick (covers sleeping, dead, initial tick, or a
    /// server-synced pose/scale change outside `update_player_pose_with_sizes`).
    pub(super) fn native_local_player_size(&mut self) {
        if !mod_api::__private::listened(mod_api::HookId::EntitySize) {
            return;
        }
        let Some(id) = self.remote.as_ref().map(|remote| remote.entity_id as u32) else {
            return;
        };
        let Some(answer) = self
            .player
            .as_ref()
            .and_then(|body| self.ask_body_size(id, body))
        else {
            return;
        };
        if let Some(body) = self.player.as_mut() {
            Self::apply_body_size(body, answer);
        }
    }

    /// Each guest's current-pose size on the server when its pose or vanilla size changed since
    /// it was last asked (initial join, spectator, or an external scale/sleep edit). Never
    /// precomputes inactive candidate poses or allocates a batch list.
    pub(super) fn native_guest_sizes(&mut self) {
        if !mod_api::__private::listened(mod_api::HookId::EntitySize) {
            return;
        }
        let len = self
            .lan
            .as_ref()
            .map_or(0, |host| host.hub.guests.as_slice().len());
        for i in 0..len {
            let Some((conn, id, pose, vanilla, previous)) = self.lan.as_ref().and_then(|host| {
                let guest = host.hub.guests.as_slice().get(i)?;
                let body = guest.motion.as_deref()?;
                Some((
                    guest.conn,
                    guest.entity_id as u32,
                    body.pose.into(),
                    body.pose.dimensions(body.scale).to_array(),
                    body.size_answer,
                ))
            }) else {
                continue;
            };
            if let Some(answer) = self.ask_entity_size(id, pose, vanilla, previous)
                && let Some(body) = self
                    .lan
                    .as_mut()
                    .and_then(|host| host.hub.guests.motion_mut(conn))
            {
                Self::apply_body_size(body, answer);
            }
        }
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
                for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
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
                // Valid finite non-negative dimensions are accepted without a synthetic 32-block cap.
                for accepted in [0.0, 32.0, 32.001, 64.0, 128.0] {
                    *size.lock().unwrap() = [accepted; 3];
                    game.mobs[i].size_answer = None;
                    game.native_mob_sizes();
                    assert_eq!(
                        game.mobs[i].dims(),
                        (
                            f64::from(accepted),
                            f64::from(accepted),
                            f64::from(accepted)
                        ),
                        "accepted {accepted}"
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

    struct ShortCrouch(Arc<Mutex<Vec<Pose>>>);

    #[mod_api::__subscriptions]
    impl Hooks for ShortCrouch {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            self.0.lock().unwrap().push(event.pose);
            if event.pose == Pose::Crouching {
                event.size[1] = event.size[0] / 0.6 * 0.8;
                event.size[2] = event.size[0];
            } else if event.pose == Pose::FallFlying {
                // Tall gliding box to verify non-standing/crouching/swimming candidates are queried.
                event.size[1] = 1.6;
            }
        }
    }

    #[test]
    #[ignore = "daily"]
    fn pose_fit_uses_the_resolved_crouch_size_including_cloned_motion() {
        use sunrise_client::{motion::PlayerMotion, pose::Pose as MotionPose};
        use sunrise_core::geometry::Vec3;
        for subscribed in [false, true] {
            let poses = Arc::new(Mutex::new(Vec::new()));
            let mods: mod_api::Mods = if subscribed {
                vec![mod_api::ModEntry::new(
                    "short_crouch",
                    Box::new(ShortCrouch(poses.clone())),
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
                    // Flying short-circuits LocalPlayer.aiStep before any pose fit check.
                    body.flying = true;
                    game.update_body_local_crouching(1, &mut body, &[]).unwrap();
                    assert!(!body.local_crouching);
                    assert!(poses.lock().unwrap().is_empty());
                    body.flying = false;

                    // Holding shift under a 1-block ceiling checks Crouching only (not Standing
                    // or Swimming).
                    body.shift_key_down = true;
                    let mut restored = body.clone();
                    game.update_body_local_crouching(1, &mut restored, &[])
                        .unwrap();
                    assert_eq!(restored.local_crouching, subscribed);
                    assert_eq!(
                        poses.lock().unwrap().as_slice(),
                        if subscribed {
                            &[Pose::Crouching][..]
                        } else {
                            &[][..]
                        }
                    );
                    poses.lock().unwrap().clear();

                    // update_body_pose checks Swimming, then desired (Crouching), and applies
                    // the resolved crouch answer without re-querying Crouching.
                    game.update_body_pose(1, &mut restored, &[], false).unwrap();
                    assert_eq!(
                        restored.pose,
                        if subscribed {
                            MotionPose::Crouching
                        } else {
                            MotionPose::Swimming
                        }
                    );
                    assert_eq!(
                        poses.lock().unwrap().as_slice(),
                        if subscribed {
                            &[Pose::Swimming, Pose::Crouching][..]
                        } else {
                            &[][..]
                        }
                    );
                    if subscribed {
                        assert_eq!(restored.dimensions.height, 0.8);
                        poses.lock().unwrap().clear();
                        // A live scale change is picked up immediately at the next pose check.
                        let key = restored.attributes.key("minecraft:scale").unwrap();
                        let handle = restored
                            .attributes
                            .get_instance(Some(key))
                            .unwrap()
                            .unwrap();
                        restored
                            .attributes
                            .edit(&handle, |value, observer| {
                                value.set_base_value(1.5, observer)
                            })
                            .unwrap();
                        let (fits, _) = game
                            .body_can_fit_pose(1, &restored, &[], MotionPose::Crouching)
                            .unwrap();
                        assert!(
                            !fits,
                            "1.5 * 0.8 = 1.2 does not fit under a 1-block ceiling"
                        );
                        assert_eq!(poses.lock().unwrap().as_slice(), &[Pose::Crouching]);

                        // Non-standard desired poses (FallFlying) are resolved on demand too.
                        poses.lock().unwrap().clear();
                        restored
                            .attributes
                            .edit(&handle, |value, observer| {
                                value.set_base_value(1.0, observer)
                            })
                            .unwrap();
                        restored.refresh_dimensions().unwrap();
                        restored.fall_flying = true;
                        game.update_body_pose(1, &mut restored, &[], false).unwrap();
                        // FallFlying was raised to 1.6 so it cannot fit under the 1-block ceiling,
                        // falling back to Crouching (0.8) instead of vanilla's 0.6 glide box.
                        assert_eq!(restored.pose, MotionPose::Crouching);
                        assert_eq!(
                            poses.lock().unwrap().as_slice(),
                            &[Pose::Swimming, Pose::FallFlying]
                        );
                    }
                })
            });
        }
    }

    #[test]
    #[ignore = "daily"]
    fn guest_pose_queries_only_active_vanilla_candidates() {
        use crate::game::native_item_places::tests::{guest as online_guest, open_to_lan};
        use sunrise_client::{chunk::Chunk, motion::PlayerMotion};
        use sunrise_core::geometry::Vec3;

        let poses = Arc::new(Mutex::new(Vec::new()));
        let mods: mod_api::Mods = vec![mod_api::ModEntry::new(
            "short_crouch",
            Box::new(ShortCrouch(poses.clone())),
        )]
        .into();
        mod_platform::testing::with_mods(mods, || {
            with_game(|game| {
                open_to_lan(game);
                game.streaming
                    .insert_remote(&mut game.world, Chunk::empty(0, 0));
                let mut guest = online_guest(1);
                guest.loaded = true;
                guest.position = [1.5, 64.0, 1.5];
                guest.motion = Some(Box::new(
                    PlayerMotion::new(game.motion, Vec3::new(1.5, 64.0, 1.5)).unwrap(),
                ));
                game.lan.as_mut().unwrap().hub.guests.push(guest);

                // native_guest_sizes asks only the current pose (Standing), never inactive poses.
                game.native_guest_sizes();
                assert_eq!(poses.lock().unwrap().as_slice(), &[Pose::Standing]);
                poses.lock().unwrap().clear();
                // Calling it again with no pose or scale change fires nothing.
                game.native_guest_sizes();
                assert!(poses.lock().unwrap().is_empty());

                // In open air, ServerPlayer.updatePlayerPose tests Swimming and reuses the cached
                // Standing answer; Crouching is never queried because Standing fits.
                game.guest_server_tick(1, true);
                assert_eq!(poses.lock().unwrap().as_slice(), &[Pose::Swimming]);
            })
        });
    }

    fn m_pose(game: &crate::game::Game<'_>, i: usize) -> Pose {
        game.mobs[i].pose()
    }
}
