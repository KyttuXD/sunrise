//! Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The server host's methods of the `world_edit` area, implemented by the game and reached through [`ServerHost`].
#[sunrise_mod_api_macros::host_area]
pub trait WorldEditServerHost {
    /// Side: server.
    /// A block entity's data saved at `data_version` (a schematic's, a structure file's, an older world's), upgraded
    /// as the chunk loader upgrades a block entity of a chunk of that version: its type's datafixes up to the game's
    /// DataVersion. Data of exactly the game's version comes back unchanged, and so does a block entity whose `id`
    /// that version doesn't know, or data the fixes can't read. The chunk-wide fixes (blocks around it, bed block
    /// entities removed) need a chunk and don't run. Pure: nothing in the world changes. `Err` names the refusal.
    /// Before any fix runs: a negative version ([`NbtUpgradeError::NegativeVersion`]), one newer than the game's
    /// ([`NbtUpgradeError::FutureVersion`]), or data with a list or compound 512 levels deep or more, counted as
    /// encoded with the wrapper compound of each mixed-list element as a level ([`NbtUpgradeError::TooDeep`], the
    /// depth cap of every NBT read). After the fixes: upgraded data that deep ([`NbtUpgradeError::UpgradedTooDeep`]),
    /// as no NBT read could take it back. There is no size refusal.
    #[access(WorldOnly)]
    fn upgrade_block_entity_nbt(
        &self,
        data_version: i32,
        nbt: Compound,
    ) -> Result<Compound, NbtUpgradeError>;
    /// Side: server.
    /// An entity's data saved at `data_version`, upgraded as the entity chunk loader upgrades an entity of a chunk
    /// of that version: its type's datafixes, its passengers' included, up to the game's DataVersion. Data of exactly
    /// the game's version, or data the fixes can't read, comes back unchanged. Pure: nothing in the world changes.
    /// Refused by name as `upgrade_block_entity_nbt` refuses.
    #[access(WorldOnly)]
    fn upgrade_entity_nbt(
        &self,
        data_version: i32,
        nbt: Compound,
    ) -> Result<Compound, NbtUpgradeError>;
}

/// The handles' reads of the `world_edit` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait WorldEditBackend {
    /// Side: both.
    /// Visits each column's height by `heightmap` (`LevelReader.getHeight`), in an active or
    /// parked server level or the client's current level. Receives `(pos, height)` in input
    /// order with `None` for an unloaded column. Does not load chunks or buffer heights.
    /// Stops when `visit` returns [`core::ops::ControlFlow::Break`]. Refuses a stale or
    /// unavailable level before calling the visitor.
    #[access(WorldOnly)]
    fn level_visit_surface_heights(
        &self,
        level: LevelRef,
        columns: &[(i32, i32)],
        heightmap: Heightmap,
        visit: &mut dyn FnMut((i32, i32), Option<i32>) -> core::ops::ControlFlow<()>,
    ) -> Result<(), LevelReadError>;
    /// Side: server.
    /// Visits the level generator's undecorated surface before features, without creating,
    /// holding or loading chunks, for this server level (active or parked). Lends
    /// `((x, z), Option<TerrainColumnView<'_>>)` in input order, independent of loaded chunks
    /// and previous calls, stopping early on [`core::ops::ControlFlow::Break`]. Height counts
    /// from the level top: the Nether returns its roof, not a floor. The top block omits slope
    /// rules and may differ even from an undecorated chunk. Trees, ores and other features are
    /// absent. Void columns and worlds without their own generator lend `None`. Refuses before
    /// sampling on a stale handle or a client ([`LevelReadError`]).
    #[access(WorldOnly)]
    fn level_visit_terrain_surface(
        &self,
        level: LevelRef,
        columns: &[(i32, i32)],
        visit: &mut dyn for<'a> FnMut(
            (i32, i32),
            Option<TerrainColumnView<'a>>,
        ) -> core::ops::ControlFlow<()>,
    ) -> Result<(), LevelReadError>;
    /// Side: server.
    /// Visits complete block states, bottom first, from `below` blocks below `[x, z]`'s heightmap
    /// base through `above - 1` above it. A nonresident chunk uses zero as the base and yields
    /// air, as do positions outside the level's build height. An empty range visits nothing.
    /// The optional base and the server biome's water colour are returned even if the visitor
    /// stops early with [`core::ops::ControlFlow::Break`]. Works on active and parked server
    /// levels. Doesn't load chunks, allocate a block list, or visit another level.
    /// If a state can't be represented, returns [`ColumnReadError`] at that block; earlier
    /// visits are not undone.
    #[access(WorldOnly)]
    fn level_visit_column_blocks(
        &self,
        level: LevelRef,
        column: [i32; 2],
        heightmap: Heightmap,
        range: (u32, u32),
        visit: &mut dyn FnMut(BlockState) -> core::ops::ControlFlow<()>,
    ) -> Result<ColumnRead, ColumnReadError>;
}

