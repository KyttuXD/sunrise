//! The `level_tickets` area: what keeps chunks and watered areas active: forced chunks and tickets. Host method declarations; access defaults and game forwarders are generated.
#![allow(unused_variables)]
use crate::*;
/// The handles' reads of the `level_tickets` area, implemented by the game and reached through [`Backend`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait LevelTicketsBackend {
    /// Side: server.
    /// Whether any watered-area ticket covers this block
    /// (`FarmlandWaterManager.hasBlockWaterTicket`). Minimum bounds are inclusive and
    /// maximum bounds exclusive on all three axes. Reads active and parked levels, loads no
    /// chunks, and calls no mod predicates. False on clients or for a stale/unavailable level.
    #[access(WorldOnly)]
    fn level_has_water_ticket(&self, level: LevelRef, pos: [i32; 3]) -> bool;
    /// Side: server.
    /// Whether `/forceload` holds the chunk (`ServerLevel.getForcedChunks`),
    /// loaded or not; a mod's own ticket on it does not count. None for a stale handle or on a client.
    #[access(WorldOnly)]
    fn chunk_forced(&self, chunk: ChunkRef) -> Option<bool>;
    /// Side: server.
    /// The level's force-loaded chunks (`/forceload query`), sorted as
    /// the command lists them. None for a stale handle or on a client.
    #[access(WorldOnly)]
    fn level_forced_chunks(&self, level: LevelRef) -> Option<Vec<[i32; 2]>>;
    /// Side: server.
    /// The chunk tickets `controller` holds in the level, as (owner, chunk
    /// position, natural_spawning), in order. Uses `controller`'s registered
    /// identity rather than the current caller. None for an invalid controller
    /// handle (such as one from a refused registration), a stale level handle,
    /// or on a client.
    #[access(WorldOnly)]
    fn level_mod_forced_chunks(
        &self,
        level: LevelRef,
        controller: TicketController,
    ) -> Option<Vec<(TicketOwner, [i32; 2], bool)>>;
}
/// The handles' writes of the `level_tickets` area, implemented by the game and reached through [`BackendMut`].
// hidden: a handle's backend, reached through `Backend`; mods call the handle, never this trait
#[doc(hidden)]
#[sunrise_mod_api_macros::host_area]
pub trait LevelTicketsBackendMut {
    /// Side: server.
    /// Mark the half-open box `min <= pos < max` as watered (`FarmlandWaterManager.addAABBTicket`).
    /// For 5x1x5 farmland starting at [0, 64, 0], use min [0, 64, 0], max [5, 65, 5].
    /// Farmland checks tickets before its vanilla water scan; rain and water still hydrate it.
    /// Any holder of the returned [`WaterTicket`] can remove it; overlaps are independent.
    /// Remove it when the sprinkler stops. Unloading the master chunk (the chunk nearest
    /// the box's center) invalidates the whole ticket, even in its other chunks. Unloading
    /// the level invalidates all its tickets. A ticket added by an enabled installed mod
    /// also expires when that mod is disabled. Never saved: re-add in `level_load` or a block
    /// entity's load listener. Reads/writes reach parked server levels.
    /// None on clients, for stale/unavailable levels, for empty or reversed bounds, or when
    /// called on behalf of a disabled installed mod.
    /// Queries test indexed tiles without loading chunks or calling mod predicates.
    #[access(WorldOnly)]
    fn level_add_water_ticket(
        &mut self,
        level: LevelRef,
        min: [i32; 3],
        max: [i32; 3],
    ) -> Option<WaterTicket>;
    /// Side: server.
    /// Remove this watered area by its handle identity (`SimpleTicket.invalidate`), leaving
    /// overlapping tickets intact. Any holder with the [`WaterTicket`] handle can remove it
    /// without caller attribution. False if already removed, stale, unloaded, or called on a
    /// client.
    #[access(WorldOnly)]
    fn water_ticket_remove(&mut self, water_ticket: WaterTicket) -> bool;
    /// Side: server.
    /// `/forceload add` or `remove` of one chunk (`ServerLevel.setChunkForced`):
    /// true when the forced set changed. A forced chunk is kept loaded and
    /// entity ticking, saved with the level, and the level keeps ticking
    /// without a player. This is the world's own `/forceload` state, not a
    /// ticket the mod owns: the chunk stays forced across restarts, and after
    /// the mod is unloaded or removed, until `set_forced(false)` or
    /// `/forceload remove` clears it. A successful add loads the chunk before
    /// returning. If streaming is paused, the ticket is recorded and takes
    /// effect when its level starts streaming. False for a stale handle, on a
    /// client, outside the world, or when adding after generation has stopped.
    #[access(WorldOnly)]
    fn chunk_set_forced(&mut self, chunk: ChunkRef, forced: bool) -> bool;
    /// Side: server.
    /// The chunk ticket of `controller` for `owner` on `chunk`
    /// (`TicketController.forceChunk`): true when the controller's tickets
    /// changed. Authorized by the registry-issued `controller` handle itself,
    /// without consulting the current caller.
    /// A chunk stays forced, entity ticking and saved, and its level keeps
    /// ticking, while any ticket or `/forceload` holds it. With
    /// `natural_spawning` the chunk joins the spawning chunks (inhabited
    /// time, thunder, a spawn attempt) while it lies in the spawning area of a
    /// player (`ChunkMap.collectSpawningChunks`); the local mob cap still lets
    /// mobs spawn only with a non-spectator within 128 blocks. One
    /// owner holds a plain and a natural-spawning ticket on a chunk apart. Tickets are
    /// scoped by (controller, owner, chunk), so two owners in one chunk, or
    /// two controllers, release separately. They are saved in
    /// `data/sunrise/mod_chunk_tickets.dat` under the controller's id; when
    /// the world loads, those of a controller no loaded mod registers are
    /// discarded. False for an invalid controller handle (such as one from a
    /// refused registration), on a client, for a chunk outside the world and
    /// for a level the server has not opened.
    #[access(WorldOnly)]
    fn chunk_set_mod_forced(
        &mut self,
        chunk: ChunkRef,
        controller: TicketController,
        owner: TicketOwner,
        forced: bool,
        natural_spawning: bool,
    ) -> bool;
}
