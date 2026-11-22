//! A mod's custom command argument type completing through the unified `CommandArgumentType::suggest`
//! path: the request the engine builds (prefix, prior parsed arguments, sender), parity between
//! server and client commands, and `CallbackOwner` panic containment.
use super::*;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use sunrise_client::command::{Registries, Suggestion};

/// What one `suggest` call saw: the prefix, the arguments before it, and the sender.
type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>, Option<(u128, String, u8)>)>>>;

fn seen() -> Seen {
    Default::default()
}

/// A command argument type whose suggestions inspect `request.prefix`, `request.arguments`, and
/// `request.sender`.
struct ContextWarps {
    candidates: Vec<(String, Option<String>)>,
    seen: Seen,
    panic_on_answer: bool,
    calls: Arc<AtomicUsize>,
}

impl mod_api::CommandArgumentType for ContextWarps {
    fn kind(&self) -> mod_api::CommandArgumentKind {
        mod_api::CommandArgumentKind::Word
    }

    fn parse(&self, input: &str) -> Result<String, mod_api::CommandArgumentError> {
        Ok(input.to_owned())
    }

    fn suggest(&self, request: &mod_api::SuggestionRequest) -> Vec<mod_api::Suggestion> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.seen.lock().unwrap().push((
            request.prefix.clone(),
            request
                .arguments
                .iter()
                .map(|argument| (argument.name.clone(), argument.text.clone()))
                .collect(),
            request.sender.as_ref().map(|sender| {
                (
                    sender.uuid(),
                    sender.name().to_owned(),
                    sender.permission().level(),
                )
            }),
        ));
        if self.panic_on_answer {
            panic!("the mod's completer panicked");
        }
        let world_matches = request
            .arguments
            .iter()
            .any(|arg| arg.name == "world" && arg.text == "survival");
        if !world_matches {
            return Vec::new();
        }
        self.candidates
            .iter()
            .filter(|(text, _)| text.starts_with(request.prefix.as_str()))
            .map(|(text, tooltip)| match tooltip {
                Some(tip) => request.suggest_with_tooltip(text, tip),
                None => request.suggest(text),
            })
            .collect()
    }
}

/// A simple argument type completing a fixed set of modes.
struct Plain;

impl mod_api::CommandArgumentType for Plain {
    fn kind(&self) -> mod_api::CommandArgumentKind {
        mod_api::CommandArgumentKind::Word
    }

    fn parse(&self, input: &str) -> Result<String, mod_api::CommandArgumentError> {
        Ok(input.to_owned())
    }

    fn suggest(&self, request: &mod_api::SuggestionRequest) -> Vec<mod_api::Suggestion> {
        ["survival", "creative"]
            .into_iter()
            .filter(|mode| mode.starts_with(request.prefix.as_str()))
            .map(|mode| request.suggest(mode))
            .collect()
    }
}

fn arguments(
    warps: ContextWarps,
    owner: mod_api::__private::game::CallbackOwner,
) -> CommandArguments {
    HashMap::from([
        (
            "test:warp".to_owned(),
            RegisteredArgument::owned(owner, Arc::new(warps)),
        ),
        (
            "test:mode".to_owned(),
            RegisteredArgument::builtin(Arc::new(Plain)),
        ),
    ])
}

/// The tree and dispatcher of `warp go <world:test:mode> <name:test:warp>`.
fn warp(
    warps: ContextWarps,
    owner: mod_api::__private::game::CallbackOwner,
) -> (Value, Dispatcher) {
    let arguments = arguments(warps, owner);
    let (tree, _, problems) = super::command_tree(
        &[mod_api::Command::new(
            "warp go <world:test:mode> <name:test:warp>",
        )],
        |_| false,
        &arguments,
    );
    assert!(problems.is_empty(), "{problems:?}");
    let mut dispatcher = Dispatcher::new();
    register_dispatcher(&mut dispatcher, &tree, "native_mod_command", &arguments);
    (tree, dispatcher)
}

fn guest() -> Registries {
    Registries {
        permission: 3,
        sender: Some(sunrise_client_core::registries::CommandSender {
            uuid: 0x5eed,
            name: "Simon".into(),
        }),
        ..Registries::default()
    }
}

fn texts(suggestions: &[Suggestion]) -> Vec<&str> {
    suggestions.iter().map(|s| s.text.as_str()).collect()
}

