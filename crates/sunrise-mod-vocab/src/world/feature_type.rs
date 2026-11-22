//! Feature types a trait mod registers: vanilla's `Feature` with its own `place`, as
//! [`Registrar::register_feature_type`](crate::Registrar::register_feature_type) hands it over
//! once at startup (the `FEATURE_TYPE` registry; a
//! configured feature is an entry of `minecraft:worldgen/feature`). A configured feature with
//! `"type": "modid:tree"` then places by the mod's code, wherever the game places features:
//! biome decoration, a nested feature (a random selector's choice), a sapling's growth, bone
//! meal, `/place feature` and `LevelRef::place_feature`.
//!
//! There is no `Host` here: decoration runs on the worldgen worker threads, so the mod sees only
//! the small context below, deterministic for the random it is given. A type keeps no mutable
//! state, because two workers can decorate neighbouring chunks at once. Everything around the
//! feature stays the game's own: the placed feature's modifiers, the biome's steps and the
//! decoration seeds.
//!
//! A type declares its config as a flat record ([`DatapackField`]s, the schema datapack
//! registries use); the game decodes each configured feature against it when the definition
//! loads, so a config that doesn't fit is refused then, naming the feature, never during
//! generation.
//!
//! Feature types run on the worldgen workers.
use crate::world::datapack_registry::{DatapackEntry, DatapackField};
use crate::world::structure_type::HeightmapType;
use crate::{BlockKey, BlockMetadata, BlockState, BlockUpdateFlags, RandomSource};
use std::sync::Arc;
use sunrise_core::nbt::Compound;

/// A feature's placement failed. The game logs the registering mod, feature type, origin and
/// error cause chain on every `Err` and treats this invocation as `false`, without rolling back
/// earlier writes or RNG draws and without disabling the feature type. A feature that merely did
/// not place (a tree with no room) answers `Ok(false)` instead.
// no wire: retains the native source chain for this in-process callback.
#[derive(Debug)]
// exhaustive: its field is private, so mods cannot build or destructure it
pub struct FeatureError {
    source: Box<dyn std::error::Error + Send + Sync>,
}
impl FeatureError {
    /// Preserve the original error for [`std::error::Error::source`].
    /// For a standalone message, use `std::io::Error::other(message)`.
    pub fn new(cause: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            source: Box::new(cause),
        }
    }
}
impl std::fmt::Display for FeatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.source, f)
    }
}
impl std::error::Error for FeatureError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

/// A mod's feature type: vanilla's `Feature`, whose code answers for a configured feature with
/// `"type": "modid:tree"`. Implemented once, kept by the game; deterministic and stateless.
pub trait FeatureType: Send + Sync + 'static {
    /// `Feature.place(level, generator, random, origin)`: place the feature at `origin`,
    /// answering whether it placed. Writes reach the bounded generation region or live level
    /// immediately; blocks already written and draws already made stay on `Ok(false)`, `Err`, or a
    /// panic, as vanilla has no rollback. The configured feature's settings are
    /// [`context.config()`](FeatureContext::config).
    fn place(
        &self,
        context: &mut dyn FeatureContext,
        origin: [i32; 3],
    ) -> Result<bool, FeatureError>;
}

