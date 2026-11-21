//! Levels a mod opens while the server runs: what it asks for and why a request is refused.

/// A level a mod opens at run time. The seed is specific to this instance: it reaches world
/// generation, the level's randoms and the hashed seed sent to clients.
// no wire: runtime level requests are made in-process by server mods.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimeLevel {
    /// The default superflat layers (bedrock, two dirt, grass), over plains.
    Flat {
        /// A dimension type already known to the world.
        dimension_type: String,
        /// The instance's seed.
        seed: i64,
    },
    /// Nothing but air, over plains.
    Void {
        /// A dimension type already known to the world.
        dimension_type: String,
        /// The instance's seed.
        seed: i64,
    },
    /// Use an existing level stem's complete dimension type and generator configuration.
    Stem {
        /// The source level stem's registry id, not the new instance's id or the generator's id.
        stem: String,
        /// The instance's seed.
        seed: i64,
    },
}

impl RuntimeLevel {
    /// A level with the default superflat layers and the named dimension type.
    pub fn flat(dimension_type: impl Into<String>, seed: i64) -> Self {
        Self::Flat {
            dimension_type: dimension_type.into(),
            seed,
        }
    }

    /// An empty level with the named dimension type.
    pub fn void(dimension_type: impl Into<String>, seed: i64) -> Self {
        Self::Void {
            dimension_type: dimension_type.into(),
            seed,
        }
    }

    /// A new instance of a stem registered at startup (from a data pack or `register_dimension`).
    /// Its generator (vanilla noise or a registered chunk generator) receives the same settings
    /// and biome source as the registered level, but generates with `seed`.
    pub fn from_stem(stem: impl Into<String>, seed: i64) -> Self {
        Self::Stem {
            stem: stem.into(),
            seed,
        }
    }
}

/// Why `RuntimeLevels::open`, `unload` or `delete` refused. The game logs the mod that asked.
// no wire: runtime level refusals return directly to the server mod that made the request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuntimeLevelError {
    /// There is no server world, or the call was made outside a mod's code.
    Unavailable,
    /// The id is not `namespace:path` with a folder-safe namespace and path.
    InvalidId,
    /// The id is a vanilla level or a level registered at startup.
    ReservedId,
    /// The id is open, or a request to open it is waiting.
    AlreadyOpen,
    /// No runtime level of that id is open.
    NotOpen,
    /// The dimension type is not known to the world, or does not have a registered name.
    UnknownDimensionType,
    /// The requested level stem is not registered in this world.
    UnknownStem,
    /// The stem does not name a noise or registered chunk generator.
    UnknownGenerator,
    /// A player, or an arrival not yet landed, is in the level.
    PlayersInside,
    /// `delete` of a level that has levels below it: an id that continues it with `/`
    /// (`mymod:instance/ab` below `mymod:instance`) that is open, waiting, a level of the world or
    /// has a folder under it. Delete those first, or delete only the level without a level below.
    LevelsBelow,
    /// `open` of an id whose `delete` closed the level but could not remove all its files. A
    /// second `delete` of the id finishes the removal; only then can it be opened.
    Removing,
}
