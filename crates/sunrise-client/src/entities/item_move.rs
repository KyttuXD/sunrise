//! `ItemEntity`'s `Entity.move`: the tick's move step, its collision through
//! the move gate (#2414) and the block effects around it, in its own module so
//! `entities.rs` keeps its size.
use super::*;
impl ItemEntity {
    /// `ItemEntity.tick` up to the blocks' push. The game has just run the
    /// fluid push (`Entity::push_by_fluid`) and passes the heights it found;
    /// it pushes by a bubble column next (`Entity::push_by_bubble_column`) and
    /// then calls [`ItemEntity::tick_settle`]. `no_physics`: the item moves
    /// through blocks this tick ([`ItemEntity::inside_block`]).
    pub fn tick_move(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
        no_physics: bool,
        heights: (f64, f64),
        entity_boxes: &[Aabb],
        border: Option<&BorderGeometry>,
        blocks: Option<&sunrise_world::movement_properties::BoundMovementProperties>,
        context: &sunrise_world::native_load_context::LoopContext,
    ) -> ItemStep {
        let step = self.tick_move_until_fall(
            world,
            facts,
            collision,
            no_physics,
            None,
            heights,
            entity_boxes,
            border,
            blocks,
            context,
        );
        let _ = self.check_fall(&step, |fall, _, _| fall);
        self.finish_move_restitution(&step);
        self.finish_move_block_effects(world, facts, no_physics, blocks, step, context);
        step
    }

    /// Resolve the item's move, stopping before `Entity.checkFallDamage` and
    /// collision restitution. `escape_speed`: with `no_physics` on a server,
    /// the speed of `Entity.moveTowardsClosestSpace` (`nextFloat() * 0.2 + 0.1`).
    pub fn tick_move_until_fall(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        collision: &BlockCollisionRegistry<'_>,
        no_physics: bool,
        escape_speed: Option<f32>,
        (water_height, lava_height): (f64, f64),
        entity_boxes: &[Aabb],
        border: Option<&BorderGeometry>,
        blocks: Option<&BoundMovementProperties>,
        context: &sunrise_world::native_load_context::LoopContext,
    ) -> ItemStep {
        self.ticks += 1;
        self.previous = self.position;
        if self.pickup_delay > 0 && !self.has_infinite_pickup_delay() {
            self.pickup_delay -= 1;
        }
        let old_velocity = self.velocity;
        // Entity.updateInWaterStateAndDoFluidPushing: touching water resets the fall.
        if water_height > 0.0 {
            self.fall_distance = 0.0;
        }
        let in_water = water_height > f64::from(0.1f32);
        if in_water {
            // ItemEntity.setUnderwaterMovement: 0.99F, 5.0E-4F, 0.06F.
            self.set_fluid_movement(0.99_f32);
        } else if lava_height > f64::from(0.1f32) {
            // ItemEntity.setUnderLavaMovement: 0.95F, 5.0E-4F, 0.06F.
            self.set_fluid_movement(0.95_f32);
        } else {
            self.velocity.y -= self.get_gravity();
        }
        // ItemEntity.tick: out of a block placed on it after gravity, so the
        // escape's 0.75 scales the fallen vertical speed (#12018).
        if let Some(speed) = escape_speed {
            self.move_towards_closest_space(world, facts, speed);
        }
        let horizontal = self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z;
        // ItemEntity.tick: skip move 3 of 4 ticks while grounded with no
        // horizontal speed. The idle branch still runs bubble-column push
        // (`applyEffectsFromBlocksForLastMovements`), so lift builds up and
        // the item moves on the next wake-up (#4558).
        if !self.on_ground
            || horizontal > 1.0e-5
            || self.ticks.wrapping_add(self.id).is_multiple_of(4)
        {
            let scene = SceneCollisions {
                world,
                binding: collision,
                subject: None,
            };
            let from = self.position;
            let wanted = self.velocity;
            // The item's Entity.move goes through the move gate (#2414): armed
            // before the move's own handling, no-physics and the stuck
            // multiplier included. PR 1a records the movement unchanged.
            let mut gate = crate::move_gate::armed(
                context,
                self.id,
                "minecraft:item",
                sunrise_world::entity_move::MoverType::SelfMovement,
                wanted,
            );
            let mut wanted = gate.delta();
            // Entity.move with noPhysics: straight through, the ground as it
            // was — and vanilla returns before the stuck multiplier, which
            // stays pending for the next move with physics.
            let moved = if no_physics {
                wanted
            } else {
                // Entity.move: a block the item sits in (cobweb, powder snow)
                // scales this move once and stops it.
                if self.stuck.length_squared() > 1.0e-7 {
                    wanted = wanted.multiply(self.stuck);
                    self.stuck = Vec3::ZERO;
                    self.velocity = Vec3::ZERO;
                }
                // `Entity.collide`: the entities that `canBeCollidedWith` the item stop it
                // too (a boat, a shulker, a happy ghast held still; not a minecart), and
                // so does the world border (`Entity.collectColliders`).
                let mut collisions = crate::collisions::EntityCollisions {
                    blocks: scene,
                    entities: entity_boxes,
                    border: border.map(|b| (b, [self.position.x, self.position.z])),
                };
                crate::move_gate::entities_resolve_with_context(
                    &mut gate,
                    &mut collisions,
                    self.bounds(),
                    wanted,
                    0.0,
                    self.on_ground,
                    self.collision_context(),
                )
                .unwrap_or(Vec3::ZERO)
            };
            let applied = move_applies(wanted, moved);
            if applied {
                self.position = Vec3::new(
                    self.position.x + moved.x,
                    self.position.y + moved.y,
                    self.position.z + moved.z,
                );
            }
            if !no_physics {
                self.on_ground = wanted.y < 0.0 && moved.y != wanted.y;
            }
            // Entity.move: what the move was stopped on. They outlive the
            // tick, as vanilla's fields do, for the tracker that sends the
            // position at the type's update interval.
            self.vertical_collision = moved.y != wanted.y;
            // `Mth.equal` on x and z (1e-5 as a float), exact on y.
            let differs = |a: f64, b: f64| (b - a).abs() >= 9.999_999_747_378_752e-6;
            self.horizontal_collision = differs(moved.x, wanted.x) || differs(moved.z, wanted.z);
            let vertical_contact = (self.vertical_collision && !no_physics)
                .then(|| self.vertical_contact(world, collision, blocks, moved))
                .flatten();
            // `Entity.move` captures the effect state before `fallOn`, even if
            // the block then changes. Restitution uses the velocity left by `fallOn`.
            let slime =
                self.on_ground && vertical_contact.is_some_and(|pos| slime_at(world, facts, pos));
            return ItemStep {
                old_velocity,
                vertical_contact,
                movement: Some(moved),
                fall_move: (!no_physics).then_some((from, moved, applied)),
                collision: (!no_physics).then_some((wanted, slime)),
            };
        }
        // applyEffectsFromBlocksForLastMovements: still lift/drag while idle.
        if !no_physics {
            self.slime_step_on(world, facts);
        }
        ItemStep {
            old_velocity,
            vertical_contact: None,
            movement: None,
            fall_move: None,
            collision: None,
        }
    }

