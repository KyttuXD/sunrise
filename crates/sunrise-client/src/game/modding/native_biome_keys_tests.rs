#![cfg(test)]
use super::execution_role::ExecutionRole;
use mod_api::{DataMap, DataMapError, LevelReadError, Tag};
struct VillagerTypes;
impl DataMap for VillagerTypes {
    type Entry = mod_api::BiomeKey;
    type Value = Tag;
    fn id(&self) -> &str {
        "neoforge:villager_types"
    }
    fn decode(&self, value: &Tag) -> Option<Tag> {
        Some(value.clone())
    }
}
#[test]
#[ignore = "daily"]
fn biome_key_world_install_reload_and_unavailable_sides() {
    mod_platform::testing::with_mods(Default::default(), || {
        crate::game::native_entity_refs::tests::with_game(|game| {
            let root = tempfile::tempdir().unwrap();
            game.world_root = root.path().to_owned();
            game.options.dedicated = Some(Vec::new());
            game.options.execution_role = ExecutionRole::DedicatedServer;
            assert_eq!(
                game.biome("minecraft:plains"),
                Err(LevelReadError::Unavailable)
            );
            let pack = root.path().join("datapacks/biomes");
            let file = pack.join("data/neoforge/data_maps/worldgen/biome/villager_types.json");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(
                pack.join("pack.mcmeta"),
                r#"{"pack":{"description":"Biome fixture"}}"#,
            )
            .unwrap();
            std::fs::write(
                root.path().join("datapacks.json"),
                r#"{"enabled":["file/biomes"],"disabled":[]}"#,
            )
            .unwrap();
            let mut previous: Option<mod_api::BiomeKey> = None;
            for value in ["minecraft:plains", "minecraft:savanna"] {
                std::fs::write(&file, format!(r#"{{"replace":true,"values":{{"minecraft:plains":{{"villager_type":"{value}"}}}}}}"#)).unwrap();
                let packs = game.reload_datapacks(false, &[]).unwrap();
                let resources = game.options.resources.clone();
                game.world_tables = game
                    .stage_world_tables(&packs, previous.is_some(), &resources)
                    .unwrap()
                    .tables;
                let plains = game.biome("minecraft:plains").unwrap().unwrap();
                assert_eq!(plains.id(), "minecraft:plains");
                assert_eq!(game.biome("test:nope"), Ok(None));
                let current = plains.key();
                let key = previous.get_or_insert_with(|| current.clone());
                assert_eq!(*key, current);
                assert_eq!(
                    key.data(game, &VillagerTypes),
                    Ok(Some(Tag::Compound(sunrise_core::nbt::Compound::from([(
                        "villager_type".into(),
                        Tag::String(value.into())
                    )]))))
                );
                assert_eq!(
                    game.biome("minecraft:desert")
                        .unwrap()
                        .unwrap()
                        .key()
                        .data(game, &VillagerTypes),
                    Ok(None)
                );
            }
            let key = previous.unwrap();
            game.options.dedicated = None;
            game.options.execution_role = ExecutionRole::Client;
            assert_eq!(
                game.biome("minecraft:plains"),
                Err(LevelReadError::Unavailable)
            );
            assert_eq!(key.id(), "minecraft:plains");
            assert_eq!(
                key.data(game, &VillagerTypes),
                Err(DataMapError::Unavailable)
            );
            game.options.dedicated = Some(Vec::new());
            game.options.execution_role = ExecutionRole::DedicatedServer;
            game.world_tables = None;
            assert_eq!(
                key.data(game, &VillagerTypes),
                Err(DataMapError::Unavailable)
            );
        });
    });
}
