# Custom loot functions and conditions

Register a map codec once in the static, server-only `LOOT_FUNCTION_TYPE` or
`LOOT_CONDITION_TYPE` registry. These are the NeoForge 26.3 registries; their
entries are codecs, not separate legacy type wrappers:

```rust,ignore
fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
    r.register_loot_function_type(&Identifier::parse("example:scale").unwrap(), Scale);
    r.register_loot_condition_type(&Identifier::parse("example:enabled").unwrap(), Enabled);
}
```

Ids belong to the declaring manifest's namespace. Duplicates refuse registration,
naming both owners; `minecraft:` and `neoforge:` are reserved. A built-in always
resolves to the engine's built-in, never to a mod codec.

## Entry data and load-time binding

The data form is an object with `type` and the codec's own fields side by side:

```json
{"type":"example:scale", "factor":3}
```

Register a load-time codec implementing `ModLootFunctionCodec` or `ModLootConditionCodec`.
The function codec's `decode(&serde_json::Value)` returns `Result<Arc<dyn ModLootFunction>, String>`;
the condition codec returns `Result<Arc<dyn ModLootCondition>, String>`. Each decoder runs once
per resource entry when data loads (including tables edited by `loot_table_load`,
named predicates and item modifiers, enchantment `equipment_drops` requirements, global loot
modifier conditions, and inline `/loot` tables prepared at the command boundary), producing an
immutable bound rule. Unknown fields are retained for the codec, not rejected by the engine.
Declare parameters read by a bound rule with `referenced_context_params`; a table whose context
set does not allow one logs a warning but still loads. At runtime an absent parameter remains
absent. Named references reuse the same bound rule in every context set that references them.

```rust,ignore
use std::sync::Arc;
use mod_api::{
    Identifier, LootStackEditor, LootTypeContext, ModLootCondition, ModLootConditionCodec,
    ModLootFunction, ModLootFunctionCodec,
};

struct ScaleCodec;

struct ScaleRule {
    factor: i32,
}

impl ModLootFunctionCodec for ScaleCodec {
    fn decode(&self, data: &serde_json::Value) -> Result<Arc<dyn ModLootFunction>, String> {
        let factor = data
            .get("factor")
            .and_then(|v| v.as_i64())
            .and_then(|v| i32::try_from(v).ok())
            .ok_or_else(|| "missing integer factor".to_string())?;
        Ok(Arc::new(ScaleRule { factor }))
    }
}

impl ModLootFunction for ScaleRule {
    fn referenced_context_params(&self) -> Vec<Identifier> {
        vec![Identifier::parse("minecraft:tool").unwrap()]
    }

    fn apply(
        &self,
        stack: &mut LootStackEditor<'_>,
        context: &mut dyn LootTypeContext,
    ) -> Result<(), String> {
        // Read borrowed parameters first; release that borrow before drawing random.
        let has_tool = context.params().tool().is_some();
        let bonus = if has_tool { context.random().next_int_bound(2) } else { 0 };
        let count = stack.view().count().saturating_mul(self.factor).saturating_add(bonus);
        // Edit the live candidate in place; call `stack.to_owned()?` + `stack.replace(owned)?`
        // only when you need an owned `LootStack`.
        stack.set_count(count);
        Ok(())
    }
}

struct EnabledCodec;
struct EnabledRule { enabled: bool }

impl ModLootConditionCodec for EnabledCodec {
    fn decode(&self, data: &serde_json::Value) -> Result<Arc<dyn ModLootCondition>, String> {
        let enabled = data
            .get("enabled")
            .and_then(|v| v.as_bool())
            .ok_or_else(|| "missing boolean enabled".to_string())?;
        Ok(Arc::new(EnabledRule { enabled }))
    }
}

impl ModLootCondition for EnabledRule {
    fn test(&self, _context: &mut dyn LootTypeContext) -> Result<bool, String> {
        Ok(self.enabled)
    }
}
```

