//! Resource readers for native mods through the game's selected pack stacks.
use super::Game;
use mod_api::{Identifier, PackType, ResourceOpenError};
use std::io::Read;
use sunrise_assets::resource_source::ResourcePath;

impl Game<'_> {
    pub(in crate::game) fn open_resource(
        &self,
        pack_type: PackType,
        identifier: &Identifier,
    ) -> Result<Option<Box<dyn Read + '_>>, ResourceOpenError> {
        match pack_type {
            PackType::ClientResources if self.options.execution_role.is_client() => {
                let name = format!("assets/{}/{}", identifier.namespace(), identifier.path());
                let Some(path) = ResourcePath::new(&name) else {
                    return Ok(None);
                };
                self.pack.source.raw_pack_files().open_resource(path)
            }
            PackType::ServerData => {
                let world = self
                    .options
                    .execution_role
                    .is_logical_server()
                    .then(|| self.fns.enabled_packs_highest_first())
                    .into_iter()
                    .flatten();
                let lower = std::iter::once(&self.pack.data_changes)
                    .chain((!self.cmds.vanilla_disabled).then_some(&self.pack.data_source));
                sunrise_client::datapack::open_resource(world, lower, identifier)
            }
            _ => return Ok(None),
        }
        .map_err(ResourceOpenError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mod_api::prelude::Host;
    use std::collections::BTreeMap;
    use sunrise_assets::resource_source::{Resource, ResourceSource};

    fn read(host: &dyn Host, pack_type: PackType, name: &str) -> Option<Vec<u8>> {
        let mut reader = host
            .open_resource(pack_type, &Identifier::parse(name).unwrap())
            .unwrap()?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        Some(bytes)
    }

    #[test]
    #[ignore = "daily"]
    fn server_resource_uses_the_higher_of_two_selected_world_packs() {
        use std::{
            fs::File,
            io::{self, Seek, SeekFrom, Write},
        };

        super::super::native_entity_refs::tests::with_game(|game| {
            let scratch = tempfile::tempdir().unwrap();
            game.world_root = scratch.path().to_owned();
            let old_limit = 64 * 1024 * 1024;
            let identifier = "demo:value/order.bin";
            for (name, bytes) in [("low", b"low".as_slice()), ("high", b"high".as_slice())] {
                let pack = game.world_root.join("datapacks").join(name);
                std::fs::create_dir_all(pack.join("data/demo/value")).unwrap();
                std::fs::write(
                    pack.join("pack.mcmeta"),
                    r#"{"pack":{"description":"test","pack_format":121,"min_format":[121,0],"max_format":[121,0]}}"#,
                )
                .unwrap();
                let mut file = File::create(pack.join("data/demo/value/order.bin")).unwrap();
                file.write_all(bytes).unwrap();
                if name == "high" {
                    file.seek(SeekFrom::Start(old_limit)).unwrap();
                    file.write_all(b"end").unwrap();
                }
            }
            game.build_registries();
            game.set_allow_commands(true);
            assert!(game.run_command("reload"));
            assert!(game.run_command("datapack disable \"file/high\""));
            assert!(game.run_command("datapack disable \"file/low\""));
            assert!(game.run_command("datapack enable \"file/low\" first"));
            assert!(game.run_command("datapack enable \"file/high\" last"));
            // The high pack wins even though its file exceeds the old guest reply limit.
            {
                let host: &dyn Host = game;
                let mut reader = host
                    .open_resource(
                        PackType::ServerData,
                        &Identifier::parse(identifier).unwrap(),
                    )
                    .unwrap()
                    .unwrap();
                let mut head = [0; 4];
                reader.read_exact(&mut head).unwrap();
                assert_eq!(&head, b"high");
                let rest = old_limit - head.len() as u64;
                assert_eq!(
                    io::copy(&mut reader.by_ref().take(rest), &mut io::sink()).unwrap(),
                    rest
                );
                let mut tail = Vec::new();
                reader.read_to_end(&mut tail).unwrap();
                assert_eq!(tail, b"end");
            }
            assert!(game.run_command("datapack disable \"file/low\""));
            assert!(game.run_command("datapack enable \"file/low\" last"));
            assert_eq!(
                read(game, PackType::ServerData, identifier).as_deref(),
                Some(b"low".as_slice())
            );
        });
    }

    #[test]
    #[ignore = "daily"]
    fn server_data_reads_vanilla_files_and_refuses_missing_ones() {
        super::super::native_entity_refs::tests::with_game(|game| {
            let bytes = read(
                game,
                PackType::ServerData,
                "minecraft:enchantment/sharpness.json",
            )
            .expect("the game's own data pack holds sharpness");
            assert!(bytes.starts_with(b"{"));
            assert!(
                read(
                    game,
                    PackType::ServerData,
                    "minecraft:enchantment/not_a_real_enchantment.json"
                )
                .is_none()
            );
            // A server has no selected client asset stack to read.
            assert!(
                read(
                    game,
                    PackType::ClientResources,
                    "minecraft:models/block/stone.json"
                )
                .is_none()
            );
        });
    }

    #[test]
    #[ignore = "daily"]
    fn server_data_reads_mod_content_and_lets_it_override_the_game() {
        super::super::native_entity_refs::tests::with_game(|game| {
            let vanilla = read(
                game,
                PackType::ServerData,
                "minecraft:enchantment/sharpness.json",
            )
            .expect("the game's own data pack holds sharpness");
            game.pack.data_changes = ResourceSource::indexed(BTreeMap::from([
                (
                    "data/demo/machines/furnace.json".to_owned(),
                    Resource::bytes(b"{\"mod\":true}".to_vec()),
                ),
                (
                    "data/minecraft/enchantment/sharpness.json".to_owned(),
                    Resource::bytes(b"{\"pack\":true}".to_vec()),
                ),
            ]));
            // A mod's own data file resolves through its content layer.
            assert_eq!(
                read(game, PackType::ServerData, "demo:machines/furnace.json").as_deref(),
                Some(b"{\"mod\":true}".as_slice())
            );
            // A later layer in the stack overrides the game's own data.
            assert_eq!(
                read(
                    game,
                    PackType::ServerData,
                    "minecraft:enchantment/sharpness.json"
                )
                .as_deref(),
                Some(b"{\"pack\":true}".as_slice())
            );
            assert_ne!(vanilla, b"{\"pack\":true}");
        });
    }

    #[test]
    #[ignore = "daily"]
    fn client_assets_read_the_winning_pack_file() {
        super::super::native_entity_refs::tests::with_client_game(|game| {
            let bytes = read(
                game,
                PackType::ClientResources,
                "minecraft:models/block/stone.json",
            )
            .expect("the vanilla asset pack holds stone's model");
            assert!(bytes.starts_with(b"{"));
            assert!(
                read(
                    game,
                    PackType::ClientResources,
                    "minecraft:models/block/not_a_real_model.json"
                )
                .is_none()
            );
        });
    }
}
