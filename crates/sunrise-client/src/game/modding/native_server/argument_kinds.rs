//! The argument kinds a native command usage names: the built-in ones, each a Brigadier
//! parser, and the types mods registered, with the dispatcher type each node parses as.

use super::{ArgType, Text, TryIntoEngine};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};
use sunrise_client::command::{
    CommandError, Dispatcher, NodeKind, Range, Reader, RegistryId, args::Coordinates, snbt,
};

/// The argument kinds a native usage may name, each with a [`parser`]; `resource:<registry>`
/// and `mod_id` come on top.
pub(super) const KINDS: [&str; 18] = [
    "word",
    "text",
    "integer",
    "number",
    "boolean",
    "player",
    "entity",
    "entities",
    "players",
    "item",
    "block_pos",
    "vec3",
    "vec3_uncentered",
    "column_pos",
    "rotation",
    "dimension",
    "nbt",
    "id",
];

/// The tree property marking `vec3_uncentered`: vanilla's `vec3(false)` has the same parser id
/// and properties as `vec3()`, so the report format cannot tell them apart.
pub(super) const UNCENTERED: &str = "uncentered";

/// One custom argument type available to command trees, with its mod callback owner when
/// registered by a mod (`None` for built-in types such as `mod_id`).
#[derive(Clone)]
pub(in super::super) struct RegisteredArgument {
    owner: Option<mod_api::__private::game::CallbackOwner>,
    argument: Arc<dyn mod_api::CommandArgumentType>,
}

impl RegisteredArgument {
    /// Wrap a built-in engine-supplied argument type (no mod callback owner).
    pub(in super::super) fn builtin(argument: Arc<dyn mod_api::CommandArgumentType>) -> Self {
        Self {
            owner: None,
            argument,
        }
    }

    /// Wrap a mod-registered argument type under its shared [`CallbackOwner`].
    pub(in super::super) fn owned(
        owner: mod_api::__private::game::CallbackOwner,
        argument: Arc<dyn mod_api::CommandArgumentType>,
    ) -> Self {
        Self {
            owner: Some(owner),
            argument,
        }
    }

    pub(super) fn kind(&self) -> Result<mod_api::CommandArgumentKind, String> {
        match &self.owner {
            Some(owner) => owner.call("command_argument", "command_argument.kind", || {
                self.argument.kind()
            }),
            None => Ok(self.argument.kind()),
        }
    }

    pub(super) fn parse(
        &self,
        input: &str,
    ) -> Result<Result<String, mod_api::CommandArgumentError>, String> {
        match &self.owner {
            Some(owner) => owner.call("command_argument", "command_argument.parse", || {
                self.argument.parse(input)
            }),
            None => Ok(self.argument.parse(input)),
        }
    }

    pub(super) fn suggest(
        &self,
        request: &mod_api::SuggestionRequest,
    ) -> Result<Vec<mod_api::Suggestion>, String> {
        match &self.owner {
            Some(owner) => owner.call("command_argument", "command_argument.suggest", || {
                self.argument.suggest(request)
            }),
            None => Ok(self.argument.suggest(request)),
        }
    }
}

/// The custom argument types the mods registered, by namespaced id, for the command tree being
/// built and its server/client dispatchers.
pub(in super::super) type CommandArguments = HashMap<String, RegisteredArgument>;

/// The argument types the installed mods registered at startup, and the built-in `mod_id`:
/// NeoForge's `ModIdArgument`, one of the loaded package ids, delivered as a string.
pub(in super::super) fn command_arguments() -> CommandArguments {
    let mod_ids = mod_api::__private::loaded_mods()
        .ids()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mod_id: Arc<dyn mod_api::CommandArgumentType> =
        Arc::new(mod_api::ChoiceArgument::new(mod_ids));
    mod_api::__private::command_argument_types()
        .into_iter()
        .map(|(id, owner, argument)| (id, RegisteredArgument::owned(owner, argument)))
        .chain([(MOD_ID.to_owned(), RegisteredArgument::builtin(mod_id))])
        .collect()
}

