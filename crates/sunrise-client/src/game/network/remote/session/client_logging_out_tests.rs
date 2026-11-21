use crate::game::input_trace::InputOrigin;
use crate::game::native_entity_refs::tests::{run_fresh_cases, with_client_game};
use crate::game::remote::tests::connect_menu_test;
use crate::game::{EventLoopCtl, Game};
use mod_api::{
    ClientHost, ClientLevelUnloadEvent, ClientPlayerLoggingOutEvent, GameMode, Hooks, ModEntry,
    PlayerRef,
};
use mod_platform::testing::with_mods;
use std::cell::Cell;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use sunrise_client::title::Action;
use sunrise_core::nbt::Tag;
use sunrise_protocol::{
    connection::{Connection, Pipe},
    packets::{
        Empty, configuration as cfg, login,
        play::{self, Clientbound as C, SpawnInfo},
    },
};
use winit::{event::WindowEvent, event_loop::ControlFlow, window::WindowId};

const OVERWORLD: &str = "minecraft:overworld";
const NETHER: &str = "minecraft:the_nether";

#[derive(Debug, PartialEq, Eq)]
enum Observed {
    Logout {
        player: Option<PlayerRef>,
        game_mode: Option<GameMode>,
        local_player: Option<PlayerRef>,
        host_game_mode: Option<GameMode>,
        level_live: bool,
    },
    LevelUnload(String),
}

const NO_PLAYER_LOGOUT: Observed = Observed::Logout {
    player: None,
    game_mode: None,
    local_player: None,
    host_game_mode: None,
    level_live: false,
};

#[derive(Clone)]
struct Probe {
    events: Arc<Mutex<Vec<Observed>>>,
    expected_dimension: Arc<Mutex<&'static str>>,
}

impl Probe {
    fn take(&self) -> Vec<Observed> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

#[mod_api::__subscriptions]
impl Hooks for Probe {
    fn client_player_logging_out(
        &self,
        host: &mut dyn ClientHost,
        event: &mut ClientPlayerLoggingOutEvent,
    ) {
        let dim = *self.expected_dimension.lock().unwrap();
        let local_player = host.local_player();
        let host_game_mode = event.player.and_then(|player| player.game_mode(&*host));
        let level_live = host.level(dim).is_some();
        self.events.lock().unwrap().push(Observed::Logout {
            player: event.player,
            game_mode: event.game_mode,
            local_player,
            host_game_mode,
            level_live,
        });
    }

