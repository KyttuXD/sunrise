//! A dropped item's construction and its fall distance (`Entity.fallDistance`).
use super::{ItemEntity, ItemStep};
use crate::items::ItemStack;
use sunrise_core::{geometry::Vec3, position::BlockPos};

impl ItemEntity {
    /// A fresh `ItemEntity` at `position` moving at `velocity`: full health,
    /// no age, delay or owner, not yet ticked.
    pub fn new(id: u32, stack: ItemStack, position: Vec3, velocity: Vec3, bob_offset: f32) -> Self {
        Self {
            attachment_lifetime: Default::default(),
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            health: 5,
            fire_ticks: 0,
            thrower: None,
            target: None,
            id,
            stack,
            position,
            previous: position,
            velocity,
            on_ground: false,
            vertical_collision: false,
            horizontal_collision: false,
            fall_distance: 0.0,
            age: 0,
            pickup_delay: 0,
            bob_offset,
            ticks: 0,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
            // The caller gives it the own random that drew its UUID and bobOffs.
            random: sunrise_core::random::LegacyRandom::new(0),
        }
    }
    /// `Entity.move`'s fall bookkeeping after the item's move: `reset_clip`
    /// is the cast of a move of a block or more over fall-resetting blocks
    /// and water (the fall it leaves), then `checkFallDamage` adds the drop,
    /// narrowed to a float, outside water. Returns the block that receives
    /// `fallOn` while the accumulated distance and pre-restitution velocity
    /// are still live. The owner finishes the landing after `fallOn` returns.
    pub fn check_fall(
        &mut self,
        step: &ItemStep,
        reset_clip: impl FnOnce(f64, Vec3, Vec3) -> f64,
    ) -> Option<BlockPos> {
        let (from, moved, applied) = step.fall_move?;
        if applied {
            self.fall_distance = reset_clip(self.fall_distance, from, moved);
        }
        if !self.was_touching_water && moved.y < 0.0 {
            self.fall_distance -= f64::from(moved.y as f32);
        }
        (self.on_ground && self.fall_distance > 0.0).then(|| {
            step.vertical_contact.unwrap_or_else(|| {
                let offset = f64::from(sunrise_world::movement_support::LEGACY_SUPPORT_OFFSET);
                BlockPos::containing(self.position.subtract(Vec3::new(0.0, offset, 0.0)))
            })
        })
    }
    /// `Entity.baseTick`: lava halves the fall each tick.
    pub fn lava_fall(&mut self, in_lava: bool) {
        if in_lava {
            self.fall_distance *= 0.5;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipping_a_reset_block_resets_before_the_move_and_landing_notice() {
        let mut item = ItemEntity::new(
            1,
            ItemStack::new("minecraft:stone", 1),
            Vec3::new(0.5, 1.0, 0.5),
            Vec3::ZERO,
            0.0,
        );
        item.on_ground = true;
        item.fall_distance = 10.0;
        let step = ItemStep {
            old_velocity: Vec3::ZERO,
            movement: Some(Vec3::new(0.0, -2.0, 0.0)),
            vertical_contact: None,
            fall_move: Some((Vec3::new(0.5, 3.0, 0.5), Vec3::new(0.0, -2.0, 0.0), true)),
            collision: Some((Vec3::new(0.0, -2.5, 0.0), false)),
        };
        let on = item.check_fall(&step, |fall, _, _| {
            assert_eq!(fall, 10.0);
            0.0
        });
        assert_eq!(on, Some(BlockPos::new(0, 0, 0)));
        assert_eq!(item.fall_distance, 2.0);
        item.finish_move_restitution(&step);
        assert_eq!(item.fall_distance, 0.0);
    }
}
