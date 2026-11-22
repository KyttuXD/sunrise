//! The `living_equipment_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A mob's, an armor stand's or a player's equipment slot took a different
    /// stack (NeoForge's `LivingEquipmentChangeEvent`, posted in
    /// `LivingEntity.collectEquipmentChanges` / `detectEquipmentUpdates`):
    /// `entity` is the [`LivingRef`] (call `entity.entity().kind(host)` for its
    /// type or `entity.set_equipment(...)` to edit it), `slot` the
    /// [`EquipmentSlot`] (`MainHand`, `OffHand`, `Feet`, `Legs`, `Chest`,
    /// `Head`, `Body`, `Saddle`), `from` a lazy [`ItemStackView`] of what the
    /// slot held at the previous equipment check (none for empty) and `to` a lazy
    /// [`ItemStackView`] of what it holds now (call [`ItemStackView::project`]
    /// on demand for components or an owned copy). Once for each slot that
    /// changed, in `EquipmentSlot` order, after the mobs' tick, so an entity
    /// that joins the level wearing something counts from empty on its first
    /// tick. It only hears: change the entity with its own helpers.
    ///
    /// Players are told the same way inside their server body tick
    /// (`ServerPlayer.doTick` -> `LivingEntity.detectEquipmentUpdates`), after
    /// per-tick item use and before equipment attribute updates, walking all
    /// eight `EquipmentSlot`s as vanilla's `PlayerEquipment` and
    /// `LivingEntity.collectEquipmentChanges` do: a player new to the level
    /// counts from empty, a spectator touching an unloaded chunk skips its body
    /// tick (and therefore this check), and `MainHand` is the selected hotbar
    /// slot, so a hotbar selection, a pickup into the selected slot or a
    /// finished item use is a `MainHand` change. Mobs and players are told only
    /// while a mod sets `living_equipment_listened`; opting in does not replay
    /// changes already detected. The world host's own
    /// player, in single player or hosting a LAN world, isn't told here:
    /// `on_equip` hears it. Server-side.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_living_equipment_change",
        dispatch = "api_carrier",
        side = "server",
        contract = "listen",
        neoforge = "LivingEquipmentChangeEvent",
        cancellable = false,
        version = 2
    )]
    fn living_equipment_change(
        entity: LivingRef,
        slot: EquipmentSlot,
        from: Option<ItemStackView<'_>>,
        to: Option<ItemStackView<'_>>,
    );
}
