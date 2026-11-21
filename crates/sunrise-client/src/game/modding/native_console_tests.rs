//! The console for native mods: logged lines reach `console_line`, and a command's feedback
//! comes back from `run_command_with_output`.
use super::{native_entity_refs::tests::with_game, native_guest_turn_tests::with_guests};
use mod_api::{
    Command, CommandFeedback, CommandResult, Commands, ConsoleLineEvent, Hooks,
    RegisterCommandsEvent, ServerHost, Text, text::TextContent,
};
use std::sync::{Arc, Mutex};
use sunrise_client::logging::TappedLine;

type HeardLine = (String, String, String, u64);

struct Mirror {
    overwrite: bool,
    heard: Arc<Mutex<Vec<HeardLine>>>,
}

#[mod_api::__subscriptions]
impl Hooks for Mirror {
    fn console_line(&self, _: &mut dyn ServerHost, event: &mut ConsoleLineEvent<'_>) {
        if self.overwrite {
            event.dropped = 99;
        }
        self.heard.lock().unwrap().push((
            event.level.to_owned(),
            event.thread.to_owned(),
            event.message.to_owned(),
            event.dropped,
        ));
    }
}

#[test]
#[ignore = "daily"]
fn each_console_listener_gets_the_original_overflow_count() {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let mods = vec![
        mod_api::ModEntry::new(
            "first",
            Box::new(Mirror {
                overwrite: true,
                heard: heard.clone(),
            }),
        ),
        mod_api::ModEntry::new(
            "second",
            Box::new(Mirror {
                overwrite: false,
                heard: heard.clone(),
            }),
        ),
    ]
    .into();
    mod_platform::testing::with_mods(mods, || {
        with_game(|game| {
            // The queue's own tests cover retention and overflow. Drive the hook with local
            // batches so unrelated threads cannot add lines or consume the first report.
            let line = |level, message: &str, dropped| TappedLine {
                level,
                thread: "Server thread".into(),
                message: message.into(),
                dropped,
            };
            game.dispatch_console_lines(vec![
                line(log::Level::Warn, "first in batch", 3),
                line(log::Level::Info, "later in batch", 0),
            ]);
            game.dispatch_console_lines(vec![line(log::Level::Info, "next batch", 0)]);
            let heard = heard.lock().unwrap();
            let heard: Vec<_> = heard
                .iter()
                .map(|(level, thread, message, dropped)| {
                    (level.as_str(), thread.as_str(), message.as_str(), *dropped)
                })
                .collect();
            assert_eq!(
                heard,
                [
                    ("WARN", "Server thread", "first in batch", 99),
                    ("WARN", "Server thread", "first in batch", 3),
                    ("INFO", "Server thread", "later in batch", 99),
                    ("INFO", "Server thread", "later in batch", 0),
                    ("INFO", "Server thread", "next batch", 99),
                    ("INFO", "Server thread", "next batch", 0),
                ]
            );
        });
    });
}

/// The console's feedback comes back as plain text with the result of `run_command`. A command
/// that ran and failed is `ran: false` with its message, and one leading slash is dropped.
#[test]
#[ignore = "daily"]
fn a_captured_command_returns_its_feedback() {
    with_guests(|game| {
        let result = game.native_run_command_with_output("/time query gametime");
        assert!(result.success);
        assert_eq!(result.feedback.len(), 1, "{result:?}");
        assert!(
            result.feedback[0].starts_with("The game time is"),
            "{result:?}"
        );
        let result = game.native_run_command_with_output(" time query gametime ");
        assert!(result.success && result.feedback.len() == 1, "{result:?}");
        let result = game.native_run_command_with_output("gamerule nosuchrule");
        assert!(!result.success);
        assert!(!result.feedback.is_empty());
        assert!(game.chat_capture.is_none(), "the outer capture is put back");
        // The same line through `run_command`'s path answers as the captured one does.
        assert!(game.native_run_command("time query gametime"));
        assert!(!game.native_run_command("gamerule nosuchrule"));
    });
}

/// The dedicated console's own commands are not reached: they answer as a command that failed.
#[test]
#[ignore = "daily"]
fn the_dedicated_consoles_own_commands_are_not_reached() {
    with_guests(|game| {
        for line in ["stop", "op nobody", "whitelist list", "ban nobody"] {
            let result = game.native_run_command_with_output(line);
            assert!(!result.success, "{line}: {result:?}");
            assert!(!result.feedback.is_empty(), "{line}");
        }
    });
}

