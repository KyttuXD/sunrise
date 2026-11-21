# Dropped items during block-entity ticks

Use the tick scope's Host to read or write other objects. For a dropped item's
`ItemEntityRef`, call `item.set_stack(host, Some(stack))` to replace its stack. Pass `None` to
empty it; the entity remains available until its next normal tick discards it,
if it is still empty. `EntityRef::discard` or `EntityRef::remove` removes the
item directly. A removed item stops resolving immediately. A numeric entity ID
never retargets a replacement or an item in another level.

Use `item.with_stack(host, |stack| stack.count())` to inspect a dropped item's stack without
copying it. For components or an owned snapshot, use
`item.with_stack(host, |stack| stack.project()).flatten()`.

If both the item and the ticking block entity must change or neither may
change, make both edits in `event.subject.transaction`. Take the Host for the
item write, then re-take the subject view. Return `Err` if the item operation
fails or the subject is unavailable; do not continue on a replacement. Item
stack writes and removals use the ordinary journal, and abort restores them
along with the subject's storage. Commit delivers notices for explicit
removals; an empty stack set directly stays until the item's tick. With no
transaction, the stack write is immediate; there is no final write-back.

Pickup delay is a signed `i32`: `0` allows pickup, positive values tick down
except for `32767`, and negative values stay as set. The save format narrows
both pickup delay and age to signed shorts, as vanilla does. Pickup delay and
target changes, spawning, motion and non-item removals are not covered by the
dropped-item journal. The transaction is not a whole-world snapshot.
