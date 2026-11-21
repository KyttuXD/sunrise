//! The `player_item_mend` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0256

pub trait Hooks {
    /// Mending restored `repair` durability to a server player's item. `experience` is the
    /// signed amount passed to the repair effect for this item: the orb's value for the first
    /// item, or remaining experience for another item. A data-pack effect can restore durability
    /// even for zero or negative input. A panicking listener is reported and disabled for the
    /// session; it does not interrupt the repair or other listeners. Server-side.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_item_mend",
        dispatch = "flat",
        side = "server",
        contract = "listen",
        contain_listener_panic = true,
        neoforge = "none (NeoForge has no such event)",
        version = 2
    )]
    fn player_item_mend(player: u128, name: &str, item: &str, repair: u32, experience: i32);
}
