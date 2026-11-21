//! The `item_expire` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0231

pub trait Hooks {
    /// Before a dropped item is removed for reaching its lifespan on the server
    /// (`ItemEntity.tick`, NeoForge's `ItemExpireEvent`). The item is still in the
    /// level and can be read through `Host::entity`.
    ///
    /// `extra_life` starts at zero. Listeners set it or add to it with signed
    /// 32-bit wrapping arithmetic. The game adds the final result to the item's
    /// absolute lifespan the same way, then clamps the lifespan to `0..=32766`
    /// (NeoForge's `Short.MAX_VALUE - 1`, because the saved age is a short).
    /// The age does not reset, and the item is removed if its age still reaches
    /// the resulting lifespan. The next expiry check starts with zero again.
    /// Use `entity_leave_level` to observe actual removal.
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
    fn item_expire(entity: u32, stack: &ItemStack, extra_life: &mut i32);
}
