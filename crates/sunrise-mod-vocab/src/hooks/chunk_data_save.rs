//! Chunk attachment snapshot encoding boundary.

pub trait Hooks {
    /// The chunk worker is about to encode the submitted save record.
    /// `data` belongs only to that record: typed edits change the saved data,
    /// never the live chunk. Exactly once per captured record, for full and proto
    /// chunks alike. The restricted Host cannot resolve live chunks or mutate the
    /// world; `host.job()` borrows the owning level's identity and the record's
    /// metadata. A failed job, including invalid or over-budget disk NBT, retains
    /// its snapshot and the prior disk record.
    #[hook(
        side = "server",
        contract = "listen",
        context = "chunk_disk",
        target = "mod_api::chunk_disk::native_chunk_data_save",
        neoforge = "ChunkDataEvent.Save",
        version = 2
    )]
    fn chunk_data_save(data: &mut ChunkDiskData);
}
