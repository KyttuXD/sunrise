//! Differential witness for tools/test_server_player_packets_parity.py. Packet bodies enter
//! the server's ordinary inbox and run through the real Hub and Game dispatch.
use super::*;
use crate::game::{Dim, execution_role::ExecutionRole, native_entity_refs::tests};
use serde_json::{Map, Value, json};
use std::sync::{atomic::AtomicI32, mpsc};
use sunrise_client::{
    chunk::Chunk,
    lan::{
        Joined, Out, PlayerSender,
        guest::{Guest, InboxItem},
    },
};
use sunrise_protocol::{
    buf::Reader,
    packets::{configuration::ClientInformation, play::Serverbound as S},
};

const VIEWER: u32 = 1;
const START: [f64; 3] = [0.5, 100.0, 0.5];
const REPORT: &str = "SUNRISE_PLAYER_PACKET_REPORT";
const CHILD: &str = "SUNRISE_PLAYER_PACKET_CHILD";

fn join(game: &mut Game<'_>, conn: u32, loaded: bool) -> mpsc::Receiver<Out> {
    let (tx, rx) = mpsc::channel();
    let id = 1000 + conn as i32;
    game.next_entity = game.next_entity.max(id as u32 + 1);
    let mut guest = Guest::new(
        Joined {
            conn,
            uuid: u128::from(conn),
            name: format!("PacketPlayer{conn}"),
            properties: Vec::new(),
            info: ClientInformation::new(2),
            brand: "fixture".into(),
            sender: PlayerSender::new(tx, Arc::default()),
            latency: Arc::new(AtomicI32::new(0)),
            address: "127.0.0.1:1".parse().unwrap(),
            expected: false,
        },
        id,
        START,
        2,
    );
    guest.dimension = game.dim.id().to_owned();
    guest.placed = true;
    guest.loaded = loaded;
    guest.load_timeout = if loaded { 0 } else { 60 };
    (guest.yaw, guest.pitch) = (17.0, -12.0);
    game.lan.as_mut().unwrap().hub.guests.push(guest);
    game.reset_entity_residency();
    rx
}

fn packet_tick(game: &mut Game<'_>, conn: u32, packets: impl IntoIterator<Item = S>) {
    game.lan.as_mut().unwrap().hub.update_guest(conn, |guest| {
        guest.inbox.extend(packets.into_iter().map(|packet| {
            let body = packet.encode();
            let mut reader = Reader::new(&body);
            let id = reader.varint().unwrap();
            InboxItem::Packet(S::decode(id, reader.rest()).unwrap(), None)
        }));
    });
    game.level_pass = Default::default();
    game.tick_lan_packets();
    guest(game, conn).sender.flush();
}

fn guest<'a>(game: &'a Game<'_>, conn: u32) -> &'a Guest {
    game.lan.as_ref().unwrap().hub.guest(conn).unwrap()
}

fn pose(guest: &Guest) -> Value {
    let [x, y, z] = guest.position.map(f64::to_bits);
    json!([
        x,
        y,
        z,
        guest.yaw.to_bits() as i32,
        guest.pitch.to_bits() as i32
    ])
}

fn receive(rx: &mpsc::Receiver<Out>) -> Vec<C> {
    rx.try_iter()
        .flat_map(|packet| match packet {
            Out::Packet(body) => vec![body],
            Out::Packets(bodies) => bodies,
            Out::Disconnect(_) => Vec::new(),
        })
        .map(|body| {
            let mut reader = Reader::new(&body);
            let id = reader.varint().unwrap();
            C::decode(id, reader.rest()).unwrap()
        })
        .collect()
}

fn credits(game: &mut Game<'_>, rows: &mut Map<String, Value>) {
    game.enter_level(Dim::End).unwrap();
    game.streaming
        .insert_remote(&mut game.world, Chunk::empty(0, 0));
    assert!(
        game.set_state_named([0, 100, 0], "minecraft:end_portal", &[])
            .unwrap()
    );
    let rx = join(game, 7, true);
    game.handle_server_portal(7);
    assert!(
        receive(&rx)
            .iter()
            .any(|p| matches!(p, C::GameEvent(event) if event.event == 4))
    );
    packet_tick(
        game,
        7,
        [S::MovePlayerPosRot(play::MovePlayerPosRot {
            x: 0.75,
            y: 100.0,
            z: 0.5,
            yaw: 60.0,
            pitch: 30.0,
            flags: 1,
        })],
    );
    rows.insert("141/credits_pose".into(), pose(guest(game, 7)));
    rows.insert(
        "141/credits_move_count".into(),
        json!(guest(game, 7).received_moves),
    );
    packet_tick(
        game,
        7,
        [S::MovePlayerPos(play::MovePlayerPos {
            x: 0.75,
            y: 100.0,
            z: 0.5,
            flags: 1,
        })],
    );
    rows.insert(
        "141/repeated_position".into(),
        json!(rx.try_iter().any(|p| matches!(p, Out::Disconnect(_)))),
    );

    let rx = join(game, 8, true);
    game.handle_server_portal(8);
    packet_tick(
        game,
        8,
        [S::MovePlayerRot(play::MovePlayerRot {
            yaw: f32::NAN,
            pitch: 0.0,
            flags: 1,
        })],
    );
    rows.insert(
        "141/invalid_rotation".into(),
        json!(rx.try_iter().any(|p| matches!(p, Out::Disconnect(_)))),
    );
    game.enter_level(Dim::Overworld).unwrap();
}

