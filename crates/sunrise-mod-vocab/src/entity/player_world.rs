//! Block interactions a mod invokes as a player.

/// Where [`PlayerRef::destroy_block`](crate::PlayerRef::destroy_block) sends the broken block's
/// own item drops after `block_drops`. Experience and indirect drops still spawn in the world.
// no wire: drop routing is an in-process option for a player break.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockDropDestination {
    /// Spawn the drops at the block, as in a normal break.
    World,
    /// Put the drops in the player's inventory. Overflow drops from the player.
    Inventory,
}

/// Why `player.sees`, `player.ack_block_changes`, `player.show_block` or `player.place_block`
/// answered with nothing done: nothing was sent, written or queued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
#[derive(sunrise_mod_api_macros::Wire)]
pub enum PlayerWorldRefusal {
    /// A player handle (the viewer, the target or the player acted as) no longer identifies a
    /// player in the server's list: it is stale, or the player has left.
    Stale,
    /// The call was made on a client; these are the server's.
    Client,
    /// The player has no connection to send to (a fake player).
    NoConnection,
    /// The player is dead; the new player a respawn makes is the one to act as.
    Dead,
    /// `ack_block_changes` with a sequence below 0.
    NegativeSequence {
        /// The sequence given.
        sequence: i32,
    },
    /// `show_block` at a position the block update packet cannot carry: y outside
    /// -2048..=2047, x or z outside -33554432..33554432.
    PositionNotSendable {
        /// The position given.
        pos: [i32; 3],
    },
    /// The block state is not in the world's registry.
    UnknownState,
    /// `place_block`'s level handle is of an earlier open of the world.
    StaleLevel,
    /// `place_block` in a level that is not the one in place, where the player is.
    OtherLevel,
    /// `place_block` above or below the world or outside the 30,000,000 block limit.
    OutOfWorld {
        /// The position given.
        pos: [i32; 3],
    },
    /// `place_block` where the chunk is not loaded.
    NotLoaded {
        /// The position given.
        pos: [i32; 3],
    },
    /// `place_block` of the state already standing there: `setBlock` writes nothing.
    Unchanged,
    /// The origin or a cell the placement would write directly is held by a carrier transfer.
    Fenced,
    /// This placement completes a wither pattern and cannot run inside a Host transaction.
    InTransaction,
}

/// A wrong-phase call refuses as `Stale`: no player is in reach there.
impl crate::AccessRefusal for PlayerWorldRefusal {
    fn unavailable() -> Self {
        Self::Stale
    }
}

impl crate::NoBackend for PlayerWorldRefusal {
    fn no_backend() -> Self {
        Self::Stale
    }
}

impl From<crate::BlockStateError> for PlayerWorldRefusal {
    fn from(error: crate::BlockStateError) -> Self {
        match error {
            // The block state tables are not installed: no world is open.
            crate::BlockStateError::Unavailable => Self::Stale,
            _ => Self::UnknownState,
        }
    }
}
