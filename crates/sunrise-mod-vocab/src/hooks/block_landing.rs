//! The `block_landing` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// An entity landed on a block state this part is attached to (`Block.fallOn`), after falling `event.distance` blocks. `event.entity` lends a scoped [`LandingEntity`](mod_api::LandingEntity) view of the falling entity (id, type, position, velocity, fall distance, ground state, crouching, `is_suppressing_bounce`, dimensions, velocity writes and `reset_fall_distance`). Writable `event.distance` and `event.damage_multiplier` feed the fall calculation before hurt and sounds. The multiplier starts at the block's vanilla value (normally 1.0). Damage is `floor((distance + 1e-6 - safe_fall_distance) * damage_multiplier * fall_damage_multiplier_attribute)`, with safe fall distance 3 by default. Halving distance is not halving damage. Zero multiplier or a distance within the safe fall prevents damage and generic fall sounds. An entity that cannot be hurt by the fall (creative, fall immune) is told with zero multiplier and keeps its immunity, and a cancelled fall still lands. It runs at the landing site on `&mut dyn LevelWriteHost` (reading and writing blocks, destroying blocks, playing sounds, sending particles, level events, scheduling ticks, drawing the level's random in vanilla's order, and accessing attachments) before `causeFallDamage` and `HIT_GROUND` run, so writes take effect immediately for the fall sound, `HIT_GROUND`, and any later landing in the same tick. Work that needs `&mut dyn ServerHost` after the entity batch is queued with [`FallOnEvent::after_landing`](mod_api::FallOnEvent::after_landing) as an owned native callback; callbacks run in queue order once the batch ends, with local checks owned by the callback and no engine-side unchanged-block replay gate. Players, mobs and dropped items land here (an item has kind `minecraft:item` and zero multiplier); projectiles and the other entities do not.
    #[hook(
        side = "server",
        contract = "collect",
        out = "damage_multiplier",
        scope = "block",
        key = "state",
        target = "sunrise_client::game::native_hooks::block_landing::GameLevel<'_>::native_fall_on",
        neoforge = "none (vanilla Block.fallOn; LivingFallEvent is separate)",
        version = 3
    )]
    fn fall_on(
        dimension: &str,
        entity: mod_api::LandingEntity<'_>,
        state: BlockState,
        pos: BlockPos,
        distance: &mut f64,
        damage_multiplier: &mut f32,
        deferred: mod_api::__private::LandingDeferred<'_>,
        #[engine] ticks: &mut Ticks,
    );
    /// An entity bounced off a block state this part is attached to (`Block.bounceOn`), after falling `event.fall_distance` blocks. It fires only when the entity bounced, just before `GameEvent.BOUNCE` and vanilla setting its movement. `event.entity` lends a scoped [`LandingEntity`](mod_api::LandingEntity) view of the bouncing entity, and `event.movement` is the movement the bounce leaves it (`x`, `y`, `z`): change it to change what the entity keeps. It runs at the bounce site on `&mut dyn LevelWriteHost` (reading and writing blocks, destroying blocks, playing sounds, sending particles, level events, scheduling ticks, drawing the level's random in vanilla's order, and accessing attachments) before `GameEvent.BOUNCE` is emitted and before the new velocity is applied. Work that needs `&mut dyn ServerHost` after the entity batch is queued with [`BounceOnEvent::after_bounce`](mod_api::BounceOnEvent::after_bounce) as an owned native callback. Players and mobs bounce here; dropped items and the other entities do not.
    #[hook(
        side = "server",
        contract = "collect",
        out = "movement",
        scope = "block",
        key = "state",
        target = "sunrise_client::game::native_hooks::block_landing::GameLevel<'_>::native_bounce_on",
        neoforge = "none (vanilla Block.bounceOn; IBlockExtension.getBounceRestitution only returns a factor)",
        version = 2
    )]
    fn bounce_on(
        dimension: &str,
        entity: mod_api::LandingEntity<'_>,
        state: BlockState,
        pos: BlockPos,
        fall_distance: f64,
        movement: &mut [f64; 3],
        deferred: mod_api::__private::LandingDeferred<'_>,
        #[engine] ticks: &mut Ticks,
    );
}
