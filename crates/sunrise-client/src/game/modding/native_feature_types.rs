//! Native worker adapter for API-owned feature behaviours: the engine's `ModFeatureType` around a
//! mod's `FeatureType`, over the caller's own world and random.
use super::native_worldgen::WorkerResources;
use mod_api::{
    __private::game::CallbackOwner, BlockKey, BlockMetadata, BlockState, BlockUpdateFlags,
    DatapackEntry, DatapackField, FeatureContext, FeatureError, FeatureType, HeightmapType,
    RandomSource,
};
use std::any::Any;
use std::sync::Arc;
use sunrise_core::nbt::{Compound, Tag};
use sunrise_world::registry::BlockRegistry;
use sunrise_world::worldgen::mod_features::{
    ModFeatureConfig, ModFeatureEntry, ModFeatureRandom, ModFeatureTarget, ModFeatureType,
    ModFeatureTypes,
};

fn engine_heightmap(kind: HeightmapType) -> sunrise_world::heightmap::HeightmapType {
    use sunrise_world::heightmap::HeightmapType as Engine;
    match kind {
        HeightmapType::WorldSurfaceWg => Engine::WorldSurfaceWg,
        HeightmapType::WorldSurface => Engine::WorldSurface,
        HeightmapType::OceanFloorWg => Engine::OceanFloorWg,
        HeightmapType::OceanFloor => Engine::OceanFloor,
        HeightmapType::MotionBlocking => Engine::MotionBlocking,
        HeightmapType::MotionBlockingNoLeaves => Engine::MotionBlockingNoLeaves,
        _ => Engine::WorldSurfaceWg,
    }
}

/// The caller's random, as a [`RandomSource`].
struct Draws<'a>(&'a mut dyn ModFeatureRandom);
impl RandomSource for Draws<'_> {
    fn next_int_bound(&mut self, bound: i32) -> i32 {
        CallbackOwner::call_engine(|| self.0.int(bound))
    }
    fn next_long(&mut self) -> i64 {
        CallbackOwner::call_engine(|| self.0.long())
    }
    fn next_float(&mut self) -> f32 {
        CallbackOwner::call_engine(|| self.0.float())
    }
    fn next_double(&mut self) -> f64 {
        CallbackOwner::call_engine(|| self.0.double())
    }
    fn next_boolean(&mut self) -> bool {
        CallbackOwner::call_engine(|| self.0.boolean())
    }
    fn next_int(&mut self) -> i32 {
        CallbackOwner::call_engine(|| self.0.int_unbounded())
    }
    fn next_gaussian(&mut self) -> f64 {
        CallbackOwner::call_engine(|| self.0.gaussian())
    }
}

/// The engine's target, as a [`FeatureContext`].
struct Placing<'a, 't, 'r> {
    mod_id: &'static str,
    feature_id: &'a str,
    target: &'t mut dyn ModFeatureTarget,
    worker: &'static WorkerResources,
    registry: &'static BlockRegistry,
    random: Draws<'r>,
    config: &'a DatapackEntry,
}