Unknown loot types, malformed registered entries, failed codec decoders, and
predicate/item-modifier reference cycles fail the candidate preparation, with a
file/id, entry path and type in the diagnostic.
Refusing a candidate leaves previously retained snapshots unchanged. Malformed
winning pack JSON never falls back to a lower-priority file. Predicate and item
modifier references, including references selected by tags, bind at load.
Referenced tables retain the caller's context set during parameter validation;
recursive table references stop traversal rather than recursing indefinitely.
Built-in functions and conditions use the same preparation boundary. Declarative
codec shapes check their required fields, optional values, enums, numeric
providers, component payloads and nested entries. The same shapes identify the
owned child functions and conditions for reference validation and tag expansion;
the engine never searches through a registered codec's opaque payload.

Optional `null` fields behave like omitted fields, unknown fields remain ignored,
and supported compact forms (including singleton holder sets, references and
compound SNBT strings) stay accepted. Generic registry IDs are checked for syntax,
not arbitrary registry membership; codecs with a closed registry still enforce it.
Primitive integer fields accept numeric JSON tokens such as `2.0` and `1e1` and
use signed Java-style narrowing rather than defaulting those tokens to zero.
Extreme numeric literals can still lose precision in the JSON representation
([#2105](https://github.com/sunrise-org/sunrise/issues/2105)). Text shape checks do
not yet replace complete selector, coordinate and canonical text-size validation
([#2112](https://github.com/sunrise-org/sunrise/issues/2112)).

## Applying and testing

A bound `ModLootFunction`'s `apply` receives a scoped `&mut LootStackEditor<'_>` over
the active candidate and a scoped `&mut dyn LootTypeContext`, returning `Result<(), String>`.
A bound `ModLootCondition`'s `test` receives that context, returning `Result<bool, String>`.
Neither re-parses JSON during a roll. The result allows a refused resource call to be
diagnosed, not mistaken for a successful edit. Each editor write validates its own input
before touching its target field; if a later write or the callback fails, earlier valid edits
and random draws remain in place, matching vanilla's in-place `LootItemFunction.apply`. A
condition call failure is diagnosed and does not pass. Ordinary callback refusals are logged
with the type id, bound resource location, owning mod and reason; panicking callbacks poison
that registration's owner across reloads.

The context lends `LootParamsView` through `params()`, authoritative tags through
`tags()`, and the evaluator's actual selected random stream through `random()`.
See [Reading tags](loot-tags.md). Reading parameters consumes no random. Use the
roll's random directly, and end any parameter borrow before drawing. Bound rules are
shared immutable declarations and must not keep random state between calls. A block-interaction
roll can lend `interacting_entity()`, `block_state()` and its optional `tool()` alongside its
origin, level and block entity. An enchantment roll lends its active `enchantment_level()`.

Functions run entry first, then pool, then table. A `sequence` passes each
function's output to the next. Conditions under `all_of` and `any_of` run in
order and short-circuit, so skipped terms consume no random. A function codec's
`conditional()` defaults to true: the engine tests its optional `condition`
first and returns the original stack if false. A direct function codec can return
false from `conditional()` and own that field itself.

`LootStackEditor` lends the current candidate through `view() -> LootStackView<'_>`,
copies it into an owned `LootStack` on demand through `to_owned()`, and mutates fields
in place (`set_item`, `set_count`, `set_damage`, `set_map`, `set_ominous_amplifier`,
`set_potion`, `set_exploration`, enchantment, stew-effect, name and component-patch
helpers, or whole-candidate `replace`). Signed intermediate counts, damage, enchantment
levels, ominous amplifiers and stew durations are not clamped. Component patch writes
preserve stored order and removals (`remove_component`, `push_component`,
`push_component_patch`, `set_components`, `retain_components`), retaining exact unknown
NBT types and enforcing vanilla's `NbtAccounter.MAX_STACK_DEPTH` (512) container depth
limit. Names are validated through the API `Text` boundary on write and projected on read.
Empty and over-max intermediate counts remain intact until ordinary root splitting after
the table functions. `LootStack` and `ExplorationMap` are in-process owned snapshots (not
`Wire`). The items evaluator consumes stacks into its ordinary item representation and has
no structure-search capability.

`params.entity()`, `damage()`, `last_damage_player()`, `tool()`, `block()` and
`biome()` return only parameters the caller supplied. The place reads are
`origin()`, `block_entity()` and `level()`; the level view lends dimension, elapsed
game time, day time and weather. `luck()` is the value applied to the roll. For
example, `params.entity().and_then(|entity| entity.equipment("mainhand"))`
borrows that exact slot's signed `LootStackView`. Candidate components iterate
in patch order, including removals; text names are projected only if read.

`params.queried_table_id()` is the caller's first supplied id, or
`neoforge:unknown_loot_table` until the root and all nested tables finish.
The root then sets its id before global modifiers; nested tables never write it.
`params.queried_stack()` lends an `ItemStackView` when a queried inventory stack
is supplied. This is distinct from the tool. Neither view is serializable.
`neoforge:loot_table_id` inside a table normally matches the unknown id rather
than the root's name.

`neoforge:can_item_perform_ability` reads the tool and its plain, case-sensitive
`ability` string, such as `shears_dig`. Without a tool it is false. Unknown names
have no default ability. An item part's answer replaces the item-layer default.

Custom types are table-internal operations. Global loot modifiers, which operate
on the completed output list, and `loot_table_load`, which edits table data,
remain separate mechanisms. Use a custom type for logic not expressible by a
standard operation; `copy_components`, `copy_custom_data`, `copy_state` and
`match_block` are vanilla built-ins, not names for custom registration.

## Copying components

`minecraft:copy_components` reads `source` as `block_entity`, `tool`, or an
available entity target. The world evaluator accepts `this`, `attacker`,
`direct_attacker`, and `attacking_player`; the item evaluator accepts `this`,
`target_entity`, and `interacting_entity`. Each entity source is available only
when the caller supplied it.
The block source is its collected component map, including a mod block entity's
stored components, retained before removal; it is not restricted to particular
vanilla block classes. A tool includes its loaded prototype and live component
patch, with removals respected.

An optional `include` list selects component IDs; omitting it selects every
source component, while an empty list selects none. `exclude` is applied after
`include`, so exclusion always wins. Selected values replace their destination
components without removing unselected values. An absent source makes no edit.
This is the same operation in both loot evaluators.

Entity sources currently supply custom name when present and custom data
(including an empty compound when unset). Subtype components such as variants
and entity-type default components will arrive with the separate general entity
component-getter follow-up; the copy function does not manufacture those values.

## Copying custom data

`minecraft:copy_custom_data` reads saved NBT rather than a component map. Its
`source` may be an inline context target such as `"this"` or `"block_entity"`,
the equivalent `{"type":"minecraft:context","target":"this"}` object, or
`{"type":"minecraft:storage","source":"example:cache"}`. Context sources are
available only when the caller supplied that entity or block entity. An unset
storage entry is an empty compound, not an absent context source. Entity target
names have the same evaluator-specific availability as `copy_components` above.

Each entry in `ops` has a `source` NBT path, a `target` NBT path into the item's
custom-data compound, and an `op`:
- `replace` writes the last matching source value to the target path.
- `append` appends every source match to each matching list, creating missing
  lists. It does not append to byte, int or long arrays.
- `merge` recursively merges compound matches into each target compound,
  creating missing compounds and ignoring non-compound matches.

Operations run in order. Missing sources and unmatched source paths leave the
stack unchanged, and the custom-data component is written only if the result
changed. Other components are untouched. Literal NBT types, arrays and Java
UTF-16 payloads are retained. Both evaluators use the same path parser and merge
implementation. Like `set` and `insert`, append and merge refuse writes beyond
the existing 512-level NBT budget before creating any target parents.
