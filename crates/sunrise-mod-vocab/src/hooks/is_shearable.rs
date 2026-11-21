//! The `is_shearable` entity Part hook.

trait Hooks {
    /// Answer whether the entity can be sheared by these shears (`player` is absent for a
    /// dispenser). The first answer wins: `Some(true)` selects this Part for `on_sheared`,
    /// `Some(false)` refuses shearing, and `None` defers to the next Part and then the species.
    /// Use `shears.item()`, `count()` or `damage()` without copying the stack; call
    /// [`ItemStackView::project`] if you need its components or an owned copy.
    #[hook(
        target = "sunrise_client::game::part_shearing::Game<'_>::part_shearable",
        side = "server",
        receiver = "read",
        contract = "replace",
        out = "shearable",
        scope = "entity",
        key = "kind",
        dispatch = "direct",
        callable = true,
        version = 2,
        neoforge = "IShearable.isShearable"
    )]
    fn is_shearable(
        #[engine] kind: &str,
        host: &dyn Host,
        entity: EntityRef,
        player: Option<PlayerRef>,
        shears: ItemStackView<'_>,
    ) -> Option<bool>;
}
