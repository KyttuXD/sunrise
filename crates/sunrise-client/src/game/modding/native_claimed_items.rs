//! Main-hand input captured by an item behavior, and the client host's view raycast.
use super::{Game, Screen, api_values::TryIntoApi};
use mod_api::{
    __private::{client_item_use, contain_outside_world, mod_index_on},
    ClientHost, ClientItemUseBehavior, ItemStack,
    content::crash::{ModCall, enter_native_mod},
};
use std::sync::Arc;
use sunrise_core::geometry::{BlockTraversalBounds, Vec3};
use sunrise_world::world_ray::{RayBlockMode, RayFluidMode, clip_world_in_bounds};

/// Client input captured by a main-hand item behavior, not a living entity's item use.
pub(super) struct ActiveClientItemUse {
    mod_id: &'static str,
    mod_index: usize,
    behavior: Arc<dyn ClientItemUseBehavior>,
    original: ItemStack,
    /// Only for comparing the current stack's kind; the original snapshot never changes.
    item_id: Arc<str>,
    slot: usize,
    ticks: u32,
}

impl Game<'_> {
    /// The first block along the view within `distance`, in loaded chunks only;
    /// `None` on a miss or a nonpositive or non-finite `distance`.
    pub(super) fn raycast(&self, distance: f32) -> Option<mod_api::Hit> {
        let (_, hit) = self.cast_view_ray(Some(f64::from(distance)))?;
        Some(hit)
    }
    /// The first block along the view across loaded chunks only, and its distance; `None` on a miss.
    pub(super) fn view_hit(&self) -> Option<(f32, mod_api::Hit)> {
        self.cast_view_ray(None)
    }
    fn cast_view_ray(&self, max_distance: Option<f64>) -> Option<(f32, mod_api::Hit)> {
        if max_distance.is_some_and(|distance| distance <= 0.0 || !distance.is_finite()) {
            return None;
        }
        let eye = self.eye_position()?;
        let direction = super::view_vector(self.yaw, self.pitch);
        let mut chunks = self.world.chunks.keys();
        let &(cx, cz) = chunks.next()?;
        let (mut min_x, mut max_x, mut min_z, mut max_z) = (cx, cx, cz, cz);
        for &(x, z) in chunks {
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_z = min_z.min(z);
            max_z = max_z.max(z);
        }
        let bounds = BlockTraversalBounds {
            min: [
                min_x.saturating_mul(16),
                self.world.bottom_y(),
                min_z.saturating_mul(16),
            ],
            max: [
                max_x.saturating_mul(16).saturating_add(15),
                self.world.top_y() - 1,
                max_z.saturating_mul(16).saturating_add(15),
            ],
        };
        let reach = match max_distance {
            Some(distance) => distance,
            None => view_exit_distance(eye, direction, bounds)?,
        };
        let start = Vec3::new(eye[0], eye[1], eye[2]);
        let end =
            start.add_vector(Vec3::new(direction[0], direction[1], direction[2]).scale(reach));
        let result = clip_world_in_bounds(
            &mut super::boats::OutlineFluidRay(self),
            start,
            end,
            RayBlockMode::Outline,
            RayFluidMode::None,
            bounds,
        )
        .ok()?;
        if result.miss {
            return None;
        }
        let hit = result.hit;
        let pos = [hit.block.x, hit.block.y, hit.block.z];
        let point = hit.location;
        let state = self.world.block(pos[0], pos[1], pos[2]);
        let distance = start.distance_squared(point).sqrt();
        Some((
            distance as f32,
            mod_api::Hit {
                block_x: pos[0],
                block_y: pos[1],
                block_z: pos[2],
                x: point.x,
                y: point.y,
                z: point.z,
                block: mod_api::__private::block_key(&self.res.facts().get(state).name)?,
            },
        ))
    }

    fn can_capture_item_use(&self) -> bool {
        self.using.is_none()
            && !self.spectator()
            && self.screen != Screen::Dead
            && self
                .player
                .as_ref()
                .is_some_and(|p| p.health.health() > 0.0)
            && self
                .remote
                .as_ref()
                .is_some_and(|r| r.logged_in && r.session.connected())
    }

    /// Can the current client still hold this capture? Do not read the stack's components.
    fn client_item_use_valid(&self, active: &ActiveClientItemUse) -> bool {
        active.slot == self.selected
            && self.can_capture_item_use()
            && self
                .stack_in(active.slot)
                .is_some_and(|stack| stack.item == active.item_id && stack.count != 0)
    }

    /// Give the registered main-hand behavior this click instead of using a target or item.
    pub(in crate::game) fn start_client_item_use(&mut self) -> bool {
        let slot = self.selected;
        let Some(stack) = self.stack_in(slot) else {
            return false;
        };
        let Some((mod_index, mod_id, behavior)) = client_item_use(&stack.item) else {
            return false;
        };
        if !mod_index_on(mod_index)
            || !self.can_capture_item_use()
            || &*stack.item == "minecraft:air"
        {
            return false;
        }
        let Ok(original) = stack.try_into_api() else {
            return false;
        };
        self.client_item_use = Some(ActiveClientItemUse {
            mod_id,
            mod_index,
            behavior: Arc::clone(&behavior),
            original: original.clone(),
            item_id: Arc::clone(&stack.item),
            slot,
            ticks: 0,
        });
        self.call_client_item_use(mod_id, "use_item", |host| {
            behavior.use_item(host, &original, slot as u32);
        });
        true
    }

    /// Reset an invalid capture or count a held tick. Only input handling releases it.
    pub(in crate::game) fn tick_client_item_use(&mut self) {
        let Some(active) = self.client_item_use.as_ref() else {
            return;
        };
        if !mod_index_on(active.mod_index) || !self.client_item_use_valid(active) {
            self.reset_client_item_use();
        } else if self.right
            && let Some(active) = self.client_item_use.as_mut()
        {
            active.ticks = active.ticks.saturating_add(1);
        }
    }

    pub(in crate::game) fn reset_client_item_use(&mut self) {
        self.end_client_item_use(false);
    }

    /// Retire the capture before entering mod code. Only a valid input release calls
    /// `release_using`; every other end is a reset. Return whether a capture owned the input.
    pub(in crate::game) fn end_client_item_use(&mut self, release_requested: bool) -> bool {
        let Some(active) = self.client_item_use.take() else {
            return false;
        };
        if release_requested
            && mod_index_on(active.mod_index)
            && self.client_item_use_valid(&active)
        {
            self.call_client_item_use(active.mod_id, "release_using", |host| {
                active.behavior.release_using(
                    host,
                    &active.original,
                    active.slot as u32,
                    active.ticks,
                );
            });
        }
        if mod_index_on(active.mod_index) {
            self.call_client_item_use(active.mod_id, "on_stop_using", |host| {
                active.behavior.on_stop_using(
                    host,
                    &active.original,
                    active.slot as u32,
                    active.ticks,
                );
            });
        }
        true
    }

    fn call_client_item_use(
        &mut self,
        mod_id: &'static str,
        phase: &'static str,
        invoke: impl FnOnce(&mut dyn ClientHost),
    ) {
        let _guard = enter_native_mod(ModCall {
            id: mod_id,
            hook: phase,
            part: Some("ClientItemUseBehavior"),
        });
        if let Err(refusal) = contain_outside_world(|| invoke(self)) {
            refusal.report();
        }
    }
}

/// End the unbounded view query at the far face of the loaded extent. A
/// distance supplied by a mod never goes through this endpoint selection.
fn view_exit_distance(eye: [f64; 3], dir: [f64; 3], bounds: BlockTraversalBounds) -> Option<f64> {
    let mut near = 0.0_f64;
    let mut far = f64::INFINITY;
    for i in 0..3 {
        let lo = f64::from(bounds.min[i]);
        let hi = f64::from(bounds.max[i]) + 1.0;
        if dir[i] == 0.0 {
            if !(lo..=hi).contains(&eye[i]) {
                return None;
            }
        } else {
            let a = (lo - eye[i]) / dir[i];
            let b = (hi - eye[i]) / dir[i];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
    }
    (far.is_finite() && far >= near && far > 0.0).then_some(far)
}
