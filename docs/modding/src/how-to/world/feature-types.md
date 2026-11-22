# Feature types

Register a `FeatureType` in `Hooks::register` to place features named by a configured feature's
`"type"` field. For example, `data/mymod/worldgen/configured_feature/tall_pillar.json` holding
`{"type": "mymod:pillar", "height": 4}` uses your registered `mymod:pillar` implementation:

```rust,ignore
{{#include ../../../examples/src/worldgen.rs:feature}}
```

This is vanilla's feature-type registry (`minecraft:worldgen/feature_type`, NeoForge's
`FEATURE_TYPE`); a configured feature is an entry of `minecraft:worldgen/feature`.

Use an ID in your mod's namespace. These stop startup with a message naming your mod: a
duplicate ID, an ID in another mod's namespace (vanilla's `minecraft:` included), an empty name,
or an invalid config schema.

## Config

The second argument of `register_feature_type` is your config's schema: a flat record of
`DatapackField`s, the same one [datapack registries](../content-mods.md) use (`Bool`, `Int`,
`Float`, `Text`, `Identifier`, each with its bounds and an optional default). The game decodes
every configured feature of your type against it when the definition loads: an undeclared key, a
missing required key, a wrong type or a value out of bounds refuses that configured feature then,
naming it and the key, so a bad data pack never reaches generation. `type` is the game's own key.
`Reference` fields aren't accepted here; name another entry with `Identifier`.

`context.config()` is the decoded `DatapackEntry`: `config().get("height")` is a `DatapackValue`,
defaults filled in.

## Where your feature runs

A configured feature of your type places wherever the game places a feature, with no extra code:

| Route | What calls your `place` |
|---|---|
| Biome decoration | A placed feature in a biome's `features` list (or added by a biome modifier), on the worldgen workers, once per position its placement modifiers give |
| Nested features | A selector (`minecraft:random_selector`, `simple_random_selector`, `random_boolean_selector`) choosing it |
| Growth and bone meal | A sapling or grass whose configured feature it is |
| Commands | `/place feature mymod:tall_pillar` |
| Code | `LevelRef::place_feature(host, "mymod:tall_pillar", pos)` |

Vanilla's placement modifiers still choose the positions (`count`, `in_square`, `height_range`,
`biome` and the rest). Your feature runs at each position as soon as the last modifier gives it,
before the modifiers draw for the next position, in vanilla's depth-first order. A position given
twice is placed twice.

## Placing

`FeatureType::place(&self, context: &mut dyn FeatureContext, origin: [i32; 3])` returns:

- `Ok(true)` when the feature placed.
- `Ok(false)` when it didn't, for example a tree with no room. Blocks written before that are
  kept, as vanilla keeps them.
- `Err(FeatureError::new(cause))` for a failure. Pass the original error with
  `.map_err(FeatureError::new)?`, or use `std::io::Error::other(message)` for a plain message.
  `std::error::Error::source` exposes the original cause; `FeatureError` is not a `Wire` value.
  The game logs the registering mod, feature type, origin and cause chain on every `Err` and
  treats that invocation as `false`, without rolling back earlier writes or RNG draws and
  without disabling the type.

Writes go directly to the bounded generation region or live level with no staging buffer, work
quota or rollback. `context.block`, `context.block_in_tag` and `context.height` immediately see
blocks written earlier in the same call, and subsequent steps (built-in or modded) see them too.
`Ok(false)` keeps what you wrote, as vanilla keeps the blocks of a partly placed tree.

A **panic** in `place` disables only that registered callback for the session (in memory only,
never saved; a new load of the mod set enables it again), logs the registering mod loudly and
queues an error toast, without rolling back earlier writes or RNG draws. Later placements of a
disabled callback answer `false` without calling it again, while other registrations keep
running.

There is no `Host` in a feature, and a feature can't place another feature. Your code runs on the
thread its placement does: a worldgen worker for decoration (chunks generate on several at once),
the tick thread for growth, `/place feature` and `LevelRef::place_feature`. With no `Host` or
game state in reach, a worker never touches mod state through you. Your type must be
`Send + Sync`, stateless and deterministic: draw only from `context.random()`, the same stream
the modifiers before you drew from.

| `FeatureContext` access | Purpose |
|---|---|
| `config` | The configured feature's decoded settings |
| `random` | The caller's `RandomSource`: `next_int`, `next_int_bound`, `next_long`, `next_float`, `next_double`, `next_boolean`, `next_gaussian` |
| `block`, `block_in_tag` | Read current blocks and block tags directly from the world |
| `height` | Heightmap heights, seeing writes already made |
| `ensure_can_write` | Check `WorldGenRegion.ensureCanWrite` / live level write admissibility at a position |
| `set_block` | Write a `BlockState` directly with `BlockUpdateFlags`; `Feature.setBlock` uses `NEIGHBORS \| CLIENTS` |
| `block_entity` | Give the current block at a position its validated block-entity saved NBT |
| `state`, `state_with`, `is_air`, `metadata` | Resolve a registered `BlockKey` to a `BlockState`, and test states |
| `min_y`, `max_y` | The level's height bounds |

`set_block` delegates directly to the owning world and returns its actual result. During
FEATURES decoration, `WorldGenRegion.ensureCanWrite` allows the center chunk and its eight
neighbors; if the center is upgrading, its generation height also restricts writes anywhere in
that region, regardless of the target chunk. A write outside the admissible region returns
`false` as an ordinary operation result and does not roll back earlier writes or disable the
type. A read the level can't answer is `None`. Neither reads nor writes ever load a chunk.
`block_entity` validates the decoded NBT against the current block at `pos` once (reporting bad
data if it does not fit) and writes immediately when `pos` is admissible, inside build height
and not in a read-only full chunk.

Resolve block states once per placement, or keep the `BlockKey`s from registration, rather than
resolving names for every block.

## Choosing a tree decorator's ground

`alter_ground` (NeoForge's `AlterGroundEvent`) lets a mod replace the ground provider of an
`AlterGroundDecorator`, such as the one that puts podzol under a spruce. To change only some
placements, wrap the `event.provider` you found and delegate the other positions to it with
`wrapped.state_at(level, pos)`.

A `GroundStateProvider::state_at` returns `Option<BlockState>`. `GroundLevel::block_state(pos)`
reads the region's canonical `BlockState`, and `GroundLevel::decorator_state(pos)` asks the
decorator's own provider for an `Option<BlockState>`. Compare or return those values directly;
for a fixed replacement, capture a state resolved through `host.block_state_id(name)` during
setup. There is no game `Host` in the provider. `None` tries the next position down; a state the
placing registry does not have stops the feature. Use `level.random()` for any draws so they stay
in order with the decorator's own.
