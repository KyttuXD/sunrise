//! The `living_equipment_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0157

pub trait Hooks {
    /// A mob's, armor stand's or connected player's equipment slot changed (NeoForge's
    /// `LivingEquipmentChangeEvent` in `LivingEntity.collectEquipmentChanges`).
    /// `entity` is a [`LivingRef`], `slot` an [`EquipmentSlot`], and `from` and
    /// `to` lend the previous tick's and current stack as [`ItemStackView`]s
    /// (`None` for an empty slot). Call [`ItemStackView::project`] if you need
    /// components or an owned copy.
    ///
    /// Each changed slot fires in `EquipmentSlot` order, before moving to the
    /// next slot. Initial equipment compares against empty. For connected
    /// server players (including the owner on the integrated server), the
    /// comparison is in the player body tick, after item use and before its
    /// equipment modifiers are applied. `MainHand` reads the selected hotbar
    /// slot; all eight slots, including `Body` and `Saddle`, are checked.
    /// Mobs are checked in their server equipment pass and armor stands in
    /// their own tick. This is a notice, not a veto; the views cannot be edited.
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
