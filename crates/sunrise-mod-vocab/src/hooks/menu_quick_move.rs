//! The `menu_quick_move` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0235

pub trait Hooks {
    /// A player shift-clicked slot `slot` of the menu of the mod block
    /// entity (`AbstractContainerMenu.quickMoveStack`, the method a Java
    /// menu overrides), on the server, before the game's own routing:
    /// move the stack where your block wants it. `subject` lends sequential
    /// access to the live resident block entity (`subject.with_view(...)`),
    /// the server `Host` (`subject.host()`), and atomic transactions
    /// (`subject.transaction(...)`); `with_view` returns `None` if the block
    /// entity was removed or replaced during the call. `slot` is the menu's
    /// index: below `container_slots` it is a container slot (the block's own,
    /// a linked slot, or a crafting result); from there on the player's inventory
    /// (27 main slots, then the hotbar). After the live slot allows pickup,
    /// `stack` copies what it holds. Take what you move out of `stack` and return
    /// `Flow::Cancel`. The same source keeps the remainder (`None` empties it)
    /// and the client gets the menu again. A result handler consumes its own inputs;
    /// the remainder stays available for the inputs it leaves until the grid changes.
    /// Left unhandled, the same menu routes the click without a second pickup check.
    /// A later Part sees what an earlier one did to `stack`.
    ///
    /// Writes through `subject.with_view(...)` are checked at the boundary
    /// and apply to the live resident block entity in place, marking the
    /// chunk dirty. The container notice (`BlockEntity.setChanged`) for the
    /// move's resident slot edits and the implicit notices from committed
    /// `ITEMS` writes follow settlement of the clicked slot and precede
    /// `menu_slot_take`. Deferred notices require the original backing container
    /// to remain in place. Surviving edits on an unhandled or refused move still
    /// notify. An explicit `BlockEntityRef::set_changed` call is immediate; a
    /// same-block write made by that notice's callback does not queue another
    /// notice. Use `subject.transaction(...)` when an edit must span the subject
    /// and other world storage atomically. A panicking callback is contained and disabled
    /// for `menu_quick_move` for the rest of the session, discarding only its
    /// `stack` and `handled` outputs. A later Part can still handle the move;
    /// if none does after a quick-move Part panics, the menu is resynced
    /// instead of falling back. A panic contained in a nested hook does
    /// not by itself reject the move. An invalid `stack` remainder leaves the clicked slot's original stack untouched.
    ///
    /// For a mod container on a vanilla menu type and on a mod menu type,
    /// the left or right button, a slot that holds something and that the
    /// mod's screen shows; not for a spectator or a dead player, a menu no
    /// longer valid, nor during a drag.
    /// Asked once per click, where vanilla asks `quickMoveStack` again while
    /// it keeps moving the same item. The player's state is swapped in, so
    /// `subject.host().players().acting_player()` names them.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_menu_quick_move",
        receiver = "associated",
        dispatch = "flat",
        side = "server",
        contract = "cancel",
        out = "handled",
        scope = "block_entity",
        key = "kind",
        neoforge = "none (a Java menu overrides AbstractContainerMenu.quickMoveStack; no event)",
        cancellable = true,
        version = 5
    )]
    fn menu_quick_move(
        kind: mod_api::BlockEntityTypeKey,
        subject: mod_api::BlockEntityScope<'_>,
        container_slots: usize,
        slot: usize,
        stack: &mut Option<mod_api::ItemStack>,
        handled: &mut bool,
    );
}
