//! The `item_expire` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0231

pub trait Hooks {
    /// A dropped item reaches its lifespan on the server (`ItemEntity.tick`,
    /// NeoForge's `EventHooks.onItemExpire` / `ItemExpireEvent`), before
    /// removal. Every mod may set or add to `extra_life`, in ticks, which
    /// starts at zero, and may query the live item entity through
    /// `host.entity(event.entity)` / `host.item_entity(...)`. This is an expiry
    /// attempt, not a removal notice: it runs even when a listener extends
    /// the item's life. Use `event.extra_life = event.extra_life.wrapping_add(ticks)`
    /// to preserve earlier listeners' contributions. The game adds the result
    /// to the lifespan (default 6000) the same way, then clamps the lifespan
    /// to `0..=32766` (NeoForge's `Short.MAX_VALUE - 1`), without resetting age.
    /// If the age still reaches the lifespan, the item is discarded and
    /// `entity_leave_level` fires before the next drop ticks. A drop whose
    /// stack cannot be represented in the API is reported and gets no extension.
    /// A panicking listener's contribution is ignored; its callback is reported
    /// and disabled for the session, and the remaining listeners still run.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_item_expire",
        dispatch = "flat",
        side = "server",
        contract = "listen",
        neoforge = "ItemExpireEvent",
        cancellable = false,
        version = 2,
        event_per_listener = true,
        isolate = true
    )]
    fn item_expire(entity: u32, stack: &mod_api::ItemStack, extra_life: &mut i32);
}
