//! The `item_picked_up` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0050

pub trait Hooks {
    /// `player` touches the ground item entity `item`
    /// (`ItemEntity.playerTouch`), before its pickup delay and target are
    /// checked: it is asked on every tick the two touch, even while the item
    /// can't be taken. Leave `can_pickup` at `Default` for vanilla's rule,
    /// set `True` to take it despite its delay or another player's target
    /// (a full inventory still leaves it), or `False` to veto this pickup
    /// attempt without changing the item's pickup delay (matching NeoForge
    /// `ItemEntityPickupEvent.Pre`). The last mod to set it decides. The item
    /// handle can read and edit the resident item. If allowed, this attempt
    /// takes from the stack captured before the event, using the item's live
    /// pickup delay and target. `set_stack` changes the stack on the ground,
    /// not the captured stack. Server-side.
    #[hook(
        dispatch = "flat",
        side = "server",
        contract = "listen",
        neoforge = "ItemEntityPickupEvent.Pre",
        version = 3
    )]
    fn item_picked_up(player: u128, item: ItemEntityRef, can_pickup: &mut TriState);
}
