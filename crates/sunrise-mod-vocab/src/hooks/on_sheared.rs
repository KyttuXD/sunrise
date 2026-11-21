//! The `on_sheared` entity Part hook.

trait Hooks {
    /// Shear the entity on the same Part that returned `Some(true)` from `is_shearable`,
    /// subject to the player's shear hook. Apply the entity's changes and play its sound here;
    /// return the drops (or an empty list). The game places drops, posts the shear event and
    /// damages the tool. An omitted method does nothing and returns no drops. The shears are
    /// borrowed: use [`ItemStackView::project`] only when you need its components or an owned copy.
    #[hook(
        target = "sunrise_client::game::part_shearing::Game<'_>::part_shear",
        side = "server",
        contract = "replace",
        out = "answer",
        scope = "entity",
        key = "kind",
        dispatch = "direct",
        callable = true,
        version = 2,
        neoforge = "IShearable.onSheared"
    )]
    fn on_sheared(
        #[engine] kind: &str,
        host: &mut dyn ServerHost,
        entity: EntityRef,
        player: Option<PlayerRef>,
        shears: ItemStackView<'_>,
    ) -> Vec<ItemStack>;
}
