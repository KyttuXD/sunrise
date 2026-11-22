//! The `level_chunks` area: chunks of an open level: handles, status, loading and resending. Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The handles' reads of the `level_chunks` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait LevelChunksBackend {
    /// Side: server.
    /// A handle for chunk `pos` of an open level, loaded or not, so a mod can
    /// force an unloaded chunk. None for a stale handle, on a client, and for
    /// a chunk outside the world (`ChunkPos` past 30 million blocks).
    fn level_chunk_ref(&self, level: LevelRef, pos: [i32; 2]) -> Option<ChunkRef>;
    /// Side: server.
    /// Whether this chunk is loaded; false on a client or for a stale handle.
    fn chunk_loaded(&self, chunk: ChunkRef) -> bool;
    /// Side: server.
    /// The status of a loaded chunk. Proto chunks are not exposed here.
    fn chunk_status(&self, chunk: ChunkRef) -> Option<ChunkStatus>;
    /// Side: server.
    /// Whether the chunk passes vanilla's seeded slime-chunk test, independent of
    /// dimension, height, biome and other spawn conditions. Works for unloaded chunks
    /// in active or parked open levels. None for a stale level, a client or a chunk
    /// outside world bounds. Does not load chunks or advance the world's random stream.
    fn chunk_is_slime_chunk(&self, chunk: ChunkRef) -> Option<bool>;
    /// Side: server.
    /// The sections of a loaded chunk whose block palette may hold any state of `block`
    /// (`PalettedContainer.maybeHas`), as section Y coordinates (`SectionPos.y`, block Y `>> 4`)
    /// from the bottom up, without reading the 4096 blocks of any section. "May": a palette
    /// keeps a state no block of the section uses any more, so read the blocks of each section
    /// named to find the ones that are there, and a section of more distinct states than a
    /// palette holds is named whenever the block exists. The way to find blocks that stand in a
    /// chunk, such as on `chunk_load`. None for a stale handle, on a client, and for a chunk
    /// that isn't loaded; it loads nothing.
    fn chunk_sections_with_block(&self, chunk: ChunkRef, block: BlockKey) -> Option<Vec<i32>>;
    /// Side: server.
    /// `ChunkMap.getPlayers(pos, false)`: the players watching this chunk, those
    /// it is inside the view of and was sent to, in the order the server keeps
    /// its players. A player watches it from its `chunk_watch` until it leaves
    /// the view. Fake players watch nothing. Loaded or not, a chunk nobody
    /// watches gives an empty list. None on a client and for a stale handle.
    fn chunk_watchers(&self, chunk: ChunkRef) -> Option<Vec<PlayerRef>>;
    /// Side: both.
    /// Whether `pos` is inside the world's height and its chunk is loaded
    /// (`Level.isLoaded`, which is false outside `isInValidBounds`): a block
    /// read or write there finds the world. False on a client or for a stale
    /// handle: the position is not loaded here. On a client, a chunk it holds is loaded
    /// (`ClientLevel.hasChunkAt`). API 381.
    fn level_is_loaded(&self, level: LevelRef, pos: [i32; 3]) -> bool;
    /// Side: both.
    /// Every loaded chunk of an open server level as handles, ordered by chunk x then z. None for
    /// a stale handle or on a client. Loads nothing.
    fn level_loaded_chunks(&self, level: LevelRef) -> Option<Vec<ChunkRef>>;
    /// Side: both.
    /// Visit each column's height by `heightmap` (`LevelReader.getHeight(type, x, z)`) in
    /// input order: one above the highest block that counts on that map, or `None` for a
    /// column whose chunk is not loaded. Never loads or generates chunks.
    ///
    /// Works on active and parked server levels and on the client's current open level.
    /// No column-count cap applies. The visitor can make shared reads on the same host.
    /// Return `ControlFlow::Continue(())` to keep visiting or `ControlFlow::Break(())` to
    /// stop before sampling later columns. Returns `Ok(ControlFlow::Break(()))` if stopped
    /// or `Ok(ControlFlow::Continue(()))` after every input column (including an empty slice).
    /// Returns `Err(LevelReadError::Stale)` for a stale handle or closed level, and
    /// `Err(LevelReadError::Unavailable)` when world reads are unavailable (such as outside
    /// an open world or on a foreign client dimension). Refusals do not call `visit`.
    fn level_visit_surface_heights(
        &self,
        level: LevelRef,
        columns: &[(i32, i32)],
        heightmap: Heightmap,
        visit: &mut dyn FnMut((i32, i32), Option<i32>) -> core::ops::ControlFlow<()>,
    ) -> Result<core::ops::ControlFlow<()>, LevelReadError>;
    /// Side: server.
    /// Visit each column's height on `heightmap`, server-side biome JSON water colour
    /// (`BiomeSpecialEffects.waterColor`, `0xRRGGBB`), and the complete [`BlockState`] values
    /// from `below` blocks under to `above` blocks from that height (bottom first; air outside
    /// the chunk's vertical range). Unloaded columns yield `None` without loading chunks.
    ///
    /// Colormap-derived grass and foliage tints are client-side only: read blended tints on
    /// the client with [`BlockAndTintGetter::block_tint`] in block tint callbacks or
    /// `ClientHost::biome_tint` in other client hooks.
    ///
    /// Works on active and parked server levels. `range` is `(below, above)`; `below + above`
    /// must be `1..=level_height` (the dimension's build height, `LevelHeightAccessor.getHeight`).
    /// No column-count or total-block cap applies: columns stream one at a time, and returning
    /// `ControlFlow::Break(())` stops before sampling later columns. The visitor can make
    /// shared reads on the same host.
    ///
    /// Returns `Ok(ControlFlow::Break(()))` if stopped or `Ok(ControlFlow::Continue(()))` after
    /// all input columns. Returns `Err(LevelReadError::Stale)` for a stale handle or closed
    /// level, and `Err(LevelReadError::Unavailable)` on a client, outside server world access,
    /// when `range` is empty or exceeds the level's build height, or when block-state
    /// conversion is unavailable. Refusals do not call `visit`.
    fn level_visit_columns(
        &self,
        level: LevelRef,
        columns: &[(i32, i32)],
        heightmap: Heightmap,
        range: (u32, u32),
        visit: &mut dyn for<'a> FnMut(
            (i32, i32),
            Option<ColumnSlice<'a>>,
        ) -> core::ops::ControlFlow<()>,
    ) -> Result<core::ops::ControlFlow<()>, LevelReadError>;
}
/// The handles' writes of the `level_chunks` area, implemented by the game and reached through [`BackendMut`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait LevelChunksBackendMut {
    /// Side: server.
    /// Sends the loaded chunk whole again, with its light, block entities and biomes as they are
    /// at the end of the tick, to every player watching it (`chunk_watchers`), as a chunk
    /// `edit_blocks` changed is sent: one packet instead of a block update per changed block.
    /// Ok with nothing sent when no player is in the level. Each refusal queues nothing and is
    /// named by `ChunkRefusal`: `Stale`, `Client`, `NotInPlace` and `NotLoaded`.
    fn chunk_resend(&mut self, chunk: ChunkRef) -> Result<(), ChunkRefusal>;
}
