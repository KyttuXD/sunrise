# Item pickup notices

Listen to the server hook `item_picked_up_post` (NeoForge's `ItemEntityPickupEvent.Post`) to track
items a player receives from the ground. It fires once per pickup that put anything into the
inventory: when the whole stack went in, and when only part of it did and the rest stayed on the
ground, as when 8 apples lie there and only 5 fit.

It does not fire when `item_picked_up` refused the pickup or the inventory took nothing. A
`TriState::False` answer from `item_picked_up` vetoes only the current touch without changing the
item's pickup delay (matching NeoForge `ItemEntityPickupEvent.Pre`). A creative player always takes
the whole stack. To impose an explicit cooldown, call `event.item.set_pickup_delay(host, ticks)`.
The delay and target are checked after Pre on both player paths. If Pre edits the ground stack with
`event.item.set_stack`, that edit does not change the stack already captured for this attempt.

It carries the player's UUID, the item entity, `original`, a copy of the stack as it lay, and
`remaining`, which starts as what did not fit from the captured stack: `None` after a whole pickup,
or the part that did not fit after a partial one. When the notice arrives, the amount the inventory
took is `original`'s count less `remaining`'s. If Pre replaced the ground stack, use
`event.item.with_stack(host, |stack| stack.project())` to inspect what is now on the ground. Set
`remaining` to edit the ground stack, or `None` to empty it; a direct `event.item.set_stack` in Post
writes the resident stack independently. If the stack was replaced and the captured stack was taken
in full, the pickup still discards the entity. Removing the entity in the listener keeps it removed,
for the host's player and LAN
guests alike: there is no take animation, the stat and criterion run.

For a quest counter or custom pickup sound, count the amount as it stood when the notice arrived:
after any listener edits `remaining`, `original` less `remaining` need not equal what the inventory
took.
