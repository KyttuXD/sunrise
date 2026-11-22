# Item pickup notices

Listen to the server hook `item_picked_up_post` (NeoForge's `ItemEntityPickupEvent.Post`) to track
items a player receives from the ground. It fires once per pickup that put anything into the
inventory: when the whole stack went in, and when only part of it did and the rest stayed on the
ground, as when 8 apples lie there and only 5 fit.

It does not fire when `item_picked_up` refused the pickup or the inventory took nothing. A
`TriState::False` answer from `item_picked_up` vetoes only the current touch without changing the
item's pickup delay (matching NeoForge `ItemEntityPickupEvent.Pre`). A creative player always takes
the whole stack.

It carries the player's UUID, the item entity, `original`, a copy of the stack as it lay, and
`remaining`, the part of the captured `original` that did not fit (`None` if it all fit).
If Pre installed a replacement, `item.with_stack` reads that live stack instead; even a whole
pickup of the captured stack leaves the replacement on the ground. Changing `remaining`
overrides what stays on the ground, not what went into the inventory. Removing the entity in the
listener keeps it removed, for the host's player and LAN guests alike: there is no take animation,
and the stat and criterion still run.

For a quest counter or custom pickup sound, count the amount as it stood when the notice arrived:
after any listener edits `remaining`, `original` less `remaining` need not equal what the inventory
took.