/// The handles' writes of the `world_edit` area, implemented by the game and reached through [`BackendMut`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait WorldEditBackendMut {
    /// Side: server.
    /// Applies the runs of `edit` at once straight into the chunks of this server level (active
    /// or parked), with what `fill ... strict` does for block entities (a block replaced by
    /// another loses its block entity, and a new block gets the one `set_block` would give it, a
    /// mod block's included); no neighbour is told, and changed chunks go to players whole. Runs
    /// in unloaded chunks and parts of runs outside the column's height are skipped. Returns the
    /// number of blocks changed, or a typed [`BlockEditError`] with nothing changed.
    /// Active-level writes are visible to the transaction but saves, full chunk updates
    /// and relighting wait for the outermost commit. Parked levels require no open transaction.
    #[access(WorldOnly)]
    fn level_edit_blocks(
        &mut self,
        level: LevelRef,
        edit: &BlockEdit,
    ) -> Result<u64, BlockEditError>;
    /// Side: server.
    /// Reads structure template `id` (`mymod:hut`, found as `StructureTemplateManager.get` finds
    /// it: the world's saved structures, then the game's and the mods' own) as `/place template`
    /// would place it at `at` with `rotation` and `mirror` in this server level (active or
    /// parked), without placing anything. Lends each transformed block as a
    /// [`StructureTemplateBlockView`] in vanilla placement order (full blocks, the others, then
    /// those with a block entity, each by y, x, z), from the palette `/place template` picks at
    /// `at`, stopping early on [`core::ops::ControlFlow::Break`], and returns the rotated
    /// template size. No processor runs and template entities are omitted. Refuses before
    /// visiting with a typed [`StructureTemplateError`].
    /// API @API:structure_template@.
    #[access(WorldOnly)]
    fn level_visit_structure_template(
        &mut self,
        level: LevelRef,
        id: &str,
        at: [i32; 3],
        rotation: StructureRotation,
        mirror: StructureMirror,
        visit: &mut dyn for<'a> FnMut(StructureTemplateBlockView<'a>) -> core::ops::ControlFlow<()>,
    ) -> Result<[i32; 3], StructureTemplateError>;
    /// Side: server.
    /// Places structure template `source` at `at` in this server level (active or parked) the
    /// way `/place template` and a structure block's load place one, with `rotation`, `mirror`
    /// and `pivot`, the level position the turn and mirroring go around (`at`, which
    /// `/place template` uses, turns the template about its own origin). Blocks, block-entity
    /// data and the template's own entities go through vanilla placement, so states are mirrored
    /// then turned, block entities keep their data and get the loot seed a placement gives them,
    /// and entities are spawned as the game spawns them: no processor runs, nothing is `strict`,
    /// and neighbours of a replaced block are not told. A template read from structure NBT is
    /// read by the game's own loader within vanilla's limits (`NbtAccounter.unlimitedHeap`,
    /// gzipped or plain). Refuses with nothing placed via a typed [`StructureTemplateError`].
    /// API @API:structure_template@.
    #[access(WorldOnly)]
    fn level_place_template(
        &mut self,
        level: LevelRef,
        source: &StructureTemplateSource,
        at: [i32; 3],
        pivot: [i32; 3],
        rotation: StructureRotation,
        mirror: StructureMirror,
    ) -> Result<(), StructureTemplateError>;
}