fn use_item(game: &mut Game<'_>, rows: &mut Map<String, Value>) {
    for kind in ["empty", "disabled", "survival", "spectator"] {
        game.lan.as_mut().unwrap().hub.update_guest(VIEWER, |g| {
            (g.yaw, g.pitch) = (17.0, -12.0);
            g.last_action_tick = 0;
            g.ack_up_to.set(-1);
            g.game_mode = if kind == "spectator" { 3 } else { 0 };
            g.offhand =
                (kind != "empty").then(|| crate::items::ItemStack::new("minecraft:snowball", 3));
        });
        let saved = game.level.enabled_features.clone();
        if kind == "disabled" {
            game.level.enabled_features = Vec::<String>::new().into();
        }
        packet_tick(
            game,
            VIEWER,
            [S::UseItem(play::UseItem {
                hand: 1,
                sequence: 11,
                yaw: 725.0,
                pitch: -485.0,
            })],
        );
        let g = guest(game, VIEWER);
        rows.insert(
            format!("142/{kind}"),
            json!([
                g.yaw.to_bits() as i32,
                g.pitch.to_bits() as i32,
                g.offhand.as_ref().map_or(0, |s| s.count),
                g.ack_up_to.get(),
                i32::from(g.last_action_tick != 0)
            ]),
        );
        game.level.enabled_features = saved;
    }
    game.lan
        .as_mut()
        .unwrap()
        .hub
        .update_guest(VIEWER, |g| g.game_mode = 0);
}

fn input(shift: bool) -> S {
    S::PlayerInput(play::PlayerInput {
        flags: if shift { 32 } else { 0 },
    })
}

fn interact(game: &mut Game<'_>, rows: &mut Map<String, Value>) {
    let interact = |id, secondary| {
        S::Interact(play::Interact {
            id,
            hand: 0,
            location: [0.0; 3],
            secondary,
        })
    };
    for shift in [true, false] {
        packet_tick(game, VIEWER, [input(!shift), interact(-1, shift)]);
        rows.insert(
            format!("143/missing_{shift}"),
            json!(guest(game, VIEWER).sneaking),
        );
    }
    for (conn, shift) in [(3, true), (4, false)] {
        let _out = join(game, conn, true);
        assert!(game.as_console(|g| g.run_command("summon oak_boat 1.5 100 0.5")));
        let boat = game.boats.last().unwrap().id;
        packet_tick(
            game,
            conn,
            [input(!shift), interact(boat as i32, shift), input(!shift)],
        );
        rows.insert(
            format!("143/boat_{shift}"),
            json!(guest(game, conn).vehicle == Some(boat)),
        );
        if !shift {
            rows.insert("143/next_input".into(), json!(guest(game, conn).sneaking));
        }
    }
}

fn spectate(game: &mut Game<'_>, rx: &mpsc::Receiver<Out>, rows: &mut Map<String, Value>) {
    let shoot = |uuid| S::TeleportToEntity(play::TeleportToEntity { uuid });
    let _peer = join(game, 2, true);
    let host = game.lan.as_mut().unwrap();
    host.hub.update_guest(VIEWER, |g| g.game_mode = 3);
    host.hub.update_guest(2, |g| {
        g.place([10.5, 104.0, 7.5]);
        (g.yaw, g.pitch) = (-35.0, 20.0);
    });
    packet_tick(game, VIEWER, [shoot(2)]);
    rows.insert("144/player".into(), pose(guest(game, VIEWER)));
    receive(rx);
    packet_tick(game, VIEWER, [shoot((1u128 << 64) | 1)]);
    rows.insert(
        "144/missing".into(),
        json!(
            receive(rx)
                .iter()
                .filter(|p| matches!(p, C::PlayerPosition(_)))
                .count()
        ),
    );
    for (word, dim, name, x) in [
        (9u128, Dim::Overworld, "overworld", 6.25),
        (10, Dim::Nether, "the_nether", 8.25),
    ] {
        let from = game.dim;
        game.enter_level(dim).unwrap();
        assert!(game.as_console(|g| g.run_command(&format!(
            "summon armor_stand {x} 101 2.75 {{UUID:[I;0,0,0,{word}],Rotation:[63f,0f]}}"
        ))));
        game.enter_level(from).unwrap();
        packet_tick(game, VIEWER, [shoot(word)]);
        game.run_level_teleports();
        let viewer = guest(game, VIEWER);
        rows.insert(format!("144/stand_{name}"), pose(viewer));
        rows.insert(
            format!("144/level_{name}"),
            json!(viewer.dimension == dim.id()),
        );
    }
}

