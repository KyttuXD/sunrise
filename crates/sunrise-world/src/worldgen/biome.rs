//! Biome definitions and multi-noise biome resolution over an imported parameter table.
use super::{
    DecodeError,
    biome_temperature::BiomeTemperatureSettings,
    block_feature::{FeatureBlockBindings, integer_properties},
    carver::CarverCatalog,
    climate::{
        ClimateIndex, ClimateSampler, ClimateSearch, ClimateVolume, IndexError, Parameter,
        ParameterPoint,
    },
    density::identifier,
    feature_catalog::{BiomePlacedFeature, FeatureCatalog},
    generator::{
        GeneratorError,
        data::{build_tags, load_carvers, read_registry_bundle, registry_tag_definitions},
    },
    overrides::WorldgenOverrides,
};
use crate::{
    environment_attributes::{AttributeMap, packed_rgb},
    registry::BlockRegistry,
};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};
use sunrise_core::{
    density::{SampleError, SamplerContext},
    nbt::{Compound, Tag},
};

/// The carver and placed-feature registries used by `BiomeGenerationSettings.CODEC`.
pub struct BiomeGenerationRegistries<'a> {
    carvers: CarverCatalog,
    features: FeatureCatalog,
    dir: &'a Path,
    packs: &'a WorldgenOverrides,
    blocks: &'a BlockRegistry,
    bindings: Option<FeatureBlockBindings<'a>>,
}

impl<'a> BiomeGenerationRegistries<'a> {
    pub fn load(
        dir: &'a Path,
        packs: &'a WorldgenOverrides,
        blocks: &'a BlockRegistry,
    ) -> Result<Self, GeneratorError> {
        let carvers = load_carvers(dir, packs)?;
        let features = FeatureCatalog::from_resources(
            &read_registry_bundle(dir, packs, "features")?.unwrap_or_default(),
            &read_registry_bundle(dir, packs, "placed_features")?.unwrap_or_default(),
            &registry_tag_definitions(dir, packs, "placed_feature", "placed_feature_tags")?,
        )?;
        Ok(Self {
            carvers,
            features,
            dir,
            packs,
            blocks,
            bindings: None,
        })
    }
}

/// A biome admitted by `Biome.DIRECT_CODEC`. Its local data retains the
/// generation settings and attributes that never go to joining clients.
pub struct BiomeDefinition(Value);

