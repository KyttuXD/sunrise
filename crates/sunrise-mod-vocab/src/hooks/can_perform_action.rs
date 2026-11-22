//! The `can_perform_action` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `IItemExtension.canPerformAction`: can `stack` perform the item
    /// ability `ability`, a plain, case-sensitive name such as `shield_block`,
    /// `shears_harvest` or `shears_carve` (a mod's own
    /// abilities also use plain names, such as `my_mod_chop`)? `can_perform` arrives with
    /// vanilla's answer for that ability: the item kind's (shears shear) or the
    /// stack's component (`blocks_attacks`, a shield's, blocks); set it either
    /// way and the stack grants or loses the ability. `stack` is a borrowed
    /// [`ItemStackView`] (call [`ItemStackView::project`] only when you need
    /// components or an owned copy). The `context` is an [`ItemAbilityContext`]:
    /// gameplay action sites lend a read-only [`Host`] (`context.host()`) and its
    /// merged tags (`context.tags()`); loot predicates (`neoforge:can_item_perform_ability`)
    /// lend the evaluator's merged tags and [`LootParamsView`] (`context.loot()`)
    /// with `context.host() == None`. Loot conditions use the shared item defaults;
    /// action sites retain their own vanilla answers until #3330. Asked where vanilla
    /// checks the item for an action: a raised item blocking attacks
    /// (`shield_block`: a granted stack blocks as a bare
    /// `blocks_attacks` component does, a denied one never blocks nor is raised);
    /// shearing a sheep, mooshroom, snow golem or bogged, a dispenser's shears
    /// and a bee nest's honeycombs (`shears_harvest`); carving a pumpkin
    /// (`shears_carve`); and loot `neoforge:can_item_perform_ability` predicates.
    /// It decides whether the stack may do the action, not what the action does to
    /// the world or the tool (`block_tool_modification` is the block's side of a
    /// tool action; item wear is its own). Only the `ItemPart`s attached to `stack`'s
    /// item are asked, on both sides, so the client's prediction agrees with the
    /// server: answer from the stack, `ability` and `context` alone, and fast. An
    /// item with no such part costs one table lookup and does not project the stack.
    /// A client without this mod predicts vanilla here; the server corrects it.
    #[hook(
        receiver = "associated",
        dispatch = "api_carrier",
        side = "both",
        contract = "listen",
        context = "item_ability",
        scope = "item",
        key = "stack",
        version = 2,
        neoforge = "IItemExtension.canPerformAction"
    )]
    fn can_perform_action(stack: ItemStackView<'_>, ability: &str, can_perform: &mut bool);
}
