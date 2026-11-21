//! The `living_equipment_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0157

pub trait Hooks {
    /// NeoForge's `LivingEquipmentChangeEvent`, at `LivingEntity.collectEquipmentChanges`.
    /// This server-side notification runs for each equipment slot that differs from the
    /// last tick's observation; a newly joined or respawned entity counts from empty.
    /// `entity` is the affected [`LivingRef`], `slot` is the [`EquipmentSlot`], and
    /// `from` and `to` lend the old and new stacks as [`ItemStackView`]s (`None` for
    /// an empty slot). Call [`ItemStackView::project`] only to read components or
    /// keep an owned copy. To find the entity's type, use `entity.entity().kind(host)`.
    ///
    /// Mobs are checked during their living tick before AI; connected players are
    /// checked in the living tick of `ServerPlayer.doTick`, before `player_tick_post`. A
    /// mob canceled in `entity_tick_pre` does not reach detection. Mobs, armor stands and connected players use all
    /// equipment slots in vanilla order, including `Body` and `Saddle`. A player's
    /// `MainHand` is the selected hotbar stack. The host's own local player, where
    /// simulated outside the connected-player path, is still heard by `on_equip`.
    /// This is a notification, not a veto; change living equipment through its handle.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_living_equipment_change",
        dispatch = "api_carrier",
        side = "server",
        version = 3,
        contract = "listen",
        isolate = true,
        neoforge = "LivingEquipmentChangeEvent",
        cancellable = false
    )]
    fn living_equipment_change(
        entity: LivingRef,
        slot: EquipmentSlot,
        from: Option<&ItemStackView<'_>>,
        to: Option<&ItemStackView<'_>>,
    );
}
