//! Feature types a mod registers: vanilla's `Feature` with its own `place`, as a configured
//! feature with `"type": "modid:tree"` names it. Handed over once at startup and looked up by
//! `FeatureDefinition::decode` before its built-in fallback, so a mod feature runs through every
//! route a built-in one does: biome decoration on the worldgen workers, nested features,
//! growth, bone meal, `/place feature` and `LevelRef::place_feature`. Runs on worker threads,
//! so a type keeps no mutable state and its target carries no `Host`.
//!
//! Placement lends the caller's bounded generation region or live level directly, with no
//! staging buffer, work quota or rollback. Writes reach the world immediately and are visible to
//! subsequent reads, block-tag queries and heightmaps. A region refusal is an operation result
//! (`false`), not poison. A returned `Err` logs the registering mod, type key, origin and cause
//! chain and reports `false` for that invocation without disabling the type; panic containment
//! belongs to the adapter's `CallbackOwner`.
//!
//! A mod feature runs on the thread its placement does: a worldgen worker for decoration, the
//! tick thread for growth, `/place feature` and `LevelRef::place_feature`. It sees only its
//! target, its config and the caller's random, with no `Host`, no `Game` and no hook dispatch,
//! so a worker never reaches mod state through it (#1532).
use crate::heightmap::HeightmapType;
use crate::registry::{BlockRegistry, BlockStateId};
use mod_api::FeatureError;
use std::any::Any;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use sunrise_core::nbt::Compound;
use sunrise_core::random::RandomSource;

/// A configured feature's config, as its type's adapter decoded it at load.
pub type ModFeatureConfig = Arc<dyn Any + Send + Sync>;

/// A feature type's engine side, as the executable's adapter sees it. `Send + Sync` because
/// neighbouring chunks decorate at once on worker threads; the same random gives the same
/// blocks.
pub trait ModFeatureType: Send + Sync {
    /// Decode a configured feature's JSON once at load (the settings sit inline next to
    /// `type`) against the type's declared schema; an error refuses the definition, naming it.
    fn prepare_config(&self, config: &serde_json::Value) -> Result<ModFeatureConfig, String>;
    /// `Feature.place`: true when the feature says it placed. An `Err` is the mod's failure.
    fn place(
        &self,
        target: &mut dyn ModFeatureTarget,
        random: &mut dyn ModFeatureRandom,
        origin: [i32; 3],
        // Decoded once and retained by the loaded definition.
        config: &(dyn Any + Send + Sync),
    ) -> Result<bool, FeatureError>;
}

/// The level a mod feature places in, object-safe: the caller's own world (a decoration
/// region, the live level), never a second block store. A read the world can't answer is
/// `None`; writes delegate immediately to the world and return its actual result. Neither loads
/// a chunk.
pub trait ModFeatureTarget {
    /// The numeric block registry the states are of, for the adapter's state index.
    fn registry(&self) -> Option<&'static BlockRegistry>;
    fn block(&mut self, position: [i32; 3]) -> Option<BlockStateId>;
    /// `WorldGenRegion.ensureCanWrite` / live level write admissibility at `position`.
    fn ensure_can_write(&mut self, position: [i32; 3]) -> bool;
    /// `LevelWriter.setBlock` with vanilla's update `flags`: delegates directly to the owning
    /// world and returns its result.
    fn set_block(&mut self, position: [i32; 3], state: BlockStateId, flags: i32) -> bool;
    /// Whether the block at `position` is in block tag `tag`.
    fn block_in_tag(&mut self, position: [i32; 3], tag: &str) -> Option<bool>;
    fn min_y(&mut self) -> Option<i32>;
    fn max_y(&mut self) -> Option<i32>;
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32>;
    /// Store validated block-entity data immediately at `position` when `position` is admissible
    /// and not in a read-only full chunk.
    fn set_block_entity(&mut self, position: [i32; 3], nbt: Compound) -> bool;
}