/// A blank line (empty, spaces, a lone slash, a slash and spaces) and no server open to players
/// run nothing and answer as `run_command` does: `ran` false and no lines.
#[test]
#[ignore = "daily"]
fn a_blank_line_or_no_server_runs_nothing() {
    with_guests(|game| {
        for blank in ["", "   ", "/", " / ", "/   "] {
            assert_eq!(
                game.native_run_command_with_output(blank),
                CommandResult::default(),
                "{blank:?}"
            );
        }
        assert!(game.chat_capture.is_none(), "nothing was captured");
        let host = game.lan.take();
        assert_eq!(
            game.native_run_command_with_output("time query gametime"),
            CommandResult::default()
        );
        assert!(!game.native_run_command("time query gametime"));
        game.lan = host;
        assert!(
            game.native_run_command_with_output("time query gametime")
                .success
        );
    });
}

struct FeedbackCommand;

#[mod_api::__subscriptions]
impl Hooks for FeedbackCommand {
    fn register_commands(&self, _: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        event
            .commands
            .push(Command::new("feedback_probe").executes(|host, _, _| {
                let mut text = Text::literal("€".repeat(1500));
                text.style.bold = Some(true);
                host.commands().feedback(text).unwrap();
                for index in 1..300 {
                    host.commands()
                        .feedback(Text::literal(format!("line {index}")))
                        .unwrap();
                    if index == 150 {
                        let inner = host
                            .commands()
                            .run_command_with_output("time query gametime");
                        assert!(inner.success);
                        assert_eq!(inner.feedback.len(), 1);
                    }
                }
                None
            }));
    }
}

#[test]
#[ignore = "daily"]
fn native_console_delivers_all_source_feedback_in_order() {
    use sunrise_client::gamerules::{Value, vanilla::SEND_COMMAND_FEEDBACK};
    mod_platform::testing::with_mods(
        vec![mod_api::ModEntry::new(
            "feedback_probe",
            Box::new(FeedbackCommand),
        )]
        .into(),
        || {
            with_guests(|game| {
                game.build_registries();
                game.register_native_commands();
                assert!(
                    game.ui
                        .rules
                        .set("send_command_feedback", Value::Bool(false))
                );
                assert!(!game.ui.rules.bool_at(SEND_COMMAND_FEEDBACK));

                let (send, receive) = std::sync::mpsc::channel();
                assert!(!Commands::run_command_with_feedback(
                    game,
                    "/feedback_probe",
                    send
                ));
                let mut output = receive.try_iter();
                let long = "€".repeat(1500);
                let Some(Ok(CommandFeedback::Success(first))) = output.next() else {
                    panic!("expected the command's first success message");
                };
                assert_eq!(first.content, TextContent::Literal(long.clone()));
                assert_eq!(first.style.bold, Some(true));
                for index in 1..300 {
                    let Some(Ok(CommandFeedback::Success(line))) = output.next() else {
                        panic!("missing success message {index}");
                    };
                    assert_eq!(line.content, TextContent::Literal(format!("line {index}")));
                }
                let Some(Ok(CommandFeedback::Failure(error))) = output.next() else {
                    panic!("a failed command must deliver its error after earlier successes");
                };
                assert!(
                    matches!(error.style.color.as_deref(), Some("red" | "#FF5555")),
                    "the error keeps its red style"
                );
                assert_eq!(
                    output.next(),
                    None,
                    "the nested command's output stays separate"
                );

                let plain = Commands::run_command_with_output(game, "feedback_probe");
                assert!(!plain.success);
                assert_eq!(plain.feedback.len(), 301);
                assert_eq!(plain.feedback[0], long);
                assert_eq!(plain.feedback[299], "line 299");
                assert!(!plain.feedback[300].is_empty());

                use sunrise_client::{
                    lan::{Out, PlayerSender, data::component_nbt},
                    scoreboard::{comp_text, tag_to_json},
                };
                use sunrise_protocol::{buf::Reader, packets::play::Clientbound};
                let guests = [12, 13].map(|conn| {
                    let (send, receive) = std::sync::mpsc::channel();
                    game.lan
                        .as_mut()
                        .unwrap()
                        .hub
                        .update_guest(conn, |guest| {
                            guest.sender = PlayerSender::new(send, Arc::default());
                        })
                        .unwrap();
                    (conn, receive)
                });
                let (send, receive) = std::sync::mpsc::channel();
                assert!(Commands::run_command_with_feedback(
                    game,
                    "msg guest12 private hello",
                    send
                ));
                let Ok(Ok(CommandFeedback::System(outgoing))) = receive.try_recv() else {
                    panic!("the console should get an outgoing whisper confirmation");
                };
                let TextContent::Translatable { key, arguments, .. } = &outgoing.content else {
                    panic!("the outgoing whisper should remain structured");
                };
                assert_eq!(key, "commands.message.display.outgoing");
                let plain = |text| {
                    sunrise_client_lib::api_text::to_engine(text)
                        .unwrap()
                        .plain()
                };
                assert_eq!(plain(&arguments[0]), "guest12");
                assert_eq!(plain(&arguments[1]), "private hello");
                assert_eq!(
                    receive.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Disconnected)
                );
                let hub = &game.lan.as_ref().unwrap().hub;
                let chat_type = hub
                    .registry_id("minecraft:chat_type", "minecraft:msg_command_incoming")
                    .unwrap();
                for (conn, receive) in guests {
                    hub.guest(conn).unwrap().sender.flush();
                    let mut whispers = Vec::new();
                    for packet in receive.try_iter() {
                        let bodies = match packet {
                            Out::Packet(body) => vec![body],
                            Out::Packets(bodies) => bodies,
                            Out::Disconnect(_) => panic!("guest disconnected"),
                        };
                        for body in bodies {
                            let mut r = Reader::new(&body);
                            let id = r.varint().unwrap();
                            if let Clientbound::DisguisedChat(chat) =
                                Clientbound::decode(id, r.rest()).unwrap()
                            {
                                whispers.push(chat);
                            }
                        }
                    }
                    if conn == 12 {
                        assert_eq!(whispers.len(), 1);
                        assert_eq!(whispers[0].chat_type.chat_type, chat_type);
                        assert_eq!(
                            whispers[0].chat_type.name,
                            component_nbt(
                                &sunrise_client::text::Text::literal("Server").component()
                            )
                        );
                        assert_eq!(
                            comp_text(&tag_to_json(&whispers[0].message)).plain(),
                            "private hello"
                        );
                    } else {
                        assert!(
                            whispers.is_empty(),
                            "only the target gets the incoming whisper"
                        );
                    }
                }
            })
        },
    );
}

