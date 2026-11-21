//! Queue chunk work on the owning server level, without changing which level is ticking.
use super::Game;
use mod_api::{ChunkRef, ChunkRefusal, LevelRef};
use sunrise_client::world::World;

impl Game<'_> {
    /// Resolve the chunk source without swapping the level or opening any chunks.
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
        let world = self.chunk_operation_world(chunk.level())?;
        let [x, z] = chunk.pos();
        if !world.chunks.contains_key(&(x, z)) {
            return Err(ChunkRefusal::NotLoaded);
        }
        world.chunk_resends.insert((x, z));
        Ok(())
    }
}