/// The caller's random, object-safe: the same stream the placement chain and the built-in
/// features draw from, in vanilla's order.
pub trait ModFeatureRandom {
    /// `nextInt(bound)`; 0 without a draw for a bound below 1.
    fn int(&mut self, bound: i32) -> i32;
    /// `nextInt()`: any `i32`.
    fn int_unbounded(&mut self) -> i32;
    fn long(&mut self) -> i64;
    fn float(&mut self) -> f32;
    fn double(&mut self) -> f64;
    fn boolean(&mut self) -> bool;
    /// `nextGaussian()`.
    fn gaussian(&mut self) -> f64;
}

/// Any `RandomSource` as a [`ModFeatureRandom`].
pub(crate) struct Draws<'a, R>(pub(crate) &'a mut R);
impl<R: RandomSource> ModFeatureRandom for Draws<'_, R> {
    fn int(&mut self, bound: i32) -> i32 {
        if bound < 1 {
            return 0;
        }
        self.0.next_int_bound(bound).unwrap_or(0)
    }
    fn int_unbounded(&mut self) -> i32 {
        self.0.next_int()
    }
    fn long(&mut self) -> i64 {
        self.0.next_long()
    }
    fn float(&mut self) -> f32 {
        self.0.next_float()
    }
    fn double(&mut self) -> f64 {
        self.0.next_double()
    }
    fn boolean(&mut self) -> bool {
        self.0.next_boolean()
    }
    fn gaussian(&mut self) -> f64 {
        self.0.next_gaussian()
    }
}

/// Stack-only borrow of the supplied placement world for direct feature writes.
struct DirectTarget<'a, W> {
    world: &'a mut W,
    mod_id: &'a str,
    feature_id: &'a str,
}

impl<W> ModFeatureTarget for DirectTarget<'_, W>
where
    W: super::feature::FeatureWorld + super::placement::PlacementContext,
{
    fn registry(&self) -> Option<&'static BlockRegistry> {
        self.world.block_registry()
    }
    fn block(&mut self, position: [i32; 3]) -> Option<BlockStateId> {
        self.world.block(position).ok()
    }
    fn ensure_can_write(&mut self, position: [i32; 3]) -> bool {
        match self.world.ensure_can_write(position) {
            Ok(allowed) => allowed,
            Err(error) => {
                let [x, y, z] = position;
                log::error!(
                    "mod {}: feature type {} failed to check write admissibility at [{x}, {y}, {z}]: {error}",
                    self.mod_id,
                    self.feature_id,
                );
                false
            }
        }
    }
    fn set_block(&mut self, position: [i32; 3], state: BlockStateId, flags: i32) -> bool {
        match self.world.set_block(position, state, flags) {
            Ok(written) => written,
            Err(error) => {
                let [x, y, z] = position;
                log::error!(
                    "mod {}: feature type {} failed to write block at [{x}, {y}, {z}]: {error:?}",
                    self.mod_id,
                    self.feature_id,
                );
                false
            }
        }
    }
    fn block_in_tag(&mut self, position: [i32; 3], tag: &str) -> Option<bool> {
        self.world.block_matches_tag(position, tag).ok()
    }
    fn min_y(&mut self) -> Option<i32> {
        self.world.min_y().ok()
    }
    fn max_y(&mut self) -> Option<i32> {
        self.world.max_y().ok()
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32> {
        self.world.height(kind, x, z).ok()
    }
    fn set_block_entity(&mut self, position: [i32; 3], nbt: Compound) -> bool {
        let [x, y, z] = position;
        match self.world.ensure_can_write(position) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                log::error!(
                    "mod {}: feature type {} failed to check write admissibility for block entity at [{x}, {y}, {z}]: {error}",
                    self.mod_id,
                    self.feature_id,
                );
                return false;
            }
        }
        match self.world.inside_build_height(position) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                log::error!(
                    "mod {}: feature type {} failed to check build height for block entity at [{x}, {y}, {z}]: {error}",
                    self.mod_id,
                    self.feature_id,
                );
                return false;
            }
        }
        match self.world.full_chunk_at(position) {
            Ok(false) => {}
            Ok(true) => return false,
            Err(error) => {
                log::error!(
                    "mod {}: feature type {} failed to check full chunk policy for block entity at [{x}, {y}, {z}]: {error}",
                    self.mod_id,
                    self.feature_id,
                );
                return false;
            }
        }
        match self.world.set_block_entity(position, nbt) {
            Ok(()) => true,
            Err(error) => {
                log::error!(
                    "mod {}: feature type {} failed to write block entity at [{x}, {y}, {z}]: {error}",
                    self.mod_id,
                    self.feature_id,
                );
                false
            }
        }
    }
}