#[test]
fn a_custom_argument_receives_prefix_earlier_arguments_and_sender() {
    let seen = seen();
    let owner = mod_api::__private::game::CallbackOwner::for_mod("provider_test_server");
    let (tree, dispatcher) = warp(
        ContextWarps {
            candidates: vec![
                ("press".into(), Some("the press".into())),
                ("furnace".into(), None),
            ],
            seen: seen.clone(),
            panic_on_answer: false,
            calls: Arc::new(AtomicUsize::new(0)),
        },
        owner,
    );

    let node = &tree["children"]["warp"]["children"]["go"]["children"]["world"]["children"]["name"];
    assert_eq!(node["custom_argument"], "test:warp");
    assert_eq!(node["suggestions"], "minecraft:ask_server");
    assert_eq!(
        tree["children"]["warp"]["children"]["go"]["children"]["world"]["custom_argument"],
        "test:mode"
    );

    let typed = "warp go survival pr";
    let registries = guest();
    let (_, suggestions) = dispatcher.suggestions(
        &dispatcher.parse(typed, &registries),
        typed.len(),
        &registries,
    );
    assert_eq!(texts(&suggestions), ["press"]);
    assert_eq!(suggestions[0].tooltip.as_deref(), Some("the press"));
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [(
            "pr".to_owned(),
            vec![("world".to_owned(), "survival".to_owned())],
            Some((0x5eed, "Simon".to_owned(), 3))
        )]
    );
}

#[test]
fn a_custom_argument_reading_arguments_and_prefix_works_on_both_server_and_client_commands() {
    let seen = seen();
    let owner = mod_api::__private::game::CallbackOwner::for_mod("provider_test_both_sides");
    let arguments = arguments(
        ContextWarps {
            candidates: vec![
                ("press".into(), Some("the press".into())),
                ("furnace".into(), None),
            ],
            seen: seen.clone(),
            panic_on_answer: false,
            calls: Arc::new(AtomicUsize::new(0)),
        },
        owner,
    );
    let commands = [mod_api::Command::new(
        "warp go <world:test:mode> <name:test:warp>",
    )];

    // Client command tree accepts the same custom argument type without refusal.
    let (client_tree, _, client_problems) = super::client_command_tree(&commands, &arguments, None);
    assert!(client_problems.is_empty(), "{client_problems:?}");
    let mut chat = sunrise_client::command::vanilla::game_dispatcher().clone();
    register_dispatcher(&mut chat, &client_tree, "native_mod_command", &arguments);
    let typed = "warp go survival pr";
    let client_reg = Registries::default();
    let (_, client_suggestions) =
        chat.client_suggestions(&chat.parse(typed, &client_reg), typed.len(), &client_reg);
    assert_eq!(texts(&client_suggestions), ["press"]);
    assert_eq!(client_suggestions[0].tooltip.as_deref(), Some("the press"));

    // Server command tree builds and completes identically with the sender present.
    let (server_tree, _, server_problems) = super::command_tree(&commands, |_| false, &arguments);
    assert!(server_problems.is_empty(), "{server_problems:?}");
    let mut server = Dispatcher::new();
    register_dispatcher(&mut server, &server_tree, "native_mod_command", &arguments);
    let server_reg = guest();
    let (_, server_suggestions) =
        server.client_suggestions(&server.parse(typed, &server_reg), typed.len(), &server_reg);
    assert_eq!(texts(&server_suggestions), ["press"]);
    assert_eq!(server_suggestions[0].tooltip.as_deref(), Some("the press"));

    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [
            (
                "pr".to_owned(),
                vec![("world".to_owned(), "survival".to_owned())],
                None,
            ),
            (
                "pr".to_owned(),
                vec![("world".to_owned(), "survival".to_owned())],
                Some((0x5eed, "Simon".to_owned(), 3)),
            ),
        ]
    );
}

#[test]
fn a_panicking_completer_disables_itself_while_siblings_keep_completing() {
    let seen = seen();
    let calls = Arc::new(AtomicUsize::new(0));
    let owner = mod_api::__private::game::CallbackOwner::for_mod("provider_test_panic");
    let (_, dispatcher) = warp(
        ContextWarps {
            candidates: Vec::new(),
            seen,
            panic_on_answer: true,
            calls: calls.clone(),
        },
        owner.clone(),
    );
    let registries = guest();
    let typed = "warp go survival pr";
    let (_, suggestions) = dispatcher.suggestions(
        &dispatcher.parse(typed, &registries),
        typed.len(),
        &registries,
    );
    assert!(suggestions.is_empty(), "{suggestions:?}");
    assert!(!owner.is_enabled());
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    // Disabled after the first panic: subsequent completions do not call it again.
    let (_, suggestions) = dispatcher.suggestions(
        &dispatcher.parse(typed, &registries),
        typed.len(),
        &registries,
    );
    assert!(suggestions.is_empty());
    assert_eq!(calls.load(Ordering::Relaxed), 1);

    // Sibling argument type keeps completing normally.
    let typed = "warp go cr";
    let (_, suggestions) = dispatcher.suggestions(
        &dispatcher.parse(typed, &registries),
        typed.len(),
        &registries,
    );
    assert_eq!(texts(&suggestions), ["creative"]);
}