#[test]
#[ignore = "daily"]
fn native_console_dispatches_direct_commands_in_the_world_spawn_dimension() {
    use super::Dim;
    use sunrise_client::chunk::Chunk;
    use sunrise_core::position::ChunkPos;

    with_guests(|game| {
        game.build_registries();
        assert_eq!(game.world_spawn_level(), Dim::Overworld);
        assert_eq!(game.dim, Dim::Overworld);
        game.level.world_spawn = Some([5, 70, 5]);
        let add_chunk = |game: &mut super::Game<'_>| {
            game.world
                .chunks
                .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
            game.block_ticks
                .add_empty_chunk(ChunkPos::new(0, 0))
                .unwrap();
        };
        add_chunk(game);
        assert_eq!(game.enter_command_level(Dim::Nether), Some(Dim::Overworld));
        add_chunk(game);
        let before = game.world.block(5, 70, 5);
        let (send, receive) = std::sync::mpsc::channel();
        assert!(Commands::run_command_with_feedback(
            game,
            "setblock ~ ~ ~ stone",
            send
        ));
        assert!(matches!(
            receive.try_recv(),
            Ok(Ok(CommandFeedback::Success(_)))
        ));
        assert_eq!(game.dim, Dim::Nether);
        assert_eq!(game.world.block(5, 70, 5), before);
        assert!(Commands::run_command_with_output(game, "setblock ~ ~ ~1 stone").success);
        assert_eq!(game.dim, Dim::Nether);
        assert_eq!(game.world.block(5, 70, 6), before);
        assert_eq!(game.enter_command_level(Dim::Overworld), Some(Dim::Nether));
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        assert_eq!(game.world.block(5, 70, 5), stone);
        assert_eq!(game.world.block(5, 70, 6), stone);
    });
}

struct FailingGameTests;

impl mod_api::GameTestFunction for FailingGameTests {
    fn run(&self, _: &mut dyn ServerHost, _: &mod_api::GameTestContext) -> mod_api::GameTestResult {
        mod_api::GameTestResult::Fail("expected failure".into())
    }
}

