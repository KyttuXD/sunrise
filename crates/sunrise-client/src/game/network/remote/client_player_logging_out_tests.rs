use crate::game::{
    native_entity_refs::tests::{run_fresh_cases, with_client_game},
    native_lifecycle::NativeLifecycle,
};
use mod_api::{ClientHost, ClientLevelUnloadEvent, ClientPlayerLoggingOutEvent, GameMode};
use std::sync::{Arc, Mutex};
use sunrise_protocol::{
    connection::Pipe,
    packets::play::{Clientbound, Login, Respawn, SpawnInfo},
};

#[derive(Debug, PartialEq, Eq)]
enum Notice {
    Logout {
        player: Option<(u32, u128)>,
        mode: Option<GameMode>,
        live_mode: Option<GameMode>,
    },
    Unload,
}

struct Listener(Arc<Mutex<Vec<Notice>>>);

#[mod_api::__subscriptions]
impl mod_api::Hooks for Listener {
    fn client_player_logging_out(
        &self,
        host: &mut dyn ClientHost,
        event: &mut ClientPlayerLoggingOutEvent,
    ) {
        self.0.lock().unwrap().push(Notice::Logout {
            player: event
                .player
                .map(|player| (player.entity().id(), player.uuid())),
            mode: event.game_mode,
            live_mode: event.player.and_then(|player| player.game_mode(host)),
        });
    }

    fn client_level_unload(&self, _: &mut dyn ClientHost, _: &mut ClientLevelUnloadEvent<'_>) {
        self.0.lock().unwrap().push(Notice::Unload);
    }
}

#[test]
#[ignore = "daily"]
fn logout_reports_the_live_player_before_unload_once_and_allows_no_player() {
    if run_fresh_cases(
        "game::remote::tests::client_player_logging_out_tests::logout_reports_the_live_player_before_unload_once_and_allows_no_player",
        "SUNRISE_LOGOUT_TEST_CHILD",
        &["logout"],
    ) {
        return;
    }
    let notices = Arc::new(Mutex::new(Vec::new()));
    let heard = || std::mem::take(&mut *notices.lock().unwrap());
    let mods = vec![mod_api::ModEntry::new(
        "logout_test",
        Box::new(Listener(notices.clone())),
    )]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_client_game(|game| {
            game.native_lifecycle = NativeLifecycle::default();
            let empty = || Notice::Logout {
                player: None,
                mode: None,
                live_mode: None,
            };

            // Minecraft.doWorldLoad disconnects even from an empty client.
            game.client_logging_out();
            assert_eq!(heard(), [empty()]);

            // An actual disconnect also runs without a player or controller
            // when the transport closes before it ever delivers Login.
            let (client, _server) = Pipe::pair();
            game.connect_local(client, "LogoutTest").unwrap();
            game.leave_server(None).unwrap();
            assert_eq!(heard(), [empty()]);

            let (client, _server) = Pipe::pair();
            game.connect_local(client, "LogoutTest").unwrap();
            game.remote.as_mut().unwrap().profile = 42;
            let spawn = SpawnInfo {
                dimension_type: 0,
                dimension: "minecraft:overworld".into(),
                seed: 0,
                game_type: 1,
                previous_game_type: None,
                debug: false,
                flat: true,
                last_death: None,
                portal_cooldown: 0,
                sea_level: 63,
            };
            game.handle_packet(Clientbound::Login(Login {
                player_id: 7,
                hardcore: false,
                levels: vec!["minecraft:overworld".into(), "test:other_overworld".into()],
                max_players: 20,
                chunk_radius: 8,
                simulation_distance: 8,
                reduced_debug_info: false,
                show_death_screen: true,
                limited_crafting: false,
                spawn: spawn.clone(),
                online_mode: false,
                enforces_secure_chat: false,
            }))
            .unwrap();

            // Changing dimension unloads a level, not the connection.
            game.handle_packet(Clientbound::Respawn(Respawn {
                spawn: SpawnInfo {
                    dimension: "test:other_overworld".into(),
                    ..spawn
                },
                keep: 0,
            }))
            .unwrap();
            assert_eq!(heard(), [Notice::Unload]);

            // Login already made the logical player. Sunrise has not made
            // its motion body yet (that awaits the first teleport).
            assert!(game.player.is_none());
            // The integrated save route notifies now; leave_server finishes
            // the same disconnect after the server stops.
            game.client_logging_out();
            game.leave_server(None).unwrap();
            assert_eq!(
                heard(),
                [
                    Notice::Logout {
                        player: Some((7, 42)),
                        mode: Some(GameMode::Creative),
                        live_mode: Some(GameMode::Creative)
                    },
                    Notice::Unload,
                ]
            );
            assert!(game.remote.is_none());
            assert!(game.local_player().is_none());
        });
    });
}
