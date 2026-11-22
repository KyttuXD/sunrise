//! The `on_sheared` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Shears `entity` (NeoForge's `IShearable.onSheared`): called on the part whose
    /// [`EntityPart::is_shearable`] said `Some(true)`, once the shear hooks let it through.
    /// Change the entity and play its sound here, and append drops to `event.drops`: the game
    /// spawns each as `spawnShearedDrop` does (one block above the entity's feet, with a small
    /// random push), posts the shear game event and damages the shears. `shears` is a read-only
    /// view over the resident stack; call `shears.project()` only when components or an owned
    /// snapshot are needed. Server-side.
    #[hook(
        target = "sunrise_client::game::native_hooks::on_sheared::Game<'_>::native_on_sheared",
        receiver = "write",
        dispatch = "direct",
        side = "server",
        contract = "collect",
        out = "drops",
        scope = "entity",
        key = "entity",
        neoforge = "IShearable.onSheared"
    )]
    fn on_sheared(
        entity: EntityRef,
        player: Option<PlayerRef>,
        shears: ItemStackView<'_>,
        drops: &mut Vec<ItemStack>,
    );
}
