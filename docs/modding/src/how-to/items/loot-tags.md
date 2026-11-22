# Reading tags in custom loot evaluation

Bound `ModLootFunction` and `ModLootCondition` rules read
`LootTypeContext::tags() -> &dyn Tags` alongside their borrowed `LootParamsView`
and, for functions, scoped `&mut LootStackEditor<'_>`. This is a read-only view of
the merged tags used by the current loot evaluator. The context supplies only its
borrowed parameters, tags and selected random.

Resolve an `Identifier` with `context.tags().key(&id)`, retain the resulting `TagKey`,
and call `is_in_tag(item, tag)` for item membership:

```rust,ignore
fn test(&self, context: &mut dyn mod_api::LootTypeContext) -> Result<bool, String> {
    let Some(tool_item) = context.params().tool().and_then(|tool| tool.item()) else {
        return Ok(false);
    };
    let Some(tag) = context.tags().key(&self.tag_id) else {
        return Ok(false);
    };
    Ok(context.tags().is_in_tag(tool_item, tag).unwrap_or(false))
}
```

The `items(tag)`, `blocks(tag)`
and `fluids(tag)` methods return `TagMembers::Missing` if a tag is absent,
`TagMembers::Members(keys)` for a loaded tag's ordered members (possibly empty),
or `TagMembers::Unavailable` if a required key mapping is not available. There is
no operation to enumerate every tag. A retained key keeps its identity across
reloads, but its members come from the current evaluation's tables.

Neither
callback gains a mutable game host or an opportunity to change tags. No tag
snapshot is copied when no codec with the evaluated type id is registered.