/// What a feature's [`place`](FeatureType::place) sees: vanilla's `WorldGenLevel` around the
/// origin (the real bounded decoration region on a worker, or the live level for growth and
/// commands) and the caller's random. Writes go directly to that world with no staging buffer or
/// custom work quota: subsequent reads, block-tag checks and heightmap queries immediately see
/// blocks already written. A read the level can't answer is `None`. Neither reads nor writes load
/// a chunk.
///
/// Write refusals from the region (outside the writable region or upgrade height) return `false`
/// from [`ensure_can_write`](Self::ensure_can_write) and [`set_block`](Self::set_block); they are
/// ordinary operation results and do not roll back earlier writes or disable the feature type. An
/// `Err` from [`FeatureType::place`] logs the registering mod, type key, origin and cause chain
/// and treats this invocation as `false`, leaving earlier writes, RNG draws and future
/// invocations intact. A panic in the callback disables that registered callback for the session
/// and reports loudly (log plus error toast) without rolling back earlier writes or draws. A
/// feature can't place another feature.
///
/// Features run on a worldgen worker for decoration and on the tick thread for growth, commands
/// and `LevelRef::place_feature`; the context holds no `Host`, so a worker never reaches mod
/// state through it.
pub trait FeatureContext {
    /// Immutable, load-owned block metadata, safe to borrow on this worker.
    fn metadata(&self) -> &dyn BlockMetadata;
    /// The configured feature's settings, decoded against the type's schema when the
    /// definition loaded: one value per declared field, defaults filled in.
    fn config(&self) -> &DatapackEntry;
    /// The caller's random: the same stream the placement modifiers before this feature drew
    /// from, so draws stay in vanilla's order.
    fn random(&mut self) -> &mut dyn RandomSource;
    /// The level's minimum block Y.
    fn min_y(&mut self) -> Option<i32>;
    /// The level's maximum block Y.
    fn max_y(&mut self) -> Option<i32>;
    /// The block's default state; resolve it once, not once per placed block.
    fn state(&self, block: BlockKey) -> BlockState;
    /// A block state with partial properties; missing properties keep defaults.
    fn state_with(&self, block: BlockKey, properties: &[(&str, &str)]) -> BlockState;
    /// `BlockState.isAir`.
    fn is_air(&self, state: BlockState) -> bool;
    /// The current block at `pos`, or `None` where the level can't be read.
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockState>;
    /// `BlockState.is(tag)` of the current block at `pos`: whether it is in block tag `tag`
    /// (`minecraft:leaves`), or `None` where the level can't be read.
    fn block_in_tag(&mut self, pos: [i32; 3], tag: &str) -> Option<bool>;
    /// `WorldGenRegion.ensureCanWrite(pos)` during generation (the center chunk and its eight
    /// neighbors during FEATURES, plus the center's upgrade generation height while upgrading), or
    /// the live level's write admissibility.
    fn ensure_can_write(&mut self, pos: [i32; 3]) -> bool;
    /// `LevelWriter.setBlock(pos, state, flags)`, as `Feature.setBlock` (flags
    /// `BlockUpdateFlags::NEIGHBORS | CLIENTS`) and the tree features (`| KNOWN_SHAPE`) call it:
    /// delegates directly to the world and returns its result. During FEATURES,
    /// `WorldGenRegion.ensureCanWrite` allows the center chunk and its eight neighbors; if the
    /// center is upgrading, its generation height also restricts writes throughout the region,
    /// regardless of the target chunk. A refusal returns `false` without disabling the type.
    fn set_block(&mut self, pos: [i32; 3], state: BlockState, flags: BlockUpdateFlags) -> bool;
    /// `WorldGenLevel.getHeight` of the heightmap kind at (x, z), seeing writes already made.
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32>;
    /// Set block-entity saved NBT (its `id` and fields) for the current block at `pos`, validated
    /// against that block as `BlockEntity.loadWithComponents`. Writes immediately when `pos` is
    /// admissible and not in a read-only full chunk; returns `false` if the data does not fit the
    /// current block or the position cannot take a block entity.
    fn block_entity(&mut self, pos: [i32; 3], nbt: Compound) -> bool;
}

/// A registered API behaviour; the game builds its engine adapter at startup.
pub(crate) struct FeatureTypeInfo {
    pub(crate) mod_id: &'static str,
    pub(crate) id: Box<str>,
    pub(crate) config: Vec<DatapackField>,
    pub(crate) ty: Arc<dyn FeatureType>,
}

/// Registered feature behaviours with their registering mod id and config schemas, for the hidden
/// game integration boundary.
// hidden: the game's startup adapter reads the registered behaviours; mods declare them through the registrar.
#[doc(hidden)]
pub fn feature_type_declarations() -> Vec<(
    &'static str,
    String,
    Vec<DatapackField>,
    Arc<dyn FeatureType>,
)> {
    crate::parts::installed_feature_type_list()
        .iter()
        .map(|info| {
            (
                info.mod_id,
                info.id.to_string(),
                info.config.clone(),
                info.ty.clone(),
            )
        })
        .collect()
}

/// A configured feature's JSON decoded against its type's schema, `type` aside; the error names
/// the offending key.
// hidden: the game decodes configs at load; mods read the decoded entry from their context.
#[doc(hidden)]
pub fn decode_feature_config(
    schema: &[DatapackField],
    json: &serde_json::Value,
) -> Result<DatapackEntry, String> {
    let serde_json::Value::Object(object) = json else {
        return Err("a configured feature is a JSON object".into());
    };
    crate::world::datapack_registry::decode_record(schema, object, &["type"])
        .map_err(|error| format!("config: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::FeatureError;
    use crate::{BlockStateError, StructureError};
    use std::error::Error;

    #[test]
    fn worldgen_errors_retain_typed_sources() {
        let feature = FeatureError::new(BlockStateError::UnknownProperty(2));
        let structure = StructureError::new(BlockStateError::InvalidValue(3));
        let cases: [(&dyn Error, BlockStateError); 2] = [
            (&feature, BlockStateError::UnknownProperty(2)),
            (&structure, BlockStateError::InvalidValue(3)),
        ];

        for (error, expected) in cases {
            let source = error.source().unwrap();
            assert_eq!(source.downcast_ref::<BlockStateError>(), Some(&expected));
        }
    }
}