/// The built-in kind whose value is a loaded package id. Mods' kinds are namespaced, so it
/// never meets one.
pub(super) const MOD_ID: &str = "mod_id";

/// The Brigadier parser and properties of `resource:<registry>`: `ResourceKeyArgument` on the
/// wire, so vanilla clients complete it, and `ResourceArgument` on the server, which refuses an id
/// the registry lacks before the mod runs.
pub(super) fn resource_parser(name: &str) -> Result<(&'static str, Value), String> {
    if mod_api::Identifier::parse_namespaced(name).is_none() {
        return Err(format!(
            "registry `{name}` is not a `namespace:path` resource location"
        ));
    }
    Ok(("minecraft:resource_key", json!({ "registry": name })))
}

/// What a mod receives for a custom argument: `mod_id` as the plain package id, a mod's own type
/// with its id.
pub(super) fn custom_value(kind: &str, value: String) -> mod_api::CommandArgumentValue {
    if kind == MOD_ID {
        mod_api::CommandArgumentValue::String(value)
    } else {
        mod_api::CommandArgumentValue::Custom {
            kind: kind.to_owned(),
            value,
        }
    }
}

/// Whether `word` is a usage's literal word: lowercase letters, digits, `_` and `-`. The first
/// word may also start with one `/`, so a mod can register WorldEdit-style `//set` (typed with the
/// chat slash): the node keeps the slash, and the client completes `//set ` against it.
pub(super) fn command_word(word: &str, first: bool) -> bool {
    let word = match first {
        true => word.strip_prefix('/').unwrap_or(word),
        false => word,
    };
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-".contains(c))
}

/// The brigadier parser of a built-in argument kind, its properties and its dispatcher type.
/// `EntityArgument.entity` / `entities` / `player` / `players` share the vanilla parser.
pub(super) fn parser(kind: &str) -> Option<(&'static str, Value, ArgType)> {
    Some(match kind {
        "word" => ("brigadier:string", json!({"type": "word"}), ArgType::Word),
        "text" => (
            "brigadier:string",
            json!({"type": "greedy"}),
            ArgType::Greedy,
        ),
        "integer" => (
            "brigadier:integer",
            Value::Null,
            ArgType::Integer {
                min: i32::MIN,
                max: i32::MAX,
            },
        ),
        "number" => (
            "brigadier:double",
            Value::Null,
            ArgType::Double {
                min: f64::MIN,
                max: f64::MAX,
            },
        ),
        "boolean" => ("brigadier:bool", Value::Null, ArgType::Bool),
        "entity" | "entities" | "player" | "players" => {
            let single = matches!(kind, "entity" | "player");
            let players = matches!(kind, "player" | "players");
            (
                "minecraft:entity",
                json!({
                    "amount": if single { "single" } else { "multiple" },
                    "type": if players { "players" } else { "entities" },
                }),
                ArgType::Entity { single, players },
            )
        }
        "item" => ("minecraft:item_stack", Value::Null, ArgType::Item),
        "block_pos" => ("minecraft:block_pos", Value::Null, ArgType::BlockPos),
        // `Vec3Argument.vec3()` centres whole x and z on their block, `vec3(false)` does not.
        "vec3" => (
            "minecraft:vec3",
            Value::Null,
            ArgType::Vec3 { center: true },
        ),
        "vec3_uncentered" => (
            "minecraft:vec3",
            Value::Null,
            ArgType::Vec3 { center: false },
        ),
        "column_pos" => ("minecraft:column_pos", Value::Null, ArgType::ColumnPos),
        "rotation" => ("minecraft:rotation", Value::Null, ArgType::Rotation),
        "dimension" => ("minecraft:dimension", Value::Null, ArgType::Dimension),
        "nbt" => ("minecraft:nbt_compound_tag", Value::Null, ArgType::Nbt),
        // `ResourceLocationArgument`: a free `namespace:path` of identifier characters, with no
        // registry to check. An unmodded client parses and refuses it the same way; the node
        // declares `minecraft:ask_server` (`usage_nodes`), so the identifiers it completes are the
        // mods' own (`tab_complete`).
        "id" => (
            "minecraft:resource_location",
            Value::Null,
            ArgType::Identifier,
        ),
        _ => return None,
    })
}

