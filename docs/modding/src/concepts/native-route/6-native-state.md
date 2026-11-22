# Native state: `[[native.state]]` is refused

Sunrise refuses `[[native.state]]` when it reads the package manifest: adding a field changes a game type's
layout, which must match the prebuilt game. If you declare this table, package readers, including the
native builder, report:

```text
E_MIXIN_STATE_LAYOUT: <mod>:<row>: native state changes the prebuilt game's layout; use an attachment instead
```

Remove the row and use an **attachment**. An attachment is a typed value the game keeps for you on an
entity, block entity, chunk, level or player, and saves with it when you give it a codec. See [server attachments](../attachments.md) and
[querying attachment holders](../attachments/querying.md).

For data about the whole game, a [published loop](3-loops.md)'s slot value lives once per `Game`. A `static` in
your crate is shared by every `Game` in the process and outlives each world: key it by world and clear it yourself.