#[mod_api::__subscriptions]
impl Hooks for FailingGameTests {
    fn register_game_tests(
        &self,
        _: &dyn mod_api::Host,
        event: &mut mod_api::RegisterGameTestsEvent<'_>,
    ) {
        let function = "native_console_test:fail";
        event
            .functions
            .register(function, Arc::new(FailingGameTests))
            .unwrap();
        for (id, required) in [
            ("native_console_test:required", true),
            ("native_console_test:optional", false),
        ] {
            let mut def =
                mod_api::GameTestDefinition::new("minecraft:empty".into(), function.into());
            def.required = required;
            event.functions.register_test(id, def).unwrap();
        }
    }
}

#[test]
#[ignore = "daily"]
fn native_console_keeps_the_source_attached_until_delayed_test_feedback_arrives() {
    use std::sync::mpsc::{TryRecvError, channel};
    use sunrise_client::chunk::Chunk;

    mod_platform::testing::with_mods(
        vec![mod_api::ModEntry::new(
            "native_console_test",
            Box::new(FailingGameTests),
        )]
        .into(),
        || {
            with_guests(|game| {
                game.build_registries();
                game.snapshot_test_defs();
                game.level.world_spawn = Some([8, 64, 8]);
                for cx in -1..=2 {
                    for cz in -1..=2 {
                        game.world
                            .chunks
                            .insert((cx, cz), Arc::new(Chunk::empty(cx, cz)));
                    }
                }
                game.block_ticks
                    .add_empty_chunk(sunrise_core::position::ChunkPos::new(0, 0))
                    .unwrap();
                assert!(Commands::run_command(game, "fill 0 63 0 15 63 15 stone"));
                let (send, receive) = channel();
                assert!(Commands::run_command_with_feedback(
                    game,
                    "test run native_console_test:*",
                    send
                ));
                let check_key = |text: &Text, expected| {
                    assert!(
                        matches!(&text.content, TextContent::Translatable { key, .. } if key == expected),
                        "{text:?}"
                    );
                };
                for key in ["commands.test.run.running", "commands.test.batch.starting"] {
                    let Ok(Ok(CommandFeedback::Success(text))) = receive.try_recv() else {
                        panic!("missing immediate success message {key}");
                    };
                    check_key(&text, key);
                }
                assert_eq!(
                    receive.try_recv(),
                    Err(TryRecvError::Empty),
                    "the command returned, but the runner still owns the source"
                );

                // A different command's capture must not receive the deferred summary.
                game.chat_capture = Some(Vec::new());
                for _ in 0..8 {
                    game.tick_game_tests();
                }
                let Ok(Ok(CommandFeedback::Success(summary))) = receive.try_recv() else {
                    panic!("missing delayed summary");
                };
                check_key(&summary, "commands.test.summary");
                let Ok(Ok(CommandFeedback::Failure(required))) = receive.try_recv() else {
                    panic!("missing required failure");
                };
                assert!(matches!(
                    required.style.color.as_deref(),
                    Some("red" | "#FF5555")
                ));
                check_key(&required.children[0], "commands.test.summary.failed");
                let Ok(Ok(CommandFeedback::System(optional))) = receive.try_recv() else {
                    panic!("missing optional failure");
                };
                check_key(&optional, "commands.test.summary.optional_failed");
                let Ok(Ok(CommandFeedback::Success(coordinates))) = receive.try_recv() else {
                    panic!("missing test position");
                };
                check_key(&coordinates, "chat.square_brackets");
                assert_eq!(receive.try_recv(), Err(TryRecvError::Disconnected));
                assert!(game.chat_capture.take().unwrap().is_empty());
            })
        },
    );
}

/// A call made while another capture is open captures its own lines, and the outer capture
/// keeps what it had and goes on.
#[test]
#[ignore = "daily"]
fn a_nested_capture_leaves_the_outer_one_as_it_was() {
    with_guests(|game| {
        let outer = game
            .chat_capture
            .replace(vec![sunrise_client::text::Text::literal("outer line")]);
        let result = game.native_run_command_with_output("time query gametime");
        assert!(result.success && result.feedback.len() == 1, "{result:?}");
        let kept: Vec<String> = game
            .chat_capture
            .take()
            .unwrap()
            .iter()
            .map(|t| t.plain())
            .collect();
        assert_eq!(kept, ["outer line"]);
        game.chat_capture = outer;
    });
}
