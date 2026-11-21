//! The `living_equipment_change` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0157

pub trait Hooks {
    /// A mob's, an armor stand's or a player's equipment slot took a different
    /// stack (NeoForge's `LivingEquipmentChangeEvent`, posted in
    /// `LivingEntity.collectEquipmentChanges`): `entity` is its ID and `kind`
    /// its type (`minecraft:zombie`, `minecraft:player`), `slot` the
    /// `EquipmentSlot` (`mainhand`, `offhand`, `feet`, `legs`, `chest`, `head`,
    /// `body`, `saddle`), `from` what the slot held when this was last heard
    /// (none for empty) and `to` what it holds now. Once for each slot that
    /// changed, in that order. For mobs this runs during their own living
    /// tick, before AI and `entity_tick_post`; a mob canceled in
    /// `entity_tick_pre` does not reach equipment detection. A mob that joins
    /// wearing something counts from empty on its first living tick. This is
    /// a notification, not a veto: change the entity with its own helpers.
    ///
    /// Connected players are told during their body tick (`ServerPlayer.doTick`),
    /// before applying the new equipment modifiers. A player new to the level
    /// counts from empty, and the main hand is the
    /// selected hotbar slot, so a hotbar selection or a pickup into the
    /// selected slot is a `mainhand` change. Only their six slots `mainhand`,
    /// `offhand`, `feet`, `legs`, `chest` and `head` are told: a player's
    /// `body` and `saddle` slots are never told. The world host's own player, in
    /// single player or hosting a LAN world, isn't told here: `on_equip` hears
    /// it. Server-side. Subscribe to `living_equipment_change` directly.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_living_equipment_change",
        dispatch = "flat",
        side = "server",
        version = 2,
        contract = "listen",
        neoforge = "LivingEquipmentChangeEvent",
        cancellable = false
    )]
    fn living_equipment_change(
        entity: u32,
        kind: &str,
        slot: &str,
        from: Option<&ItemStack>,
        to: Option<&ItemStack>,
    );
}