    /// Finish `Entity.checkFallDamage` and `restituteMovementAfterCollisions`
    /// after the landing block has acted on the live item.
    pub fn finish_move_restitution(&mut self, step: &ItemStep) {
        let (Some(moved), Some((wanted, slime))) = (step.movement, step.collision) else {
            return;
        };
        if self.on_ground {
            self.fall_distance = 0.0;
        }
        if moved.x != wanted.x {
            self.velocity.x = 0.0;
        }
        if moved.z != wanted.z {
            self.velocity.z = 0.0;
        }
        if self.vertical_collision {
            self.velocity.y = vertical_collision_velocity(
                self.velocity.y,
                moved.y,
                ITEM_GRAVITY,
                self.on_ground,
                slime,
            );
        }
    }

    /// `Entity.getOnPosLegacy` for a clipped move. Resolve the support under
    /// the full item box: at a honey wall, its center can be outside the slime
    /// block even though its box landed on it.
    fn vertical_contact(
        &self,
        world: &World,
        collision: &BlockCollisionRegistry<'_>,
        blocks: Option<&BoundMovementProperties>,
        movement: Vec3,
    ) -> Option<BlockPos> {
        let mut scene = SceneCollisions {
            world,
            binding: collision,
            subject: None,
        };
        let mut ground = sunrise_world::ground_support::GroundState::default();
        ground
            .check_support(self.on_ground, self.bounds(), Some(movement), |bounds| {
                sunrise_world::ground_support::find_supporting_block(
                    &mut scene,
                    sunrise_world::collision_context::CollisionContext::empty(false),
                    self.position,
                    bounds,
                    1_000_000,
                )
            })
            .ok()?;
        if let Some(blocks) = blocks {
            return Some(crate::motion::legacy_block_support_at(
                world,
                blocks,
                self.position,
                ground.supporting_block,
            ));
        }
        // Standalone tests can omit the movement table and its fence/wall tags.
        Some(ground.supporting_block.unwrap_or_else(|| {
            let offset = f64::from(sunrise_world::movement_support::LEGACY_SUPPORT_OFFSET);
            BlockPos::containing(self.position.subtract(Vec3::new(0.0, offset, 0.0)))
        }))
    }

    /// Resume the normal block effects after the collision response.
    pub fn finish_move_block_effects(
        &mut self,
        world: &World,
        facts: &BlockFacts,
        no_physics: bool,
        blocks: Option<&BoundMovementProperties>,
        step: ItemStep,
        context: &sunrise_world::native_load_context::LoopContext,
    ) {
        if step.movement.is_some() {
            // Entity.move applies the block's speed factor before block effects.
            if !no_physics && let Some(blocks) = blocks {
                let factor = crate::motion::block_speed_factor_at(
                    world,
                    blocks,
                    context,
                    self.position,
                    None,
                );
                self.velocity =
                    sunrise_world::entity_move::scale_by_block_speed(self.velocity, factor);
            }
            self.apply_honey_slide(world, facts);
            self.stuck = match self.stuck_multiplier(world, facts) {
                // Entity.makeStuckInBlock resets the fall.
                Some(stuck) => {
                    self.fall_distance = 0.0;
                    stuck
                }
                None => Vec3::ZERO,
            };
        }
    }
}