impl FeatureContext for Placing<'_, '_, '_> {
    fn metadata(&self) -> &dyn BlockMetadata {
        self.worker.metadata
    }
    fn config(&self) -> &DatapackEntry {
        self.config
    }
    fn random(&mut self) -> &mut dyn RandomSource {
        &mut self.random
    }
    fn min_y(&mut self) -> Option<i32> {
        CallbackOwner::call_engine(|| self.target.min_y())
    }
    fn max_y(&mut self) -> Option<i32> {
        CallbackOwner::call_engine(|| self.target.max_y())
    }
    fn state(&self, block: BlockKey) -> BlockState {
        CallbackOwner::call_engine(|| self.worker.state(block))
    }
    fn state_with(&self, block: BlockKey, properties: &[(&str, &str)]) -> BlockState {
        CallbackOwner::call_engine(|| self.worker.state_with(self.registry, block, properties))
    }
    fn is_air(&self, state: BlockState) -> bool {
        CallbackOwner::call_engine(|| self.worker.is_air(state))
    }
    fn block(&mut self, pos: [i32; 3]) -> Option<BlockState> {
        CallbackOwner::call_engine(|| {
            let state = self.target.block(pos)?;
            self.worker.index.to_api(state).ok()
        })
    }
    fn block_in_tag(&mut self, pos: [i32; 3], tag: &str) -> Option<bool> {
        CallbackOwner::call_engine(|| self.target.block_in_tag(pos, tag))
    }
    fn ensure_can_write(&mut self, pos: [i32; 3]) -> bool {
        CallbackOwner::call_engine(|| self.target.ensure_can_write(pos))
    }
    fn set_block(&mut self, pos: [i32; 3], state: BlockState, flags: BlockUpdateFlags) -> bool {
        CallbackOwner::call_engine(|| {
            let Ok(state) = self.worker.index.to_engine(state) else {
                log::warn!(
                    "mod {}: feature type {} refused unknown block state at {pos:?}",
                    self.mod_id,
                    self.feature_id,
                );
                return false;
            };
            self.target.set_block(pos, state, flags.bits())
        })
    }
    fn height(&mut self, kind: HeightmapType, x: i32, z: i32) -> Option<i32> {
        CallbackOwner::call_engine(|| self.target.height(engine_heightmap(kind), x, z))
    }
    fn block_entity(&mut self, pos: [i32; 3], nbt: Compound) -> bool {
        CallbackOwner::call_engine(|| {
            // Validate decoded NBT once against the current block at `pos` using the existing
            // block-entity codec, reporting mismatched or invalid data before writing immediately.
            let Some(name) = self
                .target
                .block(pos)
                .and_then(|state| self.registry.state(state))
                .map(|state| state.name.as_str())
            else {
                log::warn!(
                    "mod {}: feature type {} refused block entity at {pos:?}: block state unavailable",
                    self.mod_id,
                    self.feature_id,
                );
                return false;
            };
            let mut placed = nbt.clone();
            for (axis, value) in ["x", "y", "z"].into_iter().zip(pos) {
                placed.insert(axis.into(), Tag::Int(value));
            }
            let placed = Tag::Compound(placed);
            if sunrise_client::save::decode_block_entity_for_block(&placed, name).is_none() {
                let entity_id = match placed.get("id") {
                    Some(Tag::String(id)) => id.to_string_lossy(),
                    _ => "<missing>".to_owned(),
                };
                log::warn!(
                    "mod {}: feature type {} refused invalid block entity {entity_id:?} for block {name} at {pos:?}",
                    self.mod_id,
                    self.feature_id,
                );
                return false;
            }
            self.target.set_block_entity(pos, nbt)
        })
    }
}

/// The adapter: the game's `ModFeatureType` around a mod's `FeatureType`.
pub(crate) struct Adapter {
    feature_id: String,
    owner: CallbackOwner,
    inner: Arc<dyn FeatureType>,
    schema: Vec<DatapackField>,
}

impl Adapter {
    pub(crate) fn new(
        mod_id: &'static str,
        feature_id: String,
        schema: Vec<DatapackField>,
        inner: Arc<dyn FeatureType>,
    ) -> Self {
        Self {
            feature_id,
            owner: CallbackOwner::for_mod(mod_id),
            inner,
            schema,
        }
    }
}

impl ModFeatureType for Adapter {
    fn prepare_config(&self, config: &serde_json::Value) -> Result<ModFeatureConfig, String> {
        let entry = mod_api::__private::decode_feature_config(&self.schema, config)?;
        Ok(Arc::new(entry))
    }
    fn place(
        &self,
        target: &mut dyn ModFeatureTarget,
        random: &mut dyn ModFeatureRandom,
        origin: [i32; 3],
        config: &(dyn Any + Send + Sync),
    ) -> Result<bool, FeatureError> {
        let Some(registry) = target.registry() else {
            return Err(FeatureError::new(std::io::Error::other(
                "this world has no block registry for mod features",
            )));
        };
        let config = config.downcast_ref::<DatapackEntry>().ok_or_else(|| {
            FeatureError::new(std::io::Error::other(
                "config was not decoded by this adapter",
            ))
        })?;
        let worker = WorkerResources::get(registry).map_err(FeatureError::new)?;
        let mut context = Placing {
            mod_id: self.owner.id(),
            feature_id: &self.feature_id,
            target,
            worker,
            registry,
            random: Draws(random),
            config,
        };
        match self.owner.call("feature type", "feature_place", || {
            self.inner.place(&mut context, origin)
        }) {
            Ok(result) => result,
            Err(_) => Ok(false),
        }
    }
}

#[cfg(test)]
#[path = "native_feature_types/tests.rs"]
mod tests;

/// Registered API declarations become engine adapters once, not once per chunk.
pub(in crate::game) fn installed_feature_types() -> ModFeatureTypes {
    mod_api::__private::feature_type_declarations()
        .into_iter()
        .map(|(mod_id, id, schema, ty)| {
            let adapter: Arc<dyn ModFeatureType> =
                Arc::new(Adapter::new(mod_id, id.clone(), schema, ty));
            (id, Arc::new(ModFeatureEntry::new(mod_id, adapter)))
        })
        .collect()
}
