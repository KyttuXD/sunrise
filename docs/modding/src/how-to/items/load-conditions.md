# Registered data load conditions

Implement `LoadCondition` as a handler that reads the condition's JSON, then
register it from
`Hooks::register_load_conditions` using its `RegistrationContext` and
`event.conditions.register(Identifier::parse("testmod:flag").unwrap(), Flag)`.
Registration happens once per mod load, with one handler per id and a read-only
`RegistrationContext`. Any mod may register an unused ID in any namespace:
mod B can register `a:x` while mod A registers `a:y`. Built-in IDs such as
`neoforge:always`, `neoforge:never` and `neoforge:mod_loaded` cannot be replaced.
A duplicate ID names both mods in the error. A completed listener's registration error
refuses admission even if the listener ignores it. A
[failed registration callback](../../concepts/load-phases.md#registration-and-setup) publishes
neither handlers nor preparation errors.
Later listeners can still register those IDs.

```rust,ignore
struct Flag;
impl mod_api::LoadCondition for Flag {
    fn test(&self, args: &mod_api::serde_json::Value)
        -> Result<bool, mod_api::ConditionError>
    {
        args["value"].as_bool().ok_or_else(|| mod_api::ConditionError {
            message: "value must be a boolean".into(),
        })
    }
}
```

A recipe can use the registered name alongside the built-in conditions:

```json
{
  "type": "minecraft:crafting_shapeless",
  "ingredients": ["minecraft:flint"],
  "result": {"id": "minecraft:stick"},
  "neoforge:conditions": [{"type": "testmod:flag", "value": true}]
}
```

The handler receives a borrowed raw JSON value: the condition's object without
`type`. It reads and validates its own arguments, retaining JSON booleans,
numbers, strings, lists and objects. Invalid mod input must return
`ConditionError`; handlers must not panic while reading it.
Startup and reload both test the selected file before making its entry visible. In a load, the same
file and condition payload can reuse its result; typed built-in reads of feature flags, registries or tags
also distinguish the context views. A string in a custom handler's arguments is not a registry or tag
read and cannot order registry candidates. Use `neoforge:registered` or `neoforge:tag_empty` in the
condition array for a [registry dependency](conditional-data.md#registry-conditions). A new load tests
the entry with its own context. Handlers must not mutate live data.

A handler panic becomes a load error naming the condition and its registering mod. It is logged and
queues an error toast. Only that handler is disabled for the rest of the session, including after a
reload. An evaluation that reaches the disabled handler reports an error without calling it. A returned
`ConditionError` or an ordinary phase refusal is also reported when encountered, but neither disables
the handler.

`true` loads the entry; `false` omits it, without exposing a lower pack's
version. The loader handles callback errors and unknown condition types as
[described in Conditional data](conditional-data.md#errors-by-loader).

Registering an ID works the same in every namespace. The evaluator decodes the
whole conditions array before testing it, including children of `not`, `and`
and `or`. Neither `neoforge:mod_loaded` nor an already true or false branch
can guard an unknown condition ID. Testing goes left to right and skips
handlers after a false condition in an array or `and`, or after a true
condition in `or`.

The public [NeoForge data load condition documentation](https://docs.neoforged.net/docs/resources/server/conditions/)
describes `ICondition` and the top-level `neoforge:conditions` array.
