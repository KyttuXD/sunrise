//! The `menu_clicked` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0034

pub trait Hooks {
    /// A click on an open mod menu, before its default slot action (`AbstractContainerMenu.clicked`).
    /// Only parts registered for `menu`, the receiving menu type, are asked. `slot` is in menu order
    /// (container slots, then any player inventory; -999 outside), `button` and `mode` are the
    /// click input, and `uuid` identifies the clicking player. Menu-button clicks, recipe placement
    /// and creative set-slot do not call it. Each listener receives the original click input.
    /// The receiving menu is live on the Host; use [`PlayerRef::container_menu`] or the native
    /// menu's `uuid` and `window` to reach it.
    /// Return [`Flow::Cancel`] to skip only the default click. `Flow::Continue` runs the normal
    /// slot action against current slots and cursor if the same menu is still valid. Opening a
    /// replacement or closing the menu leaves no default action for the old click to run.
    /// A display-only menu has no default moves. The server reconciles the client's prediction.
    /// There is no implicit transaction: use `host.transaction` to group your own writes.
    /// Committed writes are not undone if the default is cancelled or refused. A panic is logged
    /// and shown in an error toast naming the mod, disables its `menu_clicked` for the session,
    /// and supplies no answer.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_menu_clicked",
        dispatch = "api_carrier",
        side = "server",
        contract = "cancel",
        scope = "menu",
        key = "menu",
        out = "handled",
        cancellable = true,
        isolate = true,
        neoforge = "none (a Java menu overrides AbstractContainerMenu.clicked; no event)",
        version = 4
    )]
    fn menu_clicked(
        menu: &str,
        uuid: u128,
        window: i32,
        slot: i32,
        button: i32,
        mode: i32,
        handled: &mut bool,
    );
}
