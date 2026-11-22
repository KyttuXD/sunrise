//! Native post-collision restitution, including lazy bounce queries and effects.
use crate::registry::BlockStateId;
use sunrise_core::{geometry::Vec3, position::BlockPos};

pub struct RestitutionInput {
    pub block: BlockStateId,
    pub collided_x: bool,
    pub collided_z: bool,
    pub vertical_collision: bool,
    pub collision_below: bool,
    pub living: bool,
    pub resolved_movement: Vec3,
    pub fall_distance: f64,
}
pub trait RestitutionWorld {
    type Error;
    fn suppressing_bounce(&mut self) -> Result<bool, Self::Error>;
    fn entity_bounciness(&mut self) -> Result<f64, Self::Error>;
    fn velocity(&mut self) -> Result<Vec3, Self::Error>;
    fn effective_gravity(&mut self) -> Result<f64, Self::Error>;
    fn block_suppresses_bounce(&mut self, block: BlockStateId) -> Result<bool, Self::Error>;
    fn block_bounce_restitution(&mut self, block: BlockStateId) -> Result<f32, Self::Error>;
    fn air_drag(&mut self) -> Result<f32, Self::Error>;
    fn movement_support_position(&mut self) -> Result<BlockPos, Self::Error>;
    /// `Block.bounceOn`: the entity bounced off `block`; `movement` is what the bounce leaves
    /// it, and the answer is what it keeps (a block part may change it).
    fn bounce_on(
        &mut self,
        block: BlockStateId,
        position: BlockPos,
        fall_distance: f64,
        movement: Vec3,
    ) -> Result<Vec3, Self::Error>;
    fn bounce_event(&mut self) -> Result<(), Self::Error>;
    /// Native field write after bounce callbacks and before setting velocity.
    fn mark_position_for_sync(&mut self);
    fn set_velocity(&mut self, velocity: Vec3) -> Result<(), Self::Error>;
}
fn java_max(a: f64, b: f64) -> f64 {
    if a.is_nan() {
        a
    } else if b.is_nan() {
        b
    } else if a == 0.0 && b == 0.0 {
        f64::from_bits(a.to_bits() & b.to_bits())
    } else {
        a.max(b)
    }
}
/// A bounce suspended before `Block.bounceOn`, `GameEvent.BOUNCE`, position
/// sync and `set_velocity` so the owning level can lend `&mut dyn LevelWriteHost`
/// to a block part at the bounce site.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PendingBounce {
    pub block: BlockStateId,
    pub position: BlockPos,
    pub fall_distance: f64,
    pub movement: Vec3,
}

/// Provider errors preserve already performed effects; no rollback is attempted.
/// Call only when Entity.move's simulation/collision guards request restitution.
pub fn restitute_movement<W: RestitutionWorld>(
    world: &mut W,
    input: RestitutionInput,
) -> Result<(), W::Error> {
    restitute_movement_until_bounce(world, input, false).map(|_| ())
}

/// Run restitution up to `Block.bounceOn`, pausing when `pause` is true and a
/// bounce actually emits so the owning level can lend `&mut dyn LevelWriteHost`
/// before `bounce_event`, `mark_position_for_sync` and `set_velocity`.
pub fn restitute_movement_until_bounce<W: RestitutionWorld>(
    world: &mut W,
    input: RestitutionInput,
    pause: bool,
) -> Result<Option<PendingBounce>, W::Error> {
    let mut bounce = if world.suppressing_bounce()? {
        0.0
    } else {
        world.entity_bounciness()?
    };
    let old = world.velocity()?;
    let mut velocity = old;
    if input.collided_x {
        velocity.x = -old.x * bounce;
    }
    if input.collided_z {
        velocity.z = -old.z * bounce;
    }
    let mut emit = bounce > 0.0 && (input.collided_x || input.collided_z);
    if input.vertical_collision {
        if input.collision_below {
            if -old.y <= world.effective_gravity()?
                || world.suppressing_bounce()?
                || world.block_suppresses_bounce(input.block)?
            {
                bounce = 0.0;
            } else {
                let mut block = world.block_bounce_restitution(input.block)?;
                if !input.living {
                    block *= 0.8f32;
                }
                bounce = java_max(bounce, f64::from(block));
            }
        }
        let (gravity, drag) = if bounce > 0.0 {
            let fraction = input.resolved_movement.y / old.y;
            let gravity = fraction * world.effective_gravity()?;
            let drag = 1.0 + fraction * (f64::from(world.air_drag()?) - 1.0);
            emit = true;
            (gravity, drag)
        } else {
            (0.0, 1.0)
        };
        velocity.y = (gravity - old.y) * drag * bounce;
    }
    if emit {
        let position = world.movement_support_position()?;
        if pause {
            return Ok(Some(PendingBounce {
                block: input.block,
                position,
                fall_distance: input.fall_distance,
                movement: velocity,
            }));
        }
        velocity = world.bounce_on(input.block, position, input.fall_distance, velocity)?;
        world.bounce_event()?;
        world.mark_position_for_sync();
    }
    world.set_velocity(velocity)?;
    Ok(None)
}

/// Complete a paused bounce after the owning level ran `Block.bounceOn`.
pub fn finish_bounce<W: RestitutionWorld>(world: &mut W, movement: Vec3) -> Result<(), W::Error> {
    world.bounce_event()?;
    world.mark_position_for_sync();
    world.set_velocity(movement)
}