fn resend(game: &mut Game<'_>, rows: &mut Map<String, Value>) {
    for (n, route) in ["vehicle", "player", "loading_vehicle", "loading_player"]
        .into_iter()
        .enumerate()
    {
        let conn = 20 + n as u32;
        let rx = join(game, conn, !route.starts_with("loading_"));
        let i = game.guest_index(conn).unwrap();
        game.lan.as_mut().unwrap().hub.teleport(i, Some(START));
        for elapsed in 1..=22 {
            let mut packet = None;
            if elapsed >= 19 {
                packet = Some(if route.ends_with("vehicle") {
                    S::MoveVehicle(play::MoveVehicleRequest {
                        position: [2.0, 101.0, 3.0],
                        yaw: 72.0,
                        pitch: 35.0,
                        on_ground: true,
                    })
                } else {
                    S::MovePlayerRot(play::MovePlayerRot {
                        yaw: 72.0,
                        pitch: 35.0,
                        flags: 1,
                    })
                });
            }
            receive(&rx);
            packet_tick(game, conn, packet);
            let packets = receive(&rx);
            if elapsed >= 19 {
                let positions: Vec<_> = packets
                    .iter()
                    .filter_map(|packet| match packet {
                        C::PlayerPosition(p) => Some(p),
                        _ => None,
                    })
                    .collect();
                rows.insert(
                    format!("145/{route}_{elapsed}"),
                    json!([
                        positions.len(),
                        positions.first().map_or(-1, |p| p.teleport),
                        positions.first().map_or(0, |p| p.yaw.to_bits() as i32),
                        guest(game, conn).yaw.to_bits() as i32
                    ]),
                );
            }
        }
    }
}

#[test]
#[ignore = "daily"]
fn server_player_packet_witness() {
    // Resource loading installs process-wide registries. The fixture owns a fresh process.
    if std::env::var_os(CHILD).is_none() {
        let (_, module) = module_path!().split_once("::").unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg(format!("{module}::server_player_packet_witness"))
            .args(["--ignored", "--nocapture"])
            .env(CHILD, "1")
            .env("RUST_MIN_STACK", "16777216")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    tests::with_game_role(
        tests::fixture_root(),
        ExecutionRole::DedicatedServer,
        |game| {
            game.options.dedicated = Some(Vec::new());
            game.build_registries();
            game.lan_host(
                LanSettings {
                    port: 0,
                    guest_commands: true,
                    force_game_mode: false,
                },
                NetConfig {
                    listen: false,
                    announce: false,
                    lod_distance: 0,
                    ..NetConfig::lan(0, String::new(), 16)
                },
                HubConfig {
                    default_game_mode: 0,
                    guest_commands: true,
                    force_game_mode: false,
                    hardcore: false,
                    view_distance: 2,
                    simulation_distance: 2,
                    dimension: Dim::Overworld.id().to_owned(),
                    allow_flight: true,
                    seed: 0,
                    max_players: 16,
                    integrated: false,
                    levels: game.level_shapes,
                    level_seeds: game.levels.runtime.seeds(),
                },
            )
            .unwrap();
            game.streaming
                .insert_remote(&mut game.world, Chunk::empty(0, 0));
            let rx = join(game, VIEWER, true);
            let mut rows = Map::new();
            credits(game, &mut rows);
            use_item(game, &mut rows);
            interact(game, &mut rows);
            spectate(game, &rx, &mut rows);
            resend(game, &mut rows);
            let report = serde_json::to_vec_pretty(&rows).unwrap();
            if let Some(path) = std::env::var_os(REPORT) {
                std::fs::write(path, report).unwrap();
            } else {
                // The differential runner checks every row against the jar; the independent daily
                // shard also checks the validation and missing-target controls without Java.
                assert_eq!(rows["141/repeated_position"], true);
                assert_eq!(rows["141/invalid_rotation"], true);
                assert_eq!(rows["144/missing"], 0);
            }
        },
    );
}
