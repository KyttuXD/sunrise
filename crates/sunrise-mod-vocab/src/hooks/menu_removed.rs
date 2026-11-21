//! Menu instance teardown, scoped to the owner of the menu type.
// catalog-order: 0035

pub trait Hooks {
    /// The owning part's `AbstractContainerMenu.removed` for a blockless mod menu.
    /// `opening_hand` is the hand captured at open, not the player's current hand.
    /// `owned_slots` is the detached transient storage in slot order, or empty for
    /// a display-only menu. The game does not give or drop these stacks: the part can
    /// return them, write them back to its backing item, or leave them to be discarded.
    /// `uuid` identifies the opener even if `player` is unavailable. Do not require a
    /// live player to release the part's own state. `window` names the retired window;
    /// its storage must not be looked up through the current menu.
    /// During a transaction the part receives the final stacks after the transaction
    /// settles, so a rolled-back write is never returned to the owner.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_menu_removed",
        dispatch = "flat",
        side = "server",
        contract = "listen",
        scope = "menu",
        key = "menu",
        isolate = true,
        neoforge = "none (a Java menu overrides AbstractContainerMenu.removed; no event)"
    )]
    fn menu_removed(
        menu: &str,
        block: Option<&str>,
        player: Option<PlayerRef>,
        uuid: u128,
        window: i32,
        opening_hand: Option<Hand>,
        owned_slots: &[Option<mod_api::ItemStack>],
    );
}
