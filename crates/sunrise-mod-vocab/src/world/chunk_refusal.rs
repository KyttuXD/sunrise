//! Refusals when a mod relights or resends chunks of a server level.

// no wire: reports the outcome of an in-process server chunk operation.
/// Why `level.relight_chunks` or `chunk.resend` was refused before queuing any work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ChunkRefusal {
    /// The level or chunk handle is of an earlier open of the world, or the level is gone.
    Stale,
    /// The call was made on a client; chunks are the server's.
    Client,
    /// `chunk.resend` of a chunk that is not loaded.
    NotLoaded,
    /// World access is not available from this host or in this phase.
    Unavailable,
}

impl crate::AccessRefusal for ChunkRefusal {
    fn unavailable() -> Self {
        Self::Unavailable
    }
}

impl crate::NoBackend for ChunkRefusal {
    fn no_backend() -> Self {
        Self::Unavailable
    }
}
