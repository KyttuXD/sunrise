//! Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The client host's methods of the `menus` area, implemented by the game and reached through [`crate::ClientHost`].
#[sunrise_mod_api_macros::host_area]
pub trait MenusClientHost {
    /// Side: client.
    /// The local player in a logged-in client world.
    #[access(ClientSetup)]
    fn local_player(&self) -> Option<PlayerRef>;
}
/// The handles' reads of the `menus` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait MenusBackend {
    /// Side: server.
    /// Container id (0 for the player's inventory menu).
    #[access(WorldOnly)]
    fn menu_container_id(&self, menu: MenuRef) -> Option<i32>;
    /// Side: server.
    /// Registry menu type; the inventory menu has no MenuType in vanilla.
    /// The outer None means an unavailable menu.
    #[access(WorldOnly)]
    fn menu_kind(&self, menu: MenuRef) -> Option<Option<String>>;
    /// Side: both.
    /// Number of slots in AbstractContainerMenu.slots.
    #[access(WorldOnly)]
    fn menu_slot_count(&self, menu: MenuRef) -> Option<usize>;
    /// Side: both.
    /// Full stack at a menu index. Outer None means unavailable or out of
    /// range; Some(None) means an empty slot. `slot` is the slot's position in the menu's slot
    /// list, vanilla's `Slot.index`, the number container click packets carry; vanilla has no other
    /// player-visible slot number. `menu_slot_backing` gives the index inside the backing container.
    #[access(WorldOnly)]
    fn menu_slot(&self, menu: MenuRef, slot: usize) -> Option<Option<ItemStack>>;
    /// Side: both.
    /// Carried (cursor) stack; outer None means unavailable.
    #[access(WorldOnly)]
    fn menu_carried(&self, menu: MenuRef) -> Option<Option<ItemStack>>;
    /// Side: server.
    /// The storage menu slot `slot` shows (`Slot.container` and `getContainerSlot`): the
    /// player's inventory by `ENTITY_ITEMS` index (`InventoryMenu`'s armour slots 5..9 go head
    /// to feet), a block entity's or an entity's slot, the ender chest's, the crafting grid's
    /// or a mod's chest or transient container slot.
    #[access(WorldOnly)]
    fn menu_slot_backing(&self, menu: MenuRef, slot: usize) -> Option<MenuSlotBacking>;
    /// Side: server.
    /// How many of `stack` menu slot `slot` holds (`Slot.mayPlace`, then `Slot.getMaxStackSize`):
    /// a furnace's fuel slot one bucket, an armour slot one piece that fits it, a crafting result
    /// none. For an owned transient slot, this is the declared limit or the item's own
    /// limit, whichever is lower; its mod decides what a click moves.
    #[access(WorldOnly)]
    fn menu_slot_holds(&self, menu: MenuRef, slot: usize, stack: &ItemStack) -> Option<u32>;
    /// Side: server.
    /// Whether menu slot `slot` gives up what it holds (`Slot.mayPickup`): bound armour stays
    /// on outside creative.
    #[access(WorldOnly)]
    fn menu_slot_gives(&self, menu: MenuRef, slot: usize) -> Option<bool>;
}
/// The handles' client reads of the `menus` area, implemented by the game and reached through [`ClientBackend`].
// hidden: a handle's backend, reached through `ClientBackend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait MenusClientBackend {
    /// Side: client.
    /// The open inventory screen's slot boxes in menu slot order
    /// (`AbstractContainerScreen.leftPos`/`topPos` plus each `Slot.x`/`Slot.y`, 16x16). A box
    /// is `[x, y, width, height]` in GUI pixels, with `None` for a hidden slot.
    /// Returns `None` for a stale menu, another screen, or an inventory covered
    /// by the recipe book. The boxes follow the actual layout, including GUI
    /// scale and the recipe book's horizontal offset.
    #[access(WorldOnly)]
    fn menu_slot_boxes(&self, menu: MenuRef) -> Option<Vec<Option<[f32; 4]>>>;
    /// Side: client.
    /// Slot roles from the client layout and content menu declaration, in menu order.
    /// The length equals `slot_count`. Machine inputs (including fuel and brewing
    /// bottle slots) are `Input`; unknown roles are `Other`. Roles describe neither
    /// server backing nor rules for accepting items. Returns `None` outside
    /// a world, without an open menu, for stale/foreign menus or missing content types.
    /// Game types with no known slot count return `None`; workstations without
    /// classified layouts give all `Other` when their count is known. Creative returns the underlying inventory menu, not tab wrappers:
    /// read the screen type to distinguish them. Stable until the menu changes.
    /// Built per call with one Vec allocation and a table lookup; cache for the menu.
    #[access(WorldOnly)]
    fn menu_slot_kinds(&self, menu: MenuRef) -> Option<Vec<MenuSlotKind>>;
    /// Side: client.
    /// The local player's open merchant menu's synchronized offers, copied in display order
    /// (`MerchantMenu.getOffers`). Includes both payments, the result with its components,
    /// and the synchronized stock and price data. Use [`MerchantOffer::cost_a_count`] for
    /// the adjusted first payment. Reading does not select or take a trade or send a packet.
    ///
    /// `Ok(None)` means the menu is stale, foreign, closed, or not a merchant menu.
    /// An open merchant menu without offers returns `Ok(Some(Vec::new()))`.
    /// Returns [`LevelReadError::Unavailable`] when the phase or backend cannot supply the
    /// read, or an offer cannot be converted to an API value.
    #[access(WorldOnly)]
    fn menu_merchant_offers(
        &self,
        menu: MenuRef,
    ) -> Result<Option<Vec<MerchantOffer>>, LevelReadError>;
}
/// The handles' client writes of the `menus` area, implemented by the game and reached through [`ClientBackendMut`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait MenusClientBackendMut {
    /// Side: client.
    /// Submit clicks on the local player's live menu through normal prediction
    /// and `ContainerClick`. Slots use the same menu order as `slot`; `None`
    /// is outside (-999). All clicks are validated before any prediction or send.
    /// A drag expands to one complete quick-craft gesture.
    /// Refuses stale menus, invalid slots/hotbar indices, spectators, non-client hosts,
    /// the creative inventory screen and empty batches without changing items.
    /// True means submitted, not accepted by the server: its corrections win.
    #[access(WorldOnly)]
    fn menu_click(&mut self, menu: MenuRef, clicks: &[MenuClick]) -> bool;
    /// Side: client.
    /// Close the local player's open container screen as Escape does
    /// (`AbstractContainerScreen.onClose`, `LocalPlayer.closeContainer`): the
    /// server is told (`ContainerClose`) and the screen goes. Refuses stale
    /// menus, non-client hosts and a menu whose screen is not open, such as
    /// the inventory menu while playing. A server closes a menu with `close_container`.
    #[access(WorldOnly)]
    fn menu_close(&mut self, menu: MenuRef) -> bool;
    /// Side: client.
    /// Press button `button` of the local player's open menu as a click on
    /// one of its buttons does (`MultiPlayerGameMode.handleInventoryButtonClick`,
    /// `ContainerButtonClick`): the server's menu decides what it does
    /// (`clickMenuButton`, and `menu_button` for a mod's menu). For a client
    /// mod's own input on a menu's screen other than a press, such as a key
    /// or the wheel; no sound, that is the screen's. Refuses
    /// stale menus, non-client hosts and the inventory menu, which no server
    /// opened. True means sent, not that the menu took it.
    #[access(WorldOnly)]
    fn menu_click_button(&mut self, menu: MenuRef, button: i32) -> bool;
}
/// The handles' server writes of the `menus` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait MenusBackendMut {
    /// Side: server.
    /// Close a player's open menu as `ServerPlayer.closeContainer` does. The server queues
    /// `ContainerClose`, then removes the menu, returns the carried stack to the inventory,
    /// and fires `player_container_close`. In the player's own turn, uses their active
    /// inventory and cursor. Refuses stale menus, clients, a pending disconnect, and
    /// the player's own inventory menu.
    /// During a click action, true accepts a request to close that same window at commit; an
    /// aborted action leaves it open. A client closes its own screen with `close`.
    #[access(WorldOnly)]
    fn menu_close_container(&mut self, menu: MenuRef) -> bool;
    /// Side: server.
    /// Set a data slot of the owning mod's current blockless menu (`DataSlot.set`).
    /// Returns true only if the value changed. Publishes the final signed `i32` at
    /// root commit (immediately outside a transaction) if the index fits `u16`.
    /// Larger declared indices remain in server storage. An abort restores the
    /// value silently. Stale handles, unsupported menus, other mods and undeclared
    /// indices are refused.
    #[access(WorldOnly)]
    fn menu_set_data_slot(
        &mut self,
        menu: MenuRef,
        index: usize,
        value: i32,
    ) -> Result<bool, MenuDataRefusal>;
    /// Side: server.
    /// Replaces everything the open native menu `menu` shows with `items` (by slot, padded with
    /// empty ones, extra ones dropped, as [`PlayerRef::open_chest_menu`] takes them) and sends
    /// it at once as one `ClientboundContainerSetContentPacket`
    /// (`AbstractContainerMenu.sendAllDataToRemote`), without reopening the menu: the title,
    /// the cursor and the screen stay. Writing slots through `ItemPlace::MenuSlot` sends each
    /// changed slot with the next tick's sync instead; this is the whole-contents form, for a
    /// page turned or a list rebuilt. Each refusal sends nothing and changes nothing, and is
    /// named by [`MenuItemsRefusal`]: `Client`, `NoPlayer`, `NotOpen` (stale or not an open
    /// native menu), `Stack` (a stack the game's item conversion refuses), `OwnedSlots` (use
    /// `ItemPlace::MenuSlot` for an owned window), or `Unavailable` (outside the world phase).
    /// Inside a transaction, an abort puts the display contents back without sending a packet.
    #[access(WorldOnly)]
    fn menu_set_items(
        &mut self,
        menu: MenuRef,
        items: Vec<Option<ItemStack>>,
    ) -> Result<(), MenuItemsRefusal>;
    /// Side: server.
    /// Only inside `recipe_book_place`, once per click: places `recipe`, the recipe that hook
    /// was asked about, into a grid of the open mod menu `menu` as vanilla's
    /// `RecipeBookMenu.handlePlacement` and `ServerPlaceRecipe.placeRecipe` do. `grid_width`
    /// and `grid_height` give the grid's cells, row by row; `grid_slots` the menu slot of each
    /// cell (a recipe never fills a cell past the last slot given); `clear_slots` the slots
    /// emptied first, their items going back to the player's inventory (the grid's slots are
    /// cleared too). `use_max_items` places as many sets as the items allow (Shift), else one
    /// more than the grid holds when it already matches. Items are taken from the player's
    /// inventory and from what the menu's sources lend. Candidate IDs and client displays do
    /// not replace the server's recheck of live stacks, including custom predicates. Returns
    /// whether items were missing: the grid was cleared and the answer for `ghost`. Refused
    /// (and nothing changes) with [`RecipePlaceError`] outside that hook, for a different or
    /// stale menu, on a second call, for another recipe, a slot the menu lacks or hides, a
    /// slot without backing storage, a repeated slot, an empty grid, a recipe larger than the
    /// slots given, a slot whose pickup a part refuses, or a player with no room for the
    /// cleared items outside creative mode.
    #[access(WorldOnly)]
    fn menu_place_recipe_in_grid(
        &mut self,
        menu: MenuRef,
        recipe: &str,
        grid_width: u16,
        grid_height: u16,
        grid_slots: &[u16],
        clear_slots: &[u16],
        use_max_items: bool,
    ) -> Result<bool, RecipePlaceError>;
}
