//! Player tick phases that lend the movement world only while it does engine work.
use super::*;
use crate::pose::{Dimensions, Pose, SizeAnswer};
use crate::sprint::Keys;

/// Drive motion with the level and body borrowed for one phase at a time. Between phases
/// the game can lend its complete read host to a dimension query without borrowing the
/// scene or body mutably. The resident motion world has no mod host.
pub trait MotionTick {
    fn with_motion<T>(
        &mut self,
        step: impl FnOnce(&mut MotionWorld<'_, '_, '_>) -> Result<T>,
    ) -> Result<T>;

    fn has_size_hook(&self) -> bool {
        false
    }

    fn resolve_size(
        &self,
        _pose: Pose,
        _old: Dimensions,
        _vanilla: Dimensions,
    ) -> Option<SizeAnswer> {
        None
    }

    /// A resident size is only resolved on construction or when pose or scale changes.
    fn refresh_player_size(&mut self) -> Result<()> {
        if !self.has_size_hook() {
            return Ok(());
        }
        let (pose, old, vanilla, previous) = self.with_motion(|world| {
            let body = &world.player;
            let vanilla = body.pose.dimensions(body.scale);
            let old = body.size_answer.map_or(body.dimensions, |answer| {
                Dimensions::from_array(answer.effective())
            });
            Ok((body.pose, old, vanilla, body.size_answer))
        })?;
        if !previous.is_some_and(|answer| answer.asked(pose.into(), vanilla.to_array()))
            && let Some(answer) = self.resolve_size(pose, old, vanilla)
        {
            self.with_motion(|world| {
                world.player.dimensions = Dimensions::from_array(answer.effective());
                world.player.size_answer = Some(answer);
                Ok(())
            })?;
        }
        Ok(())
    }

    fn can_fit_pose(&mut self, pose: Pose) -> Result<bool> {
        Ok(fit_pose(self, pose)?.0)
    }

    /// LocalPlayer.aiStep: old input decides crouching, not the current movement pose.
    fn update_local_crouching(&mut self) -> Result<()> {
        let (test, shift, sleeping) = self.with_motion(|world| {
            let body = &world.player;
            Ok((
                !body.flying && !body.swimming && !body.is_passenger,
                body.shift_key_down,
                body.sleeping,
            ))
        })?;
        let crouching = test
            && self.can_fit_pose(Pose::Crouching)?
            && (shift || (!sleeping && !self.can_fit_pose(Pose::Standing)?));
        self.with_motion(|world| {
            world.player.local_crouching = crouching;
            Ok(())
        })
    }

    /// Player.updatePlayerPose after movement; the swimming check can short-circuit the rest.
    fn update_player_pose(&mut self) -> Result<()> {
        update_pose(self, false)
    }

    fn server_player_pose_tick(&mut self) -> Result<()> {
        self.with_motion(|world| {
            world.player.count_down_server_spin();
            world.player.refresh_attributes()
        })?;
        self.refresh_player_size()?;
        self.update_player_pose()
    }

    fn passenger_pose_tick(&mut self) -> Result<()> {
        self.with_motion(|world| world.passenger_base_tick())?;
        passenger_sits_up(self)
    }

    fn rider_tick(&mut self, direction: [f32; 2]) -> Result<()> {
        self.with_motion(|world| {
            world.passenger_base_tick()?;
            world.rider_travel_tick(direction)
        })?;
        passenger_sits_up(self)
    }

    fn simulation_tick(&mut self, direction: [f32; 2], jumping: bool) -> Result<()> {
        let previous = self.with_motion(|world| {
            world.player.health.advance_clock();
            world.fluid_base_tick()?;
            world.move_before_pose(direction, jumping)
        })?;
        finish_tick(self, previous)
    }

    fn controlled_simulation_tick(&mut self, keys: Keys) -> Result<()> {
        self.controlled_simulation_tick_with_sprint_window(keys, 7)
    }

    fn controlled_simulation_tick_with_sprint_window(
        &mut self,
        keys: Keys,
        sprint_window: i32,
    ) -> Result<()> {
        local_tick(self, keys, sprint_window, None)
    }

    /// Replay emits no movement notifications, but still asks for the same movement inputs.
    fn replayed_tick(&mut self, keys: Keys, sprinting: bool) -> Result<()> {
        self.with_motion(|world| {
            world.player.replaying = true;
            Ok(())
        })?;
        let result = local_tick(self, keys, 7, Some(sprinting));
        self.with_motion(|world| {
            world.player.replaying = false;
            Ok(())
        })?;
        result
    }
}

impl MotionTick for MotionWorld<'_, '_, '_> {
    #[inline]
    fn with_motion<T>(
        &mut self,
        step: impl FnOnce(&mut MotionWorld<'_, '_, '_>) -> Result<T>,
    ) -> Result<T> {
        step(self)
    }
}

fn local_tick(
    tick: &mut (impl MotionTick + ?Sized),
    keys: Keys,
    sprint_window: i32,
    replay_sprinting: Option<bool>,
) -> Result<()> {
    tick.refresh_player_size()?;
    tick.with_motion(|world| {
        world.player.health.advance_clock();
        world.fluid_base_tick()
    })?;
    tick.update_local_crouching()?;
    let previous =
        tick.with_motion(|world| world.apply_local_input(keys, sprint_window, replay_sprinting))?;
    finish_tick(tick, previous)
}

fn finish_tick(tick: &mut (impl MotionTick + ?Sized), previous: Vec3) -> Result<()> {
    tick.refresh_player_size()?;
    tick.update_player_pose()?;
    tick.with_motion(|world| world.finish_simulation_tick(previous))
}

fn fit_pose(
    tick: &mut (impl MotionTick + ?Sized),
    pose: Pose,
) -> Result<(bool, Option<SizeAnswer>)> {
    let (vanilla, previous, old) = tick.with_motion(|world| {
        let body = &mut *world.player;
        let vanilla = pose.dimensions(body.attribute("minecraft:scale")? as f32);
        Ok((vanilla, body.size_answer, body.dimensions))
    })?;
    let answer = previous
        .filter(|answer| answer.asked(pose.into(), vanilla.to_array()))
        .or_else(|| {
            tick.has_size_hook()
                .then(|| tick.resolve_size(pose, old, vanilla))
                .flatten()
        });
    let dims = answer.map_or(vanilla, |answer| Dimensions::from_array(answer.effective()));
    let fits = tick.with_motion(|world| {
        let bounds = dims.bounds(world.player.position);
        // Player.canPlayerFitWithinBlocksAndEntitiesWhen deflates the candidate by 1e-7.
        world.no_player_collision(bounds.deflate(Vec3::new(1.0e-7, 1.0e-7, 1.0e-7)))
    })?;
    Ok((fits, answer))
}

fn update_pose(tick: &mut (impl MotionTick + ?Sized), passenger: bool) -> Result<()> {
    let (swim_fits, swim_size) = fit_pose(tick, Pose::Swimming)?;
    if !swim_fits {
        return Ok(());
    }
    let (desired, spectator) = tick.with_motion(|world| {
        let body = &world.player;
        let desired = if body.sleeping {
            Pose::Sleeping
        } else if body.swimming {
            Pose::Swimming
        } else if body.fall_flying {
            Pose::FallFlying
        } else if body.auto_spin_attack_ticks > 0 {
            Pose::SpinAttack
        } else if body.shift_key_down && !body.flying {
            Pose::Crouching
        } else {
            Pose::Standing
        };
        Ok((desired, body.spectator))
    })?;
    let (pose, answer) = if spectator || passenger {
        (desired, None)
    } else {
        let (fits, size) = fit_pose(tick, desired)?;
        if fits {
            (desired, size)
        } else {
            let (fits, size) = fit_pose(tick, Pose::Crouching)?;
            if fits {
                (Pose::Crouching, size)
            } else {
                (Pose::Swimming, swim_size)
            }
        }
    };
    let changed = tick.with_motion(|world| {
        let body = &mut *world.player;
        if body.pose == pose {
            return Ok(false);
        }
        if let Some(answer) = answer {
            body.size_answer = Some(answer);
        }
        body.set_pose(pose)?;
        Ok(true)
    })?;
    if changed && answer.is_none() {
        tick.refresh_player_size()?;
    }
    Ok(())
}

fn passenger_sits_up(tick: &mut (impl MotionTick + ?Sized)) -> Result<()> {
    update_pose(tick, true)?;
    // The cushion seat can put the feet into its support block (#4509).
    let sat_up = tick.with_motion(|world| {
        if world.player.pose == Pose::Swimming && !world.player.sleeping {
            world.player.set_pose(Pose::Standing)?;
            return Ok(true);
        }
        Ok(false)
    })?;
    if sat_up {
        tick.refresh_player_size()?;
    }
    Ok(())
}

impl MotionWorld<'_, '_, '_> {
    fn passenger_base_tick(&mut self) -> Result<()> {
        self.player.fluids.was_eye_in_water = self.player.is_eye_in_fluid(WATER);
        self.update_fluids()?;
        // Player.tick: a passenger is never on the ground until its own move says otherwise.
        self.player.ground.on_ground = false;
        self.player.ground.on_ground_no_blocks = false;
        self.player.ground.supporting_block = None;
        self.player.swimming = false;
        self.player.fall_flying = false;
        self.player.fall_fly_ticks = 0;
        Ok(())
    }
}