    fn client_level_unload(
        &self,
        _host: &mut dyn ClientHost,
        event: &mut ClientLevelUnloadEvent<'_>,
    ) {
        self.events
            .lock()
            .unwrap()
            .push(Observed::LevelUnload(event.dimension.to_owned()));
    }
}

struct AlterLogoutEvent;

#[mod_api::__subscriptions]
impl Hooks for AlterLogoutEvent {
    fn client_player_logging_out(
        &self,
        _: &mut dyn ClientHost,
        event: &mut ClientPlayerLoggingOutEvent,
    ) {
        event.player = None;
        event.game_mode = None;
    }
}

fn with_logout_probe(test: impl FnOnce(&mut Game<'_>, &Probe)) {
    let probe = Probe {
        events: Arc::new(Mutex::new(Vec::new())),
        expected_dimension: Arc::new(Mutex::new(OVERWORLD)),
    };
    // The first listener may edit its event but cannot change what the next listener sees.
    let mods: mod_api::Mods = vec![
        ModEntry::new("alter_logout_event", Box::new(AlterLogoutEvent)),
        ModEntry::new("logout_probe", Box::new(probe.clone())),
    ]
    .into();
    with_mods(mods, || with_client_game(|game| test(game, &probe)));
}

fn poll_remote_until(game: &mut Game<'_>, description: &str, done: impl Fn(&Game<'_>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(game) {
        assert!(Instant::now() < deadline, "waiting for {description}");
        game.poll_remote_events(false).unwrap();
        std::thread::yield_now();
    }
}

#[derive(Default)]
struct TestLoop(Cell<bool>);

impl EventLoopCtl for TestLoop {
    fn exit(&self) {
        self.0.set(true);
    }

    fn set_control_flow(&self, _: ControlFlow) {}
}

#[test]
#[ignore = "daily"]
fn quitting_from_the_title_logs_out_once() {
    const CASE: &str = "SUNRISE_TEST_TITLE_LOGOUT";
    if run_fresh_cases(
        "game::remote::session::client_logging_out_tests::quitting_from_the_title_logs_out_once",
        CASE,
        &["button", "window-close"],
    ) {
        return;
    }
    with_logout_probe(|game, probe| {
        game.show_title(game.new_title("saves".into()));
        let event_loop = TestLoop::default();
        match std::env::var(CASE).as_deref() {
            Ok("button") => {
                game.title_action(&event_loop, Action::Quit).unwrap();
                assert!(
                    probe.take().is_empty(),
                    "the title button only stops the loop"
                );
                assert!(event_loop.0.get());
                game.shut_down();
                assert_eq!(probe.take(), [NO_PLAYER_LOGOUT]);
            }
            Ok("window-close") => {
                game.options.headless = true;
                game.handle_window_event(
                    &event_loop,
                    WindowId::dummy(),
                    WindowEvent::CloseRequested,
                    InputOrigin::Game,
                );
                assert_eq!(probe.take(), [NO_PLAYER_LOGOUT]);
                game.idle(&event_loop);
                assert!(event_loop.0.get(), "the asynchronous close finished");
                game.shut_down();
                assert!(probe.take().is_empty(), "shutdown did not log out again");
            }
            other => panic!("unexpected title quit case: {other:?}"),
        }
    });
}

fn begin_login(game: &mut Game<'_>) -> Connection {
    let (client, server) = Pipe::pair();
    game.connect_local(client, "LogoutTest").unwrap();
    let mut server = Connection::local(server);
    server
        .reader
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    server.recv().unwrap(); // Intention
    server.recv().unwrap(); // Hello
    server
}

fn configuration_disconnect() -> Vec<u8> {
    cfg::Clientbound::Disconnect(cfg::Disconnect {
        reason: Tag::String("Configuration closed".into()),
    })
    .encode()
}

#[test]
#[ignore = "daily"]
fn cancel_and_handshake_failures_do_not_log_out_but_configuration_disconnects_do() {
    if run_fresh_cases(
        "game::remote::session::client_logging_out_tests::cancel_and_handshake_failures_do_not_log_out_but_configuration_disconnects_do",
        "SUNRISE_TEST_ATTEMPT_LOGOUT",
        &["attempt"],
    ) {
        return;
    }
    with_logout_probe(|game, probe| {
        // An invalid address exercises an attempt that never opens a socket.
        game.show_title(game.new_title("saves".into()));
        game.connect_server("", "LogoutTest").unwrap();
        assert_eq!(
            probe.take(),
            [NO_PLAYER_LOGOUT],
            "ConnectScreen starts with a disconnect"
        );
        let (w, h) = game.gui_size();
        let (x, y) = sunrise_client::level_loading::connect_cancel(w, h);
        game.cursor = (x + 100.0, y + 10.0);
        game.connect_screen_click().unwrap();
        assert!(game.remote.is_none(), "Cancel closed the attempt");
        assert!(
            probe.take().is_empty(),
            "Cancel did not disconnect a client level"
        );

        game.connect_server("", "LogoutTest").unwrap();
        assert_eq!(probe.take(), [NO_PLAYER_LOGOUT]);
        assert!(game.remote.is_some());
        poll_remote_until(game, "connection setup failure", |game| {
            game.remote.is_none()
        });
        assert!(
            probe.take().is_empty(),
            "the attempt failed before a socket opened"
        );

        let mut server = begin_login(game);
        server
            .send(
                &login::Clientbound::Disconnect(login::Disconnect {
                    reason_json: r#"{"text":"Login refused"}"#.into(),
                })
                .encode(),
            )
            .unwrap();
        poll_remote_until(game, "handshake disconnect", |game| game.remote.is_none());
        assert!(
            probe.take().is_empty(),
            "the login listener only changes screens"
        );

        let mut server = begin_login(game);
        server
            .send(
                &login::Clientbound::LoginFinished(login::LoginFinished {
                    profile: login::GameProfile {
                        uuid: 42,
                        name: "LogoutTest".into(),
                        properties: vec![],
                    },
                    session: 1,
                })
                .encode(),
            )
            .unwrap();
        server.recv().unwrap(); // Login acknowledged
        server.recv().unwrap(); // Brand
        server.recv().unwrap(); // Client information
        server.send(&configuration_disconnect()).unwrap();
        poll_remote_until(game, "initial configuration disconnect", |game| {
            game.remote.is_none()
        });
        assert_eq!(probe.take(), [NO_PLAYER_LOGOUT]);
    });
}

fn spawn_info(dimension: &str, game_mode: GameMode, sea_level: i32) -> SpawnInfo {
    SpawnInfo {
        dimension_type: 0,
        dimension: dimension.into(),
        seed: 0,
        game_type: game_mode.id(),
        previous_game_type: None,
        debug: false,
        flat: true,
        last_death: None,
        portal_cooldown: 0,
        sea_level,
    }
}

fn enter_overworld(game: &mut Game<'_>) -> sunrise_protocol::connection::ConnWriter {
    let (_reader, mut writer) = connect_menu_test(game);
    writer
        .send(
            &C::Login(play::Login {
                player_id: 42,
                hardcore: false,
                levels: vec![OVERWORLD.into(), NETHER.into()],
                max_players: 1,
                chunk_radius: 8,
                simulation_distance: 8,
                reduced_debug_info: false,
                show_death_screen: true,
                limited_crafting: false,
                spawn: spawn_info(OVERWORLD, GameMode::Survival, 63),
                online_mode: false,
                enforces_secure_chat: false,
            })
            .encode(),
        )
        .unwrap();
    poll_remote_until(game, "play login", |game| game.local_player().is_some());
    writer
}

#[test]
#[ignore = "daily"]
fn client_player_logging_out_fires_once_before_teardown_and_handles_no_player_transitions() {
    // The real login and respawn packets set process-global sea levels.
    if run_fresh_cases(
        "game::remote::session::client_logging_out_tests::client_player_logging_out_fires_once_before_teardown_and_handles_no_player_transitions",
        "SUNRISE_TEST_PLAY_LOGOUT",
        &["play"],
    ) {
        return;
    }
    with_logout_probe(|game, probe| {
        let mut writer = enter_overworld(game);
        assert!(probe.take().is_empty());
        *probe.expected_dimension.lock().unwrap() = NETHER;
        writer
            .send(
                &C::Respawn(play::Respawn {
                    spawn: spawn_info(NETHER, GameMode::Creative, 32),
                    keep: 0,
                })
                .encode(),
            )
            .unwrap();
        poll_remote_until(game, "Nether respawn", |game| {
            game.remote
                .as_ref()
                .is_some_and(|remote| remote.level == NETHER)
        });
        assert_eq!(probe.take(), [Observed::LevelUnload(OVERWORLD.into())]);

        let live_player = game.local_player().expect("logged-in local player");
        game.leave_server(None).unwrap();
        assert_eq!(
            probe.take(),
            [
                Observed::Logout {
                    player: Some(live_player),
                    game_mode: Some(GameMode::Creative),
                    local_player: Some(live_player),
                    host_game_mode: Some(GameMode::Creative),
                    level_live: true,
                },
                Observed::LevelUnload(NETHER.into()),
            ]
        );
        assert!(game.local_player().is_none());
        assert_eq!(live_player.game_mode(game), None);

        *probe.expected_dimension.lock().unwrap() = OVERWORLD;
        let mut writer = enter_overworld(game);
        assert!(probe.take().is_empty());
        writer.send(&C::StartConfiguration(Empty).encode()).unwrap();
        poll_remote_until(game, "start reconfiguration", |game| {
            game.remote
                .as_ref()
                .is_some_and(|remote| remote.reconfiguring.is_some())
        });
        assert_eq!(probe.take(), [Observed::LevelUnload(OVERWORLD.into())]);
        assert!(
            game.local_player().is_none(),
            "reconfiguration retired the player epoch"
        );

        writer.send(&configuration_disconnect()).unwrap();
        poll_remote_until(game, "reconfiguration disconnect", |game| {
            game.remote.is_none()
        });
        assert_eq!(probe.take(), [NO_PLAYER_LOGOUT]);
    });
}
