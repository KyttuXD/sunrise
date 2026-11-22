# Item abilities

Ability names are plain, case-sensitive strings with no namespace. They are not registry identifiers.

An item's Part can change the answer to an ability question. The hook receives an [`ItemAbilityContext`] and a borrowed [`ItemStackView`]. Gameplay action sites lend a read-only [`Host`] (`context.host()`) and its merged [`Tags`] (`context.tags()`); `neoforge:can_item_perform_ability` loot predicates lend the evaluator's merged [`Tags`] and [`LootParamsView`] (`context.loot()`) with `context.host() == None`. Keep answers pure functions of the stack, ability and context so the client and server agree. Call `event.stack.project()` only when you need components or an owned copy.

A client without this mod predicts vanilla here; the server corrects it.

```rust,ignore
#[sunrise_part]
impl ItemPart for CarvingTool {
    fn can_perform_action(
        &self,
        context: &ItemAbilityContext<'_>,
        event: &mut CanPerformActionEvent<'_>,
    ) {
        // Gameplay action sites lend a read-only `Host`; loot predicates lend `LootParamsView`.
        let _in_loot_predicate = context.host().is_none() && context.loot().is_some();
        let _tags = context.tags();
        // Fast metadata reads avoid cloning components; call `event.stack.project()` only if needed.
        let _item = event.stack.item();
        if event.ability == "shears_carve" {
            event.can_perform = true;
        }
    }
}
```

Attach this Part to your item with the registrar's `item_part` method. This grants pumpkin carving only: `shears_harvest` is a separate ability for mob and dispenser shearing. `shield_block` permits blocking and uses the stack's blocking component, or the default component when absent. Leave unknown abilities unchanged.

The game asks these abilities at the following action sites:

| Ability | Consumers |
| --- | --- |
| `shears_carve` | Pumpkin carving. |
| `shears_harvest` | Shearing sheep, mooshrooms, snow golems and bogged; dispenser shearing; harvesting bee nest honeycombs. |
| `shield_block` | Raising a blocking item and blocking attacks. |

`event.can_perform` starts with vanilla's answer. Set it to `true` to grant an ability or `false` to deny it. The answer controls permission, not the action's world effects or item wear.
