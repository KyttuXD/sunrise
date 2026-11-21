//! Dispatch living travel at the resolved server player or mob branch, lending the
//! branch's input and the body's delta movement only when there is a subscriber.
use mod_api::{TravelMode, TravelMotion};
use sunrise_core::geometry::Vec3;
use sunrise_world::native_load_context::{LoopContext, LoopSide};
use sunrise_world::travel::TravelMode as GameTravelMode;

/// The catalog target for `living_travel`. The caller lends the current travel
/// input and delta movement, and the generated dispatch takes care of listener order.
pub fn native_living_travel(
    context: &LoopContext,
    entity: u32,
    kind: &str,
    mode: TravelMode,
    motion: TravelMotion<'_>,
    canceled: &mut bool,
) {
    let reader = sunrise_client_render::api_context::LoopReader(context);
    mod_api::dispatch_living_travel(&reader.context(), entity, kind, mode, motion, canceled);
}

/// Notify the authoritative travel listeners, if any. Edits write through to the
/// branch's argument and the body; cancellation skips only the branch and its moves
/// and flight damping. Client prediction never asks the hook.
#[inline(never)]
pub fn decide(
    context: &LoopContext,
    entity: u32,
    kind: &str,
    mode: impl FnOnce() -> GameTravelMode,
    input: &mut Vec3,
    velocity: &mut Vec3,
) -> bool {
    if context.side() != LoopSide::Server
        || !mod_api::__private::listened(mod_api::HookId::LivingTravel)
    {
        return true;
    }
    let mode = match mode() {
        GameTravelMode::Air => TravelMode::Air,
        GameTravelMode::Water => TravelMode::Water,
        GameTravelMode::Lava => TravelMode::Lava,
        GameTravelMode::Glide => TravelMode::Glide,
        GameTravelMode::Flying => TravelMode::Flying,
        GameTravelMode::VelocityOnly => TravelMode::VelocityOnly,
    };
    let motion = TravelMotion::new(
        [&mut input.x, &mut input.y, &mut input.z],
        [&mut velocity.x, &mut velocity.y, &mut velocity.z],
    );
    let mut canceled = false;
    native_living_travel(context, entity, kind, mode, motion, &mut canceled);
    !canceled
}