/// The type of the argument node that parsed `range`: the parsed node, not the value, says which
/// kind a position, id or string is.
pub(super) fn parsed_type<'a>(
    dispatcher: &'a Dispatcher,
    parsed_nodes: &[(usize, Range)],
    range: Range,
) -> Option<&'a ArgType> {
    parsed_nodes.iter().find_map(|(node, parsed)| {
        if *parsed != range {
            return None;
        }
        match &dispatcher.nodes[*node].kind {
            NodeKind::Argument { ty, .. } => Some(ty),
            _ => None,
        }
    })
}

/// What a `vec3`, `vec3_uncentered`, `column_pos`, `rotation` or `block_pos` argument delivers:
/// `position` is `coordinates` resolved against the source, `rotation` the source's yaw and pitch.
pub(super) fn positional_value(
    ty: Option<&ArgType>,
    coordinates: &Coordinates,
    position: [f64; 3],
    rotation: [f32; 2],
) -> mod_api::CommandArgumentValue {
    use mod_api::CommandArgumentValue as Value;
    match ty {
        Some(ArgType::Vec3 { .. }) => Value::Vec3(position),
        Some(ArgType::ColumnPos) => {
            Value::ColumnPos([position[0].floor() as i32, position[2].floor() as i32])
        }
        Some(ArgType::Rotation) => Value::Rotation(coordinates.rotation(rotation)),
        _ => Value::BlockPos(position.map(|v| v.floor() as i32)),
    }
}

/// `DimensionArgument.getDimension`: the level of a dimension the world has loaded, or
/// `argument.dimension.invalid`.
pub(super) fn dimension_value(
    id: &str,
    level: impl FnOnce(&str) -> Option<mod_api::LevelRef>,
) -> Result<mod_api::CommandArgumentValue, CommandError> {
    level(id)
        .map(mod_api::CommandArgumentValue::Dimension)
        .ok_or_else(|| CommandError::tr("argument.dimension.invalid", &[&id]))
}

/// An `nbt` argument's compound, from the SNBT the dispatcher accepted.
pub(super) fn nbt_value(text: &str) -> Result<mod_api::CommandArgumentValue, CommandError> {
    let mut reader = Reader::new(text);
    snbt::parse_compound_argument(&mut reader).map(mod_api::CommandArgumentValue::Nbt)
}

/// The message a mod's argument parser refused with, as the engine's text. A text the
/// boundary cannot carry is reported as its own diagnostic: the refusal still fails the
/// argument, never silently passing it.
fn refused_argument_text(error: mod_api::CommandArgumentError) -> Text {
    let message: mod_api::Text = error.into();
    match message.try_into_engine(&()) {
        Ok(text) => text,
        Err(error) => Text::literal(error.to_string()),
    }
}