impl BiomeDefinition {
    pub fn decode(
        mut value: Value,
        generation: &mut BiomeGenerationRegistries<'_>,
    ) -> Result<Self, DecodeError> {
        let fields = NetworkBiome::decode(&value)?;
        // EnvironmentAttributeMap.CODEC_ONLY_POSITIONAL.
        if let Some(attribute) = fields
            .attributes
            .attributes()
            .find(|a| matches!(a.id, "gameplay/sky_light_level" | "gameplay/fast_lava"))
        {
            return Err(DecodeError::Registry(format!(
                "attribute {} cannot be positional",
                attribute.id
            )));
        }
        let record = value.as_object_mut().ok_or(DecodeError::Field("biome"))?;
        let carvers = record.get("carvers").ok_or(DecodeError::Field("carvers"))?;
        generation
            .carvers
            .decode_set(&carvers.to_string())
            .map_err(|error| DecodeError::Registry(format!("carvers: {error}")))?;
        let features = record
            .get("features")
            .ok_or(DecodeError::Field("features"))?;
        for holder in generation
            .features
            .biome_steps(features)?
            .into_iter()
            .flatten()
        {
            if let BiomePlacedFeature::Direct(json) = holder {
                let bindings = match &mut generation.bindings {
                    Some(bindings) => bindings,
                    slot @ None => {
                        let tags =
                            read_registry_bundle(generation.dir, generation.packs, "block_tags")
                                .and_then(|base| {
                                    let mut warnings = Vec::new();
                                    let tags = build_tags(
                                        &base.unwrap_or_default(),
                                        generation.packs,
                                        "block_tags",
                                        &mut warnings,
                                        |id| generation.blocks.default_state(id).is_some(),
                                    )?;
                                    for warning in warnings {
                                        log::warn!("{warning}");
                                    }
                                    Ok(tags)
                                })
                                .map_err(|error| DecodeError::Registry(error.to_string()))?;
                        slot.insert(FeatureBlockBindings::with_tags(
                            generation.blocks,
                            integer_properties(generation.blocks),
                            &tags,
                        )?)
                    }
                };
                generation
                    .features
                    .placed_feature_with_blocks(&json, bindings)?;
            }
        }
        // Use the decoded special effects for the local readers too, including RGB vectors.
        record.insert(
            "effects".into(),
            Value::Object(
                fields
                    .effects
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), Value::String(value)))
                    .collect(),
            ),
        );
        Ok(Self(value))
    }

    pub fn into_json(self) -> Value {
        self.0
    }

    /// `Biome.NETWORK_CODEC` as used by `RegistrySynchronization.packRegistry`.
    /// Encode the frozen holder, not its pack resource or load conditions.
    pub fn encode_network(value: &Value) -> Result<Tag, DecodeError> {
        use super::biome_temperature::TemperatureModifier;
        let NetworkBiome {
            climate,
            attributes,
            effects,
        } = NetworkBiome::decode(value)?;
        let mut nbt = Compound::from([
            (
                "has_precipitation".into(),
                Tag::Byte(i8::from(climate.has_precipitation)),
            ),
            (
                "temperature".into(),
                Tag::Float(climate.base_temperature.to_bits()),
            ),
            ("downfall".into(), Tag::Float(climate.downfall.to_bits())),
            (
                "effects".into(),
                Tag::compound(
                    effects
                        .into_iter()
                        .map(|(key, value)| (key, Tag::String(value.as_str().into()))),
                ),
            ),
        ]);
        if climate.modifier == TemperatureModifier::Frozen {
            nbt.insert("temperature_modifier".into(), Tag::String("frozen".into()));
        }
        // The optional field checks its default before the nested network codec filters it.
        if !attributes.is_empty() {
            let attributes = attributes
                .into_network_nbt()
                .map_err(DecodeError::Registry)?;
            nbt.insert("attributes".into(), Tag::Compound(attributes));
        }
        Ok(Tag::Compound(nbt))
    }
}

/// Fields shared by the direct and network codecs; the latter encodes only
/// the syncable subset of the attribute map.
struct NetworkBiome {
    climate: BiomeTemperatureSettings,
    attributes: AttributeMap,
    effects: BTreeMap<&'static str, String>,
}

impl NetworkBiome {
    fn decode(value: &Value) -> Result<Self, DecodeError> {
        let climate = BiomeTemperatureSettings::from_json(&value.to_string())?;
        let attributes = value
            .get("attributes")
            .map(AttributeMap::parse)
            .transpose()
            .map_err(DecodeError::Registry)?
            .unwrap_or_default();
        let raw = value
            .get("effects")
            .and_then(Value::as_object)
            .ok_or(DecodeError::Field("effects"))?;
        let mut effects = BTreeMap::new();
        // BiomeSpecialEffects.CODEC and ExtraCodecs.STRING_RGB_COLOR.
        for (key, field, required) in [
            ("water_color", "effects.water_color", true),
            ("foliage_color", "effects.foliage_color", false),
            ("dry_foliage_color", "effects.dry_foliage_color", false),
            ("grass_color", "effects.grass_color", false),
        ] {
            match raw.get(key) {
                Some(color) => {
                    let rgb = packed_rgb(color).ok_or(DecodeError::Field(field))?;
                    effects.insert(key, format!("#{rgb:06x}"));
                }
                None if required => return Err(DecodeError::Field(field)),
                None => {}
            }
        }
        match raw.get("grass_color_modifier") {
            None => {}
            Some(modifier) => match modifier.as_str() {
                Some("none") => {}
                Some(name @ ("dark_forest" | "swamp")) => {
                    effects.insert("grass_color_modifier", name.to_owned());
                }
                _ => return Err(DecodeError::Field("effects.grass_color_modifier")),
            },
        }
        Ok(Self {
            climate,
            attributes,
            effects,
        })
    }
}

/// Identity within an immutable biome registry snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BiomeId(pub u32);

