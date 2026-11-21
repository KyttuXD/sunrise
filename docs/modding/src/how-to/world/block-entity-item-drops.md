# Dropped items during block-entity ticks

Use the tick scope's Host to read or write other objects. A dropped item's
`ItemEntityRef::set_stack` replaces its complete nonempty stack;
`EntityRef::discard` or `EntityRef::remove` removes the item. A removed item
stops resolving immediately. A numeric entity ID never retargets a
replacement or an item in another level.

Use `item.with_stack(host, |stack| stack.count())` to inspect a dropped item's stack without
copying it. For components or an owned snapshot, use
`item.with_stack(host, |stack| stack.project()).flatten()`.

If both the item and the ticking block entity must change or neither may
change, make both edits in `event.subject.transaction`. Take the Host for the
item write, then re-take the subject view. Return `Err` if the item operation
fails or the subject is unavailable; do not continue on a replacement. Item
stack writes and removals use the ordinary journal, and abort restores them
along with the subject's storage. Commit delivers the dropped item's removal
notice. With no transaction, these writes are immediate; there is no tick
batch or final write-back.

Pickup delay and target changes, spawning, motion and non-item removals are
not covered by the dropped-item journal. The transaction is not a whole-world
snapshot.

## Expiry and removal

Implement `Hooks::item_expire(&self, host: &mut dyn ServerHost, event: &mut ItemExpireEvent<'_>)`
to observe or extend the life of a dropped item at its server-side expiry check.
Read the stack from `event.stack` or resolve the still-live item with `host.entity(event.entity)`.
Leave `event.extra_life` unchanged to observe without changing the outcome, or compose with earlier
listeners using `event.extra_life = event.extra_life.wrapping_add(ticks)`.

`extra_life` starts at zero for every check. The server adds it to the current lifespan using
signed 32-bit wrapping addition, then clamps the total lifespan to `0..=32766`, as in NeoForge's
`ItemEntity.tick` patch. The age does not reset. The item stays only if its age is below the new lifespan.

For actual removal, implement `Hooks::entity_leave_level` and filter for `event.kind == "minecraft:item"`.
The item is already gone in that callback, which also covers non-expiry removals such as pickup.
