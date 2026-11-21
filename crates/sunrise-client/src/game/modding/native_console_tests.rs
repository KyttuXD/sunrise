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
                let mut output = receive.into_iter();
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
                assert!(error.style.color.is_some(), "the error keeps its color");
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
