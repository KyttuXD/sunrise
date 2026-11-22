# Dropped items during block-entity ticks

Use the tick scope's Host to read or write other objects. A dropped item's
`ItemEntityRef::set_stack` takes `Option<ItemStack>` (`ItemEntity.setItem`):
`Some(stack)` writes a non-empty stack, while `None` (or an air stack) stores
an empty stack that normal `ItemEntity.tick` discards on its next tick.
`EntityRef::discard` or `EntityRef::remove` removes the item immediately. A
removed item stops resolving immediately. A numeric entity ID never retargets a
replacement or an item in another level.

Use `item.with_stack(host, |stack| stack.count())` to inspect a dropped item's stack without
copying it. For components or an owned snapshot, use
`item.with_stack(host, |stack| stack.project()).flatten()`.

If both the item and the ticking block entity must change or neither may
change, make both edits in `event.subject.transaction`. Take the Host for the
item write, then re-take the subject view. Return `Err` if the item operation
fails or the subject is unavailable; do not continue on a replacement. Item
stack writes and removals use the ordinary journal, and abort restores them
along with the subject's storage. Clearing the stack alone does not enqueue removal at
commit. A transfer that extracts the last item schedules discard at commit, as a hopper
discards after taking the last item. With no transaction, the stack write is immediate.

Pickup delay and target changes, spawning, motion and non-item removals are
not covered by the dropped-item journal. The transaction is not a whole-world
snapshot.
