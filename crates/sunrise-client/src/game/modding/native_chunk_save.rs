//! Server-level disk context and save admission for chunk callbacks.
use super::*;

impl Game<'_> {
    /// Bind the level's shared persistence owner before adopting its streamer.
    /// The level and its `Persistent` generator submit to this same `SaveDir`.
    pub(super) fn bind_chunk_disk_worker(&self, save: &SaveDir, dim: Dim) -> Result<()> {
        if self.options.execution_role.is_client()
            || (!mod_api::__private::listened(mod_api::HookId::ChunkDataLoad)
                && !mod_api::__private::listened(mod_api::HookId::ChunkDataSave))
        {
            return Ok(());
        }
        if let Some(world) = self.native_lifecycle.epoch() {
            let id = dim.id();
            let dimension = mod_api::Identifier::parse(id)
                .map_err(|error| format!("chunk disk context for {id}: {error}"))?;
            save.set_chunk_disk_context(
                world,
                dimension,
                Arc::new(self.loop_context.worldgen_context()),
            );
        }
        Ok(())
    }

    /// Chunks whose subscribed save was refused or failed become dirty again.
    /// Without a `chunk_data_save` listener this does nothing.
    pub(in crate::game) fn requeue_failed_chunk_saves(&mut self) {
        if mod_api::__private::listened(mod_api::HookId::ChunkDataSave) {
            for key in self.save.take_failed_chunk_saves() {
                self.world.modified.insert(key);
            }
        }
    }

    /// With a `chunk_data_save` listener, saves wait while an action savepoint
    /// is open, so tentative state is never captured: unloads, and the plan
    /// that causes them, are deferred until it closes (the design's "defer
    /// unload when the gate refuses"). Never defers without a listener.
    pub(super) fn defer_chunk_disk_save(&mut self) -> bool {
        if self.options.execution_role.is_client()
            || !mod_api::__private::listened(mod_api::HookId::ChunkDataSave)
        {
            return false;
        }
        self.requeue_failed_chunk_saves();
        Self::admit_entity_save().is_err()
    }

    /// `ChunkMap.saveChunksEagerly`: snapshot only while the tick and writer
    /// have room. NBT encoding, compression and durable writes stay on workers.
    pub(super) fn save_chunks_eagerly(&mut self) {
        // No world open (`world_root` is set by `finish_open_world`): the save
        // is `Game::new`'s placeholder under the working directory.
        if self.world.carrier_fences.is_broken()
            || self.remote.is_some()
            || self.world_root.as_os_str().is_empty()
            || self.agent.hold_server
            || self.clock.sprinting()
            || self
                .server_lists
                .as_ref()
                .is_some_and(|lists| !lists.lock().unwrap().autosave)
        {
            return;
        }
        if self.defer_chunk_disk_save() {
            return;
        }
        let deadline = self
            .server_tick_deadline
            .unwrap_or_else(|| Instant::now() + self.clock.tick_duration());
        let mut modified = std::mem::take(&mut self.world.modified);
        modified.save_eagerly(
            Instant::now(),
            || Instant::now() < deadline && self.save.active_chunk_writes() < 128,
            |key| self.world.chunks.contains_key(&key),
            |key| {
                // `saveChunkIfNeeded` saves only a LevelChunk, so a lit one; a
                // column still waiting for its light keeps its queue entry.
                let Some(chunk) = self.world.chunks.get(&key).filter(|c| c.lit) else {
                    return false;
                };
                let chunks = self.with_entities([chunk]);
                match self
                    .save
                    .save_later(&chunks, self.pack.assets.registry, world_biomes!(self))
                {
                    Ok(_) => true,
                    Err(e) => {
                        log::warn!("Sunrise: failed to save changed chunks: {e}");
                        false
                    }
                }
            },
        );
        self.world.modified = modified;
    }
}
