//! The `item_smelted` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `PlayerEvent.ItemSmeltedEvent` at `FurnaceResultSlot.checkTakeAchievements`.
    /// Reports a committed result take from a furnace, blast furnace or smoker, after the
    /// furnace's recipe experience has been paid out. `player` is the live player;
    /// `stack` borrows the result (`ItemStackView::project` copies components on demand).
    /// For a shift-click it describes the original result stack; `amount_removed` is only
    /// the count moved. Checks with zero removed (including the follow-up after shift-click
    /// and a hotbar swap) do not notify. The hook does not fire for cooking or automation.
    /// Non-cancellable; fires on the integrated and dedicated server. A panicking
    /// listener is disabled for the session without interrupting other listeners.
    #[hook(
        dispatch = "api_carrier",
        side = "server",
        contract = "listen",
        neoforge = "PlayerEvent.ItemSmeltedEvent",
        cancellable = false,
        isolate = true,
        event_per_listener = true,
        version = 2
    )]
    fn item_smelted(player: PlayerRef, stack: &ItemStackView<'_>, amount_removed: u32);
}
