//! The `player_container_close` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0164

pub trait Hooks {
    /// Server-side `PlayerContainerEvent.Close` (`ServerPlayer.doCloseContainer`): the
    /// menu has run `removed` and settled the cursor, and a replacement has not opened.
    /// `menu` is a read-only view of the closing window, not a handle to the player's
    /// next menu. The player's own inventory is not an opened container. Tearing down
    /// a player on disconnect is not `doCloseContainer` and posts no close event.
    /// Use `MenuPart::menu_removed` for a menu's own transient storage cleanup.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_player_container_close",
        dispatch = "flat",
        side = "server",
        contract = "listen",
        version = 2,
        neoforge = "PlayerContainerEvent.Close",
        cancellable = false,
        isolate = true
    )]
    fn player_container_close(player: PlayerRef, menu: ClosingMenu<'_>);
}
