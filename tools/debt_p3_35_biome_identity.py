#!/usr/bin/env python3
"""Replay the signature changes for the native biome identity migration."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VOCAB = "crates/sunrise-mod-vocab/src/"
GAME = "crates/sunrise-client/src/game/"
RENDER = "crates/sunrise-client-render/src/"
CLIENT = "crates/sunrise-client/"


def replace(relative: str, *pairs: tuple[str, str]) -> None:
    path = ROOT / relative
    contents = path.read_text()
    for before, after in pairs:
        assert before in contents, (relative, before)
        contents = contents.replace(before, after)
    path.write_text(contents)


replace(
    VOCAB + "world/structure_type.rs",
    ("use crate::{BlockKey,", "use crate::{BiomeRef, BlockKey,"),
    ("fn noise_biome(&mut self, quart: [i32; 3]) -> BiomeId;",
     "fn noise_biome(&mut self, quart: [i32; 3]) -> BiomeRef<'_>;"),
)
replace(
    GAME + "modding/native_structure_types.rs",
    ("BiomeId, ChunkPos,", "BiomeRef, ChunkPos,"),
    ("fn noise_biome(&mut self, quart: [i32; 3]) -> BiomeId {",
     "fn noise_biome(&mut self, quart: [i32; 3]) -> BiomeRef<'_> {"),
)
replace(
    VOCAB + "client/section_visibility.rs",
    ("use crate::{BiomeId, BlockState};", "use crate::{BiomeRef, BlockState};"),
    ("pub struct SectionVisibilityInput {", "pub struct SectionVisibilityInput<'a> {"),
    ("impl SectionVisibilityInput {", "impl<'a> SectionVisibilityInput<'a> {"),
    ("Option<BiomeId>", "Option<BiomeRef<'a>>"),
)
replace(
    VOCAB + "hooks/section_visibility.rs",
    ("input: &SectionVisibilityInput,", "input: &SectionVisibilityInput<'_>,"),
)
replace(
    "crates/sunrise-mod-api-gen/dispatch-tests/src/fixtures/section_visibility.rs",
    ("struct SectionVisibilityInput(pub(super) [i32; 3]);",
     "struct SectionVisibilityInput<'a>(pub(super) &'a [i32; 3]);"),
    ("SectionVisibilityInput([", "SectionVisibilityInput(&["),
    ("static OTHER: SectionVisibilityInput =", "static OTHER: SectionVisibilityInput<'static> ="),
)
replace(
    RENDER + "streaming/section_visibility.rs",
    ("use mod_api::{BiomeId,", "use mod_api::{BiomeRef,"),
    ("size_of::<Option<BiomeId>>()", "size_of::<Option<BiomeRef<'static>>>()"),
)
replace(
    VOCAB + "world/data_maps.rs",
    ("fn id<'a>(&self,", "fn id<'a>(&'a self,"),
)

# Keep the renderer's name in a shared allocation; the mod key can retain it
# without copying the text. Preserve previous owned String boundaries.
replace(
    RENDER + "biome.rs",
    ("pub name: String,", "pub name: Arc<str>,"),
    ("            name,\n        }\n    }\n}", "            name: name.into(),\n        }\n    }\n}"),
    ("name: \"minecraft:plains\".to_owned(),", 'name: "minecraft:plains".into(),'),
    ("b.name.clone()", "b.name.to_string()"),
    ("b.name.as_str()", "b.name.as_ref()"),
    ('assert_eq!(b.name, "minecraft:plains")', 'assert_eq!(b.name.as_ref(), "minecraft:plains")'),
    ('assert_eq!(biomes.get(gone).name, "minecraft:plains")', 'assert_eq!(biomes.get(gone).name.as_ref(), "minecraft:plains")'),
)
replace(RENDER + "environment.rs", ("self.biome.name.as_str()", "self.biome.name.as_ref()"))
replace(CLIENT + "examples/chunk_encoding_bench.rs", ("b.name.clone()", "b.name.to_string()"))
replace(CLIENT + "tests/it/datapack_biome_sync.rs", ("b.name.as_str()", "b.name.as_ref()"))
replace(CLIENT + "src/mobs/spawn_rules.rs", ("members.contains(biome)", "members.contains(biome.as_ref())"))
replace(GAME + "administration/commands/terrain_cmds.rs", ("|b| b.name == biome", "|b| b.name.as_ref() == biome"))
replace(GAME + "blocks/location_world.rs", (".name.clone()", ".name.to_string()"))
replace(GAME + "creatures/biome_facts.rs", ("b.name ==", "b.name.as_ref() =="))
replace(GAME + "interface/dedicated/mod.rs", ("biomes.get(biomes.id(b)).name == b", "biomes.get(biomes.id(b)).name.as_ref() == b"))
replace(GAME + "interface/shader_packs.rs", ("b.name.as_str()", "b.name.as_ref()"))
replace(GAME + "mobs/natural_spawning.rs", ("b.name.clone()", "b.name.to_string()"), ("b.name.as_str()", "b.name.as_ref()"))
replace(GAME + "modding/native_entity_refs.rs", ("list[target].name.clone()", "list[target].name.to_string()"))
replace(GAME + "modding/native_level_notify_tests.rs", *(
    (f"list[{slot}].name.clone()", f"list[{slot}].name.to_string()")
    for slot in ("usize::from(target)", "usize::from(old)", "1")
))
replace(GAME + "modding/native_level_refs.rs", ("get(biome).name.clone()", "get(biome).name.to_string()"))
for filename in ("network/lan/mod.rs", "runtime/replay.rs"):
    replace(GAME + filename, (".map(|b| b.name.clone())", ".map(|b| b.name.to_string())"))