/// A registered feature type and the mod that registered it.
pub struct ModFeatureEntry {
    pub mod_id: String,
    pub ty: Arc<dyn ModFeatureType>,
}
impl ModFeatureEntry {
    pub fn new(mod_id: impl Into<String>, ty: Arc<dyn ModFeatureType>) -> Self {
        Self {
            mod_id: mod_id.into(),
            ty,
        }
    }
}

/// A configured feature of a mod's type: the type and its config, decoded once at load.
#[derive(Clone)]
pub struct ModFeature {
    pub id: String,
    pub entry: Arc<ModFeatureEntry>,
    pub config: ModFeatureConfig,
}
impl std::fmt::Debug for ModFeature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModFeature")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl ModFeature {
    /// One direct call of the type at `origin` over `world`. Writes and RNG draws stay in place
    /// regardless of the return value; an `Err` logs the registering mod, type key, origin and
    /// full cause chain before answering `false`.
    pub(crate) fn place<W>(
        &self,
        world: &mut W,
        random: &mut impl RandomSource,
        origin: [i32; 3],
    ) -> bool
    where
        W: super::feature::FeatureWorld + super::placement::PlacementContext,
    {
        let mut target = DirectTarget {
            world,
            mod_id: &self.entry.mod_id,
            feature_id: &self.id,
        };
        let mut draws = Draws(random);
        match self
            .entry
            .ty
            .place(&mut target, &mut draws, origin, &*self.config)
        {
            Ok(placed) => placed,
            Err(error) => {
                let [x, y, z] = origin;
                let mut chain = error.to_string();
                let mut source =
                    std::error::Error::source(&error).and_then(std::error::Error::source);
                while let Some(cause) = source {
                    use std::fmt::Write;
                    let _ = write!(chain, ": {cause}");
                    source = cause.source();
                }
                log::error!(
                    "mod {}: feature type {} failed at [{x}, {y}, {z}]: {chain}",
                    self.entry.mod_id,
                    self.id,
                );
                false
            }
        }
    }
}

/// The installed types, `modid:tree` → entry carrying the registering mod id and type. Set once,
/// before any world opens (the executable hands the mods' registrations over), and read while
/// decoding features.
pub type ModFeatureTypes = BTreeMap<String, Arc<ModFeatureEntry>>;

static MOD_TYPES: OnceLock<ModFeatureTypes> = OnceLock::new();

#[cfg(test)]
thread_local! {
    /// The types a test installs, in place of the process-wide table.
    static TEST_TYPES: std::cell::Cell<Option<&'static ModFeatureTypes>> =
        const { std::cell::Cell::new(None) };
}

/// Install the process-wide table once; a second installation keeps the first and hands its
/// own types back.
pub fn install_mod_feature_types(types: ModFeatureTypes) -> Result<(), ModFeatureTypes> {
    let mut offered = Some(types);
    MOD_TYPES.get_or_init(|| offered.take().unwrap_or_default());
    offered.map_or(Ok(()), Err)
}

/// The installed feature type `id`: none until the executable installs the mods', and a test's
/// own while one is installed.
pub fn mod_feature_type(id: &str) -> Option<&'static Arc<ModFeatureEntry>> {
    #[cfg(test)]
    if let Some(types) = TEST_TYPES.get() {
        return types.get(id);
    }
    MOD_TYPES.get()?.get(id)
}

#[cfg(test)]
/// Runs `with` over a test's own feature types, as decoding reads them.
pub fn with_test_feature_types<T>(types: &ModFeatureTypes, with: impl FnOnce() -> T) -> T {
    let leaked: &'static ModFeatureTypes = Box::leak(Box::new(types.clone()));
    let prev = TEST_TYPES.replace(Some(leaked));
    let out = with();
    TEST_TYPES.set(prev);
    out
}

#[cfg(test)]
#[path = "mod_features/tests.rs"]
mod tests;
