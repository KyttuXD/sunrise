# Typed bulk block edits

Call `level.edit_blocks(host, &edit)` on an active or parked server `LevelRef`.
Use complete `BlockState` values in `BlockEdit.palette`. Construct them with
`BlockState::of(&host.block_states(), block, constraints)` (or change an existing state
with `with`). `filters` contains owned `StatePredicate` values. Use
`StatePredicate::block(block, constraints)` once the block registry has resolved. It checks
property names and values against the game's block definition, not a caller's metadata.
`StatePredicate::tag(tag, constraints)` retains arbitrary property requirements, including
for an empty tag. Both constructors reject duplicate properties.

A predicate requires block identity or tag membership and every supplied property.
Omitted properties are wildcards, never default-state requirements. An unknown or
missing property on a tag member never matches. Sparse, non-Cartesian state tables
are supported: a partial predicate does not need to form a complete state with defaults.

Construct predicates once and pass them directly. To inspect a predicate, use
`block_key()` or `tag_key()` and borrow `constraints()`. Predicates and edits are
native values, not Wire payloads.

Before any mutation, the game resolves palette states and filter names and checks
run indices; a refusal returns `BlockEditError` with nothing changed. `BlockRun.only` can refer to any existing filter at that index. NBT
is not representable. Unloaded chunks and out-of-height portions of runs are skipped.
Edits preserve strict block-entity behavior, drop no contents from replaced block
entities, notify no neighbors and send whole changed chunks.
Inside a transaction in the edited level, the new states and entities are visible immediately.
Abort restores the original states, resident block entities and connection groups; only the
outermost commit in that level publishes the changed chunks. Without an open transaction in
the edited level, the edit commits immediately, even if another level has a transaction open.
When a bulk edit and a `LevelRef::set_block` write touch the same position, they replay in
call order. Only the ordinary write can spill contents or notify neighbors.

## Stream column heights and blocks

`LevelRef::visit_surface_heights(host, columns, heightmap, visit)` works on both sides
(active and parked server levels, and the client's open level). It streams each column's
loaded height (`Option<i32>`) in input order without loading chunks or enforcing a fixed
column-count cap, stopping early when `visit` returns `ControlFlow::Break(())`.

`LevelRef::visit_columns(host, columns, heightmap, (below, above), visit)` streams loaded
server columns one at a time as borrowed `ColumnSlice<'a>` views (or `None` where the chunk
is not loaded), with no column-count or total-block cap:

- `slice.base` is the column's height on `heightmap`.
- `slice.blocks` holds the complete `BlockState` values from `base - below` up to
  `base + above - 1`, bottom first (air outside the chunk's vertical range). `below + above`
  must lie in `1..=level_height` (`LevelHeightAccessor.getHeight`).
- `slice.water_color` carries the `0xRRGGBB` server-side biome JSON water colour
  (`BiomeSpecialEffects.waterColor`) at the column's top (`base - 1`). Colormap-derived
  grass and foliage tints are client-side only. See
  [Read blended biome tints on the client](read-biome-data.md#read-blended-biome-tints-on-the-client)
  for `ClientHost::biome_tint(pos, resolver)` and the equivalent read inside a block tint callback.

## Query terrain before loading chunks

`LevelRef::visit_terrain_surface(host, columns, visit)` reads an open server level's
generator (active or parked), not resident chunks. It streams one `Option<TerrainColumn>`
per input pair `(x, z)` in input order with no column-count cap, and stops before sampling
later columns when `visit` returns `ControlFlow::Break(())`.

- `height` is one above the highest solid block, skipping fluids, like OceanFloor. The scan
  starts at the level top, so the Nether returns its bedrock roof, not a floor.
- `block` is that surface's `BlockKey`; `water` is one above the highest fluid over it,
  or absent on dry land. `biome` names the biome in the world's table, including data pack biomes.
- Void columns and worlds without their own generator yield `None`. Custom generators that
  do not provide a noise surface also have no answer.
- No chunk is created, loaded or held. Features (trees, ores, lakes and decorations) and slope
  rules are not included: even an undecorated chunk may have a different top block. A teleport
  mod can sample candidate columns, then hold only its chosen chunk and verify the decorated blocks.
- Stale handles or closed levels return `Err(LevelReadError::Stale)` and clients or
  unavailable hosts return `Err(LevelReadError::Unavailable)` before visiting any column.
  Results depend on generator inputs, not loaded chunks or query order. The visitor can make
  shared reads on the same host. The game makes one noise sample per column on the server
  tick, and logs the visited column count when a call exceeds the configured slow-call threshold.

The non-exhaustive `TerrainColumn` is supplied by the game.

#### Upgrading saved block entities and entities

Data a mod reads from an older source (a schematic, a structure file, an old world's region) carries the
`DataVersion` it was saved at. `host.upgrade_block_entity_nbt(data_version, nbt)` and
`host.upgrade_entity_nbt(data_version, nbt)` run the game's own datafixes on one such value, the ones the chunk
loader runs on a block entity of a chunk of that version and the entity chunk loader on an entity (its passengers
included), up to the game's `DataVersion`. Both are server-side and pure: they read nothing from the level and change
nothing in it.

- Data at exactly the game's version comes back unchanged, and so does a block entity whose `id` its version
  doesn't know (as a chunk keeps one) and data the fixes can't read. Version 0 is a valid source.
- The fixes that need a whole chunk don't run: those that look at the blocks around a block entity, and the one that
  removes bed block entities.
- `Err` is an `NbtUpgradeError`. Depth counts as the data is encoded: the root
  compound is at 0, and the compound the NBT writer puts around each element of a mixed list is a level of its
  own, so that element sits two levels below its list.
  - Refused before any fix runs, on the data you gave: `NegativeVersion`; `FutureVersion`, a version newer than the
    game's (the fixes only go forward); `TooDeep`, a list or compound 512 levels deep or more, the depth cap of
    every NBT read.
  - Refused after the fixes, on the upgraded data: `UpgradedTooDeep`. The data you gave was within the cap, but a
    fix made it deeper (1.9's `Riding` to `Passengers` doubles a chain's depth) past what any NBT read, the game's
    own included, could take back. Nothing is returned, so split or trim the data at its source.
- There is no size limit and no node cap, as saved data has none: nothing a world the game accepts could carry is
  refused for its size.
