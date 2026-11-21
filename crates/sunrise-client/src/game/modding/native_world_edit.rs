//! Bulk block edits for server mods: a helper on the level being ticked, as `native_set_block` is, that makes many
//! runs of blocks up columns at once, straight into the chunks.
use super::Game;
use mod_api::{ChunkRef, ChunkRefusal, LevelRef};
use sunrise_client::world::World;

impl Game<'_> {
    /// For a server mod: the runs made at once, straight into the chunks of the authoritative level `dimension`
    /// (the one being ticked, as for `native_set_block`), with what `fill ... strict` does for block entities (a
    /// block replaced by another loses its block entity, and a new block gets the one `native_set_block` would
    /// give it, a mod block's included); no neighbour is told unless `neighbour_updates`, and the chunks changed
    /// go to the players whole.
    /// Runs in chunks not loaded, and the parts of runs outside the column, are left out. The blocks changed, or
    /// why the edit was left out, with nothing changed: another dimension, a client of a server, a palette entry
    /// or filter that doesn't read or carries NBT (refused), a run naming a palette entry or filter past the end,
    /// or more than 65536 filters.
    pub(super) fn edit_blocks(
        &mut self,
        dimension: &str,
        edit: &mod_api::BlockEdit,
    ) -> Result<u64, String> {
        if !self.native_level_open(dimension) {
            return Err(format!("{dimension} is not the level being ticked"));
        }
        let started = std::time::Instant::now();
        let changed = self.apply_block_edit(edit)?;
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        if self.options.spikes.is_some_and(|threshold| ms > threshold) {
            log::info!(
                "Sunrise: slow mod block edit {ms:.1} ms ({} runs, {changed} blocks)",
                edit.runs.len()
            );
        }
        Ok(changed)
    }
    /// Resolve the server's resident chunk source without switching or opening a level.
    fn chunk_operation_world(&mut self, level: LevelRef) -> Result<&mut World, ChunkRefusal> {
        if !self.is_logical_server() {
            return Err(ChunkRefusal::Client);
        }
        if !self.level_handle_current(level) {
            return Err(ChunkRefusal::Stale);
        }
        if level.dimension() == self.dim.key() {
            return Ok(&mut self.world);
        }
        self.levels
            .parked_world_mut_of(level.dimension_id())
            .ok_or(ChunkRefusal::Stale)
    }

    pub(super) fn level_relight_chunks(
        &mut self,
        level: LevelRef,
        chunks: &[[i32; 2]],
    ) -> Result<usize, ChunkRefusal> {
        let world = self.chunk_operation_world(level)?;
        let before = world.dirty_light.len();
        for &[x, z] in chunks {
            if !world.chunks.contains_key(&(x, z)) {
                continue;
            }
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let key = (x.wrapping_add(dx), z.wrapping_add(dz));
                    if world.chunks.contains_key(&key) {
                        world.dirty_light.insert(key);
                    }
                }
            }
        }
        Ok(world.dirty_light.len() - before)
    }

    pub(super) fn chunk_resend(&mut self, chunk: ChunkRef) -> Result<(), ChunkRefusal> {
        let networked = self.lan.is_some();
        let world = self.chunk_operation_world(chunk.level())?;
        let [x, z] = chunk.pos();
        if !world.chunks.contains_key(&(x, z)) {
            return Err(ChunkRefusal::NotLoaded);
        }
        if networked {
            world.chunk_resends.insert((x, z));
        }
        Ok(())
    }
    /// Whether `dimension` is the authoritative level being ticked: where a server mod's world helpers act.
    pub(super) fn native_level_open(&self, dimension: &str) -> bool {
        (self.serving.is_some() || self.options.dedicated.is_some()) && dimension == self.dim.id()
    }
}
