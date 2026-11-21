//! The `item_smelted` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// `FurnaceResultSlot.checkTakeAchievements` ran at a furnace, blast
    /// furnace or smoker's result slot with a nonzero `removeCount`
    /// (NeoForge `PlayerEvent.ItemSmeltedEvent`). `player` is the live player
    /// handle on the side that processed the click; `stack` lends the stack
    /// passed to the callback (`getSmelting`), and `amount_removed` is
    /// `removeCount` (`getAmountRemoved`). Each listener receives the same
    /// original fields; changing the event record does not change the take.
    ///
    /// Fires during client prediction and on the server after
    /// `AbstractFurnaceBlockEntity.awardUsedRecipesAndPopExperience`.
    /// Pickup and throw pass the taken stack and its count. Shift-click
    /// fires once with the pre-move stack and the count moved (`onQuickCraft`).
    /// A hotbar swap has `removeCount == 0`, so it can award recipes and
    /// experience on the server without firing this event.
    #[hook(
        dispatch = "api_carrier",
        side = "both",
        contract = "listen",
        neoforge = "PlayerEvent.ItemSmeltedEvent",
        cancellable = false,
        event_per_listener = true,
        version = 2
    )]
    fn item_smelted(player: PlayerRef, stack: &ItemStackView<'_>, amount_removed: u32);
}
