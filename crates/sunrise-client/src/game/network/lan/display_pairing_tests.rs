use super::{C, Game, HubConfig, LanSettings, NetConfig, Value, serializer};
use crate::game::{commands::Target, native_entity_refs::tests::with_game};
use std::sync::{Arc, mpsc};
use sunrise_client::{
    chunk::Chunk,
    lan::{Joined, Out, PlayerSender, guest::Guest},
};
use sunrise_core::nbt::Tag;
use sunrise_protocol::{buf::Reader, packets::configuration::ClientInformation};

fn guest_at_display(game: &mut Game<'_>) -> mpsc::Receiver<Out> {
    game.lan_host(
        LanSettings {
            port: 0,
            guest_commands: false,
            force_game_mode: false,
        },
        NetConfig {
            listen: false,
            announce: false,
            lod_distance: 0,
            ..NetConfig::lan(0, String::new(), 2)
        },
        HubConfig {
            default_game_mode: 0,
            guest_commands: false,
            force_game_mode: false,
            hardcore: false,
            view_distance: 2,
            simulation_distance: 2,
            dimension: game.dim.id().to_owned(),
            allow_flight: false,
            seed: 0,
            max_players: 2,
            integrated: true,
            levels: game.level_shapes,
            level_seeds: game.levels.runtime.seeds(),
        },
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    let id = game.take_entity_id();
    let mut guest = Guest::new(
        Joined {
            conn: 1,
            uuid: 1,
            name: "DisplayGuest".into(),
            properties: Vec::new(),
            info: ClientInformation::new(2),
            brand: String::new(),
            sender: PlayerSender::new(tx, Arc::default()),
            latency: Arc::default(),
            address: "127.0.0.1:1".parse().unwrap(),
            expected: false,
        },
        id as i32,
        [0.5, 64.0, 0.5],
        2,
    );
    guest.placed = true;
    guest.sent_chunks.insert((0, 0));
    game.lan.as_mut().unwrap().hub.guests.push(guest);
    rx
}

/// Keep the game fixture's global native registration in a fresh process.
#[test]
#[ignore = "daily"]
fn late_displays_pair_their_item_and_keybind_components() {
    const CHILD: &str = "SUNRISE_DISPLAY_PAIRING_MARKER";
    let Some(marker) = std::env::var_os(CHILD) else {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("ran");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "game::lan::display_pairing_tests::late_displays_pair_their_item_and_keybind_components",
                "--include-ignored",
            ])
            .env(CHILD, &marker)
            .env("SUNRISE_HOME", root.path().join("home"))
            .env("SUNRISE_LAUNCHER_HOME", root.path().join("launcher"))
            .env("XDG_DATA_HOME", root.path().join("data"))
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .env("SUNRISE_LAUNCHER_NO_SELF_UPDATE", "1")
            .env("SUNRISE_NO_AUDIO", "1")
            .status()
            .unwrap();
        assert!(status.success() && marker.exists());
        return;
    };
    std::fs::write(marker, "ran").unwrap();
    with_game(|game| {
        game.build_registries();
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let rx = guest_at_display(game);
        game.as_console(|game| {
            assert!(game.run_command("summon block_display 0.5 64 0.5"));
        });
        let existing = game.others.last().unwrap().id as i32;
        game.tick_lan(true);
        let host = game.lan.as_ref().unwrap();
        assert_eq!(host.states_next_entity, Some(game.next_entity));
        assert!(host.hub.guests[0].tracker.is_tracking(existing));
        let diamond = host
            .hub
            .registry_id("minecraft:item", "minecraft:diamond")
            .unwrap();
        rx.try_iter().for_each(drop);

        game.as_console(|game| {
            assert!(game.run_command(
                "summon item_display 0.5 64 0.5 {item:{id:\"minecraft:diamond\",count:2}}"
            ));
        });
        let item = game.others.last().unwrap().id as i32;
        game.as_console(|game| {
            assert!(
                game.run_command("summon text_display 0.5 64 0.5 {text:{keybind:\"key.jump\"}}")
            );
        });
        let text = game.others.last().unwrap().id;
        let keybind = Tag::compound([("keybind", Tag::String("key.jump".into()))]);
        let save = game.command_target_nbt(Target::Other(text)).unwrap();
        assert_eq!(save.get("text"), Some(&keybind));
        let display = game.display(game.entity(text).unwrap()).unwrap();
        let component = display.text(game).unwrap().unwrap();
        assert_eq!(
            component.content,
            mod_api::text::TextContent::Keybind("key.jump".into())
        );

        // No intervening ordinary tracking pass may repair the pairing packet.
        game.lan_note_joined();
        let metadata: Vec<_> = rx
            .try_iter()
            .flat_map(|out| match out {
                Out::Packet(body) => vec![body],
                Out::Packets(bodies) => bodies,
                Out::Disconnect(_) => panic!("unexpected disconnect"),
            })
            .filter_map(|body| {
                let mut reader = Reader::new(&body);
                let id = reader.varint().unwrap();
                match C::decode(id, reader.rest()).unwrap() {
                    C::SetEntityData(packet) => Some(packet),
                    _ => None,
                }
            })
            .collect();
        assert!(metadata.iter().any(|packet| {
            packet.id == item
                && packet.values.iter().any(|entry| {
                    matches!(entry, (23, serializer::ITEM_STACK, Value::Item(Some(stack)))
                        if stack.item == diamond && stack.count == 2)
                })
        }));
        assert!(metadata.iter().any(|packet| {
            packet.id == text as i32
                && packet.values.contains(&(
                    23,
                    serializer::COMPONENT,
                    Value::Text(keybind.clone()),
                ))
        }));
    });
}
