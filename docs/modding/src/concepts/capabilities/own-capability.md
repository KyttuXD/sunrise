# A mod's own capability

You define your own capability, as Mekanism defines chemicals: a resource type and a key, published from
its root crate ([Dependencies between mods](../dependencies.md)) for other mods to use.

```rust,ignore
// The chemistry mod's root crate
#[derive(Clone, PartialEq)]
pub struct Chemical(pub String);
pub const CHEMICALS: CapabilityKey<dyn ResourceHandler<Chemical>> = CapabilityKey::new("chem_api:chemical");
```

Declare the defining mod under `[dependencies]` to build against the same `Chemical` and `CHEMICALS`. Your providers,
`r.block_entity_capability(&chem_api::CHEMICALS, &vat, …)` (where `vat` is a declared type key or
`r.block_entity_type_key("chem_pipe:vat")`), answer the same capability, and
`level.capability(host, &chem_api::CHEMICALS, pos, side)` finds either mod's handler. `resource_util::move_resources` moves a
chemical as it moves items, in the caller's transaction. The game has no answer of its own for a mod's key, so only
the mods' providers do, and a lookup with none registered costs one table check. A handler that keeps its amount in
its block entity's saved data (`BlockEntityRef::merge_data`) is covered by the game's journal, so an aborted
transaction puts it back. `ItemCapabilityKey::new` and `EntityCapabilityKey::new` define item and entity
capabilities the same way.

A key is known by its name, which must be namespaced (`chem_api:chemical`), and a name has one handler type. Two
mods that each declare their own `Chemical` under one name, rather than sharing the defining mod's, would never see
each other's handlers. So the game refuses that at startup, naming both mods and both types, and it refuses a key whose name has no
namespace. Both checks run once, when the mods register.

Storage for a holder without a block entity (an entity, a player, a chunk, the level) comes stock, kept in an
attachment the mod declares, as NeoForge mods keep an `ItemStacksResourceHandler` or `SimpleEnergyHandler` in one:
`AttachedItems::new(&SLOTS, holder, size, capacity, valid)` and `AttachedFluids::new` return a handler directly.
Both are `AttachedStacks<R>`, a `ResourceHandler<R>` over an `AttachmentType<StackList<R>>` (`single` for one
index). `StackList<R>` maps occupied indices to `(resource, amount)`; use `StackList::new` for an empty attachment
default. The handler's `size` defines its index range without allocating empty slots. Bulk extraction and the
resource transfer helpers visit occupied source indices in order, skipping empty gaps. Bulk insertion skips a
range that rejects the resource or cannot store the write. `capacity` stays the physical limit of an in-range
index, and `valid` is independent of capacity. A write through a smaller view preserves any stored
indices outside that view. A mod's own resource type implements `StackedResource` to use the same handler.
`AttachedEnergy::new(&ENERGY, holder, capacity, max_insert, max_extract)` works over an `AttachmentType<u32>`.

Give an item or fluid attachment `ITEM_STACKS` or `FLUID_STACKS` to save it. The stock codecs save only occupied
indices, and their NBT `Int` fields must represent each saved index and amount. Every write goes through the
attachment store, so the transaction journal undoes it on abort. Sync follows the attachment type's `sync` flag
and the connection's payload limits.