/// The dispatcher type of one validated tree node: a vanilla kind, or the mod's registered
/// argument type with its parser and suggestions.
pub(super) fn argument_type(node: &Value, arguments: &CommandArguments) -> ArgType {
    if node["parser"] == "minecraft:resource_key" {
        let registry = node["properties"]["registry"].as_str().unwrap_or_default();
        return ArgType::Res(
            RegistryId::parse(registry).expect("native command tree names validated registries"),
        );
    }
    if let Some(id) = node["custom_argument"].as_str() {
        let registered = arguments
            .get(id)
            .unwrap_or_else(|| panic!("native command tree names unregistered argument type {id}"));
        let parse = registered.clone();
        let suggest = registered.clone();
        let kind_id: Arc<str> = id.into();
        let parse_id = kind_id.clone();
        let suggest_id = kind_id.clone();
        // Use the validated tree property rather than calling mod code again.
        let kind = if node["properties"]["type"] == "greedy" {
            sunrise_client::command::CustomArgumentKind::Greedy
        } else {
            sunrise_client::command::CustomArgumentKind::Word
        };
        let custom = sunrise_client::command::CustomArgument::new(
            kind_id,
            kind,
            move |input| match parse.parse(input) {
                Ok(parsed) => parsed.map_err(refused_argument_text),
                Err(why) => {
                    log::warn!("command argument `{parse_id}` parse failed: {why}");
                    Err(Text::tr("commands.generic.failed", &[]))
                }
            },
            move |reg: &sunrise_client::command::Registries,
                  prefix: &str,
                  arguments: &[sunrise_client::command::ParsedArgument]| {
                let request = mod_api::SuggestionRequest::new(
                    prefix,
                    arguments
                        .iter()
                        .map(|argument| {
                            mod_api::TypedArgument::new(
                                argument.name.clone(),
                                argument.text.clone(),
                            )
                        })
                        .collect(),
                    reg.sender.as_ref().map(|sender| {
                        mod_api::__private::game::command_sender(
                            sender.uuid,
                            sender.name.clone(),
                            mod_api::PermissionLevel::from_level(i32::from(reg.permission)),
                        )
                    }),
                );
                let results = match suggest.suggest(&request) {
                    Ok(results) => results,
                    Err(why) => {
                        log::warn!("command argument `{suggest_id}` suggest failed: {why}");
                        return Vec::new();
                    }
                };
                results
                    .into_iter()
                    .map(|suggestion| sunrise_client::command::Suggestion {
                        range: suggestion.range,
                        text: suggestion.text,
                        tooltip: suggestion.tooltip,
                        int_value: suggestion.int_value,
                    })
                    .collect()
            },
        );
        return ArgType::Custom(custom);
    }
    if node["parser"] == "minecraft:vec3" && node[UNCENTERED] == true {
        return ArgType::Vec3 { center: false };
    }
    KINDS
        .into_iter()
        .filter_map(parser)
        .find_map(|(id, properties, ty)| {
            (node["parser"] == id && node["properties"] == properties).then_some(ty)
        })
        .expect("native command tree contains registered parsers")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_kind_is_vanillas_free_identifier_parser() {
        let (parser, properties, ty) = super::parser("id").expect("the id kind has a parser");
        assert_eq!(parser, "minecraft:resource_location");
        assert_eq!(properties, Value::Null, "the parser takes no properties");
        assert!(matches!(ty, ArgType::Identifier), "{ty:?}");
        assert!(KINDS.contains(&"id"));
    }

    #[test]
    fn native_resource_arguments_own_registry_names_without_a_registration_budget() {
        for bad in [
            "a:b:c",
            "a/b:c",
            "item",
            "minecraft:",
            ":x",
            "Minecraft:x",
            "..:x",
        ] {
            let refusal = resource_parser(bad).unwrap_err();
            assert!(
                refusal.contains(bad) && refusal.contains("namespace:path"),
                "{refusal}"
            );
        }
        let arguments = CommandArguments::new();
        let names = (0..257).map(|n| format!("mymod:r{n}"));
        let long = format!("mymod:registry/{}", "b".repeat(300));
        for name in names.chain([long]) {
            let (parser, properties) = resource_parser(&name).unwrap();
            assert_eq!(parser, "minecraft:resource_key");
            let node = json!({ "parser": parser, "properties": properties });
            let ArgType::Res(registry) = argument_type(&node, &arguments) else {
                panic!("the server must check the registry for resource arguments");
            };
            drop(node);
            assert_eq!(registry.as_str(), name);
        }
    }
}
