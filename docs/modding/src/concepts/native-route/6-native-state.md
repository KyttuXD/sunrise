# Native state: `[[native.state]]` is refused

Sunrise refuses `[[native.state]]` when it reads the package manifest: adding a field changes a game type's
layout, which must match the prebuilt game. If you declare this table, package readers, including the
native builder, report:

```text
E_MIXIN_STATE_LAYOUT: <mod>:<row>: native state changes the prebuilt game's layout; use an attachment instead
```

Remove the row. For data owned by an entity, block entity, chunk, level or player, register an
**attachment** with the matching holder. The game keeps the value on that object and saves it when you give
it a codec. See [server attachments](../attachments.md) and [querying attachment holders](../attachments/querying.md).

For a field owned by the whole `Game`, put the field on a [published loop](3-loops.md)'s slot type, initialize
it in `Default`, and access it on `self` in the slot callbacks. The slot lives once per `Game`, not once per
level. A `static` in your crate is shared by every `Game` in the process and outlives each world: key it by world
and clear it yourself.