#[derive(Debug)]
pub enum BiomeError {
    Decode(DecodeError),
    Index(IndexError),
    Sample(SampleError),
    OutsideVolume,
}
impl std::fmt::Display for BiomeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(e) => e.fmt(f),
            Self::Index(e) => e.fmt(f),
            Self::Sample(e) => e.fmt(f),
            Self::OutsideVolume => f.write_str("biome query is outside the prepared quart volume"),
        }
    }
}
impl std::error::Error for BiomeError {}
impl From<DecodeError> for BiomeError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}
impl From<IndexError> for BiomeError {
    fn from(e: IndexError) -> Self {
        Self::Index(e)
    }
}
impl From<SampleError> for BiomeError {
    fn from(e: SampleError) -> Self {
        Self::Sample(e)
    }
}

#[derive(Debug)]
pub struct MultiNoiseBiomeSource {
    parameters: ClimateIndex<String>,
}
impl MultiNoiseBiomeSource {
    pub fn new(entries: Vec<(ParameterPoint, String)>) -> Result<Self, BiomeError> {
        let entries = entries
            .into_iter()
            .map(|(point, name)| Ok((point, identifier(&name)?)))
            .collect::<Result<_, DecodeError>>()?;
        Ok(Self {
            parameters: ClimateIndex::new(entries)?,
        })
    }
    /// Sunrise's quantized import format, not the vanilla biome definition
    /// codec: [{"coordinates":[[min,max]; six],"offset":long,"biome":id}].
    /// Built-in numeric tables stay private and are authenticated by the runner.
    pub fn from_quantized_json(json: &str) -> Result<Self, BiomeError> {
        let input: serde_json::Value = serde_json::from_str(json).map_err(DecodeError::from)?;
        let mut entries = Vec::new();
        for entry in input
            .as_array()
            .ok_or(DecodeError::Field("biome parameter list"))?
        {
            let raw = entry["coordinates"]
                .as_array()
                .filter(|v| v.len() == 6)
                .ok_or(DecodeError::Field("climate coordinates"))?;
            let mut coordinates = [Parameter::new(0, 0); 6];
            for (coordinate, raw) in coordinates.iter_mut().zip(raw) {
                let raw = raw
                    .as_array()
                    .filter(|v| v.len() == 2)
                    .ok_or(DecodeError::Field("climate interval"))?;
                *coordinate = Parameter::new(
                    raw[0]
                        .as_i64()
                        .ok_or(DecodeError::Field("climate minimum"))?,
                    raw[1]
                        .as_i64()
                        .ok_or(DecodeError::Field("climate maximum"))?,
                );
            }
            let offset = entry["offset"]
                .as_i64()
                .ok_or(DecodeError::Field("climate offset"))?;
            let biome = entry["biome"]
                .as_str()
                .ok_or(DecodeError::Field("biome identifier"))?;
            entries.push((
                ParameterPoint {
                    coordinates,
                    offset,
                },
                biome.to_owned(),
            ));
        }
        Self::new(entries)
    }
    pub fn parameters(&self) -> &ClimateIndex<String> {
        &self.parameters
    }
    pub fn resolver<'a>(&'a self, climate: &'a ClimateSampler) -> BiomeResolver<'a> {
        BiomeResolver {
            climate,
            search: self.parameters.searcher(),
        }
    }
}
#[derive(Debug)]
pub struct BiomeResolver<'a> {
    climate: &'a ClimateSampler,
    search: ClimateSearch<'a, String>,
}
impl<'a> BiomeResolver<'a> {
    pub fn sample(
        &mut self,
        context: &mut SamplerContext,
        quart: [i32; 3],
    ) -> Result<&'a str, BiomeError> {
        Ok(self
            .search
            .find(self.climate.sample(context, quart)?)?
            .as_str())
    }
    pub fn prepare_volume(
        &self,
        context: &mut SamplerContext,
        minimum: [i32; 3],
        extents: [i32; 3],
    ) -> Result<ClimateVolume, BiomeError> {
        Ok(self.climate.sample_volume(context, minimum, extents)?)
    }
    /// Query order shares the same previous-leaf state as scalar resolution.
    pub fn sample_prepared(
        &mut self,
        volume: &ClimateVolume,
        quart: [i32; 3],
    ) -> Result<&'a str, BiomeError> {
        let target = volume.target(quart).ok_or(BiomeError::OutsideVolume)?;
        Ok(self.search.find(target)?.as_str())
    }
}
