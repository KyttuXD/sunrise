//! Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The host's methods both sides share of the `item_entities` area, implemented by the game and reached through [`Host`].
#[sunrise_mod_api_macros::host_area]
pub trait ItemEntitiesHost {
    /// Side: both.
    /// Narrows an entity to a dropped item, or None for another kind, a
    /// stale handle, a client, or an item temporarily taken out for ticking.
    #[access(WorldOnly)]
    fn item_entity(&self, entity: EntityRef) -> Option<ItemEntityRef>;
}
/// The handles' reads of the `item_entities` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait ItemEntitiesBackend {
    /// Side: server.
    /// Lend `ItemEntity.getItem` once, as in [`ItemEntityRef::with_stack`].
    /// An empty stack returns `None` without calling the reader.
    #[access(WorldOnly)]
    #[manual_handle]
    fn item_entity_stack(
        &self,
        item: ItemEntityRef,
        read: &mut dyn for<'a> FnMut(ItemStackView<'a>),
    ) -> Option<()>;
    /// Side: server.
    /// The live signed pickup delay (`32767` never ticks down), or `None` if the item is unavailable.
    #[access(WorldOnly)]
    fn item_entity_pickup_delay(&self, item: ItemEntityRef) -> Option<i32>;
    /// Side: server.
    /// `ItemEntity.getAge`, including negative lifetime sentinels.
    #[access(WorldOnly)]
    fn item_entity_age(&self, item: ItemEntityRef) -> Option<i32>;
    /// Side: server.
    /// The UUID set by `ItemEntity.setTarget`, limiting pickup to that player;
    /// None also means an unavailable item. This is not the thrower.
    #[access(WorldOnly)]
    fn item_entity_target(&self, item: ItemEntityRef) -> Option<u128>;
    /// Side: server.
    /// `ItemEntity.getOwner`: the thrower as a resolved entity, not the pickup
    /// target. None for no thrower, an unloaded or different-level thrower,
    /// an unavailable handle, or one Sunrise cannot resolve.
    #[access(WorldOnly)]
    fn item_entity_owner(&self, item: ItemEntityRef) -> Option<EntityRef>;
}
/// The handles' writes of the `item_entities` area, implemented by the game and reached through [`BackendMut`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait ItemEntitiesBackendMut {
    /// Side: server.
    /// Replaces the complete stack using `ItemEntity.setItem`. Use `None` for an
    /// empty stack; the entity remains available until its normal tick discards
    /// it, if still empty. Returns `Ok(false)` for an unavailable item or outside
    /// a server. To remove the entity immediately, use its removal handle.
    ///
    /// The new stack is visible immediately. On an open transaction, abort
    /// restores the old stack for the same resident item. A block-entity tick
    /// does not open a transaction automatically; use the tick scope's
    /// `transaction` to commit this write together with a subject edit.
    #[access(WorldOnly)]
    fn item_entity_set_stack(
        &mut self,
        item: ItemEntityRef,
        stack: Option<ItemStack>,
    ) -> Result<bool, StackWriteError>;
    /// Side: server.
    /// `ItemEntity.setPickUpDelay`: stores the signed `i32` as given. The save
    /// format narrows it to a signed short, just like vanilla.
    #[access(WorldOnly)]
    fn item_entity_set_pickup_delay(&mut self, item: ItemEntityRef, ticks: i32) -> bool;
    /// Side: server.
    /// `ItemEntity.setDefaultPickUpDelay`: ten ticks.
    #[access(WorldOnly)]
    fn item_entity_set_default_pickup_delay(&mut self, item: ItemEntityRef) -> bool;
    /// Side: server.
    /// `ItemEntity.setNoPickUpDelay`.
    #[access(WorldOnly)]
    fn item_entity_set_no_pickup_delay(&mut self, item: ItemEntityRef) -> bool;
    /// Side: server.
    /// `ItemEntity.setNeverPickUp`: the infinite-delay sentinel 32767.
    #[access(WorldOnly)]
    fn item_entity_set_never_pickup(&mut self, item: ItemEntityRef) -> bool;
    /// Side: server.
    /// `ItemEntity.setTarget`; None allows any player to pick it up.
    #[access(WorldOnly)]
    fn item_entity_set_target(&mut self, item: ItemEntityRef, target: Option<u128>) -> bool;
}
