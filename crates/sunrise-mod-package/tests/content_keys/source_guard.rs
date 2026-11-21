//! Audit Serde constructs whose input schema is not necessarily exposed to
//! the probe. The key includes the source file, item and attribute arguments,
//! so a second use of a known workaround still needs its own evidence.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

// Files defining workspace types used in declaration inputs, and the paths by which
// the reader names them (including re-exports). Registry crates such as semver,
// serde_json and toml are out of scope.
#[rustfmt::skip]
const EXTERNAL_DECLARATIONS: &[(&str, &[&str])] = &[
    ("../sunrise-core/src/arm_pose.rs", &["sunrise_core::arm_pose::ArmPose"]),
    ("../sunrise-mod-vocab/src/content/package_side.rs", &["mod_vocab::Side"]),
    ("../sunrise-mod-vocab/src/world/package_details.rs", &["mod_vocab::DependencyOrder", "mod_vocab::DependencyOrder::After"]),
    // The field's custom deserializer maps names to this vocabulary instead of deriving Serde.
    ("../sunrise-mod-vocab/src/client/types.rs", &["mod_vocab::NoteInstrument", "mod_vocab::NoteInstrument::ALL"]),
];

#[rustfmt::skip]
const NOT_DECLARATIONS: &[(&str, &str)] = &[
    ("mod_vocab::__private::DeclarationKind", "labels validation errors"),
    ("mod_vocab::DeclarationError", "reports validation errors"),
    ("mod_vocab::wire", "typed payload codec, not Serde"),
    ("mod_surface::manifest::MANIFEST", "name of the published manifest"),
    ("mod_surface::manifest::MAX_MANIFEST", "whole-manifest size boundary"),
    ("mod_surface::manifest::read", "bounded UTF-8 read, not a schema"),
    ("mod_surface::uses::USES_FILE", "name of native fit metadata"),
    ("mod_surface::uses::MAX_USES_BYTES", "size limit for native fit metadata"),
    ("sunrise_mod_api_macros::Wire", "derive for the typed payload codec, not Serde"),
    ("sunrise_assets::blockstate::BlockStateFile", "asset reference check, not a declaration"),
    ("sunrise_assets::blockstate::VariantSet", "asset reference check, not a declaration"),
    ("sunrise_assets::item_model::ClientItem", "asset reference check, not a declaration"),
    ("sunrise_assets::model::CuboidModel", "asset reference check, not a declaration"),
    ("sunrise_assets::model::Loader", "asset reference check, not a declaration"),
    ("sunrise_assets::model::TextureSlot", "asset reference check, not a declaration"),
];

type Use<'a> = (&'a str, &'a str, &'a str);

#[rustfmt::skip]
const REVIEWED: &[(Use<'static>, &[&str])] = &[
    (("content.rs", "push_reaction", "deserialize_with = \"deserialize_push_reaction\""), &["block_full", "refused_block_bad_push_reaction"]),
    (("content.rs", "offset", "deserialize_with = \"deserialize_offset\""), &["block_full", "refused_block_bad_offset"]),
    (("content.rs", "max_horizontal_offset", "deserialize_with = \"deserialize_horizontal_offset\""), &["block_full", "refused_block_horizontal_offset_range"]),
    (("content.rs", "max_vertical_offset", "deserialize_with = \"deserialize_vertical_offset\""), &["block_full", "refused_block_vertical_offset_range"]),
    (("content.rs", "instrument", "deserialize_with = \"deserialize_instrument\""), &["block_full", "refused_block_bad_instrument"]),
    (("content.rs", "ignited_by_lava", "deserialize_with = \"deserialize_ignited_by_lava\""), &["block_full", "refused_block_bad_property_type"]),
    (("content.rs", "replaceable", "deserialize_with = \"deserialize_replaceable\""), &["block_full"]),
    (("content.rs", "skip_same_block_faces", "deserialize_with = \"deserialize_skip_same_block_faces\""), &["block_full"]),
    (("content.rs", "RandomTicks", "untagged"), &["block_full"]),
    (("content.rs", "BlockShapes", "impl Deserialize"), &["block_full", "keys_block_forms", "refused_block_shape_bounds"]),
    (("content.rs", "StateProperty", "tag = \"type\""), &["block_full"]),
    (("content.rs", "open_sound", "deserialize_with = \"GateSound::deserialize_open\""), &["block_full", "keys_block_forms"]),
    (("content.rs", "close_sound", "deserialize_with = \"GateSound::deserialize_close\""), &["block_full", "keys_block_forms"]),
    (("content.rs", "duration", "deserialize_with = \"deserialize_stew_duration\""), &["block_full"]),
    (("content.rs", "GateSound", "untagged"), &["block_full", "keys_block_forms", "refused_block_gate_sound_site"]),
    (("content/entity_sounds.rs", "SoundOverride", "untagged"), &["entity_type_full", "keys_entity_sounds"]),
    (("content/entity_type.rs", "allowed_in_peaceful", "deserialize_with = \"deserialize_peaceful_setting\""), &["entity_type_full", "refused_entity_type_thrown_peaceful", "refused_entity_type_null_allowed_in_peaceful"]),
    (("content/entity_type.rs", "shadow_radius", "deserialize_with = \"deserialize_shadow\""), &["entity_type_full", "refused_entity_type_negative_shadow", "refused_entity_type_null_shadow_radius"]),
    (("content/entity_type.rs", "shadow_strength", "deserialize_with = \"deserialize_shadow\""), &["entity_type_full", "refused_entity_type_null_shadow_strength"]),
    (("content/entity_type.rs", "passenger_attachments", "deserialize_with = \"deserialize_passenger_attachments\""), &["entity_type_full", "refused_entity_type_bad_attachments", "refused_entity_type_attachment_length", "refused_entity_type_attachment_type"]),
    (("manifest.rs", "version", "deserialize_with = \"version_from_str\""), &["manifest_full", "manifest_minimal", "refused_manifest_bad_version"]),
    (("manifest.rs", "Property", "untagged"), &["manifest_full", "refused_manifest_bad_property_type"]),
    (("manifest/dependency.rs", "Dependency", "impl Deserialize"), &["manifest_full", "refused_manifest_unknown_dependency_key", "src/dependencies.rs:positional_dependency_policies_keep_their_fields_and_defaults"]),
    (("manifest/dependency.rs", "url", "deserialize_with = \"https_url\""), &["manifest_full", "src/dependencies.rs:policy_schema_errors_are_refused_when_the_manifest_is_parsed"]),
    (("manifest/rule.rs", "Rule", "impl Deserialize"), &["manifest_full", "keys_manifest_forms", "refused_manifest_unknown_rule_key", "src/dependencies.rs:positional_dependency_policies_keep_their_fields_and_defaults"]),
    (("manifest/rule.rs", "url", "deserialize_with = \"https_url\""), &["manifest_full", "refused_manifest_bad_rule_url", "src/dependencies.rs:policy_schema_errors_are_refused_when_the_manifest_is_parsed"]),
    (("manifest/jar.rs", "JarRef", "try_from = \"Fields\""), &["manifest_full", "refused_manifest_bad_jar_id", "refused_manifest_bad_jar_hash", "refused_manifest_empty_jar_version"]),
    (("manifest/runtime_files.rs", "RuntimeTable", "try_from = \"BTreeMap<String, String>\""), &["refused_manifest_runtime_on_content", "refused_manifest_empty_runtime", "refused_manifest_bad_runtime_hash", "refused_manifest_bad_runtime_path"]),
    // Not content declarations. Name the reader's unit-test fixtures instead of
    // admitting exact-build native mods or cached root metadata into this corpus.
    (("manifest/jar.rs", "JarRequirement", "try_from = \"RequirementFields\""), &["src/manifest/jar.rs:decoded_root_requirements_validate_owners_at_the_read_boundary"]),
    (("manifest/mixin.rs", "options", "flatten"), &["src/manifest/mixin.rs:the_tools_keys_pass_through_and_the_packages_are_checked"]),
    (("manifest/mixin.rs", "fields", "flatten"), &["src/manifest/mixin.rs:mixins_bridges_states_modules_share_one_id_space"]),
    (("manifest/mixin.rs", "Kind", "impl Deserialize"), &["src/manifest/mixin.rs:the_tools_keys_pass_through_and_the_packages_are_checked", "src/manifest/mixin.rs:hostile_shapes_are_refused_without_panicking"]),
];

#[test]
pub(crate) fn serde_blind_spots_are_reviewed() {
    crate::at_crate_root();
    let mut reviewed: BTreeMap<_, _> = REVIEWED
        .iter()
        .map(|&((file, item, feature), fixtures)| {
            ([file, item, feature].map(str::to_owned), fixtures)
        })
        .collect();
    assert_eq!(reviewed.len(), REVIEWED.len(), "duplicate Serde exception");
    let mut files = Vec::new();
    for stem in ["manifest", "content"] {
        files.push(format!("{stem}.rs"));
        sources(Path::new(stem), &mut files);
    }
    files.sort();
    let paths = files
        .iter()
        .map(|file| (file.as_str(), Path::new("src").join(file)))
        .chain(
            EXTERNAL_DECLARATIONS
                .iter()
                .map(|&(file, _)| (file, PathBuf::from(file))),
        );
    let known_paths: BTreeSet<_> = EXTERNAL_DECLARATIONS
        .iter()
        .flat_map(|&(_, paths)| paths.iter().copied())
        .chain(NOT_DECLARATIONS.iter().map(|&(path, _reason)| path))
        .collect();
    let corpus = crate::corpus::entries();
    let mut unclassified = Vec::new();
    let mut unreviewed = Vec::new();
    for (file, path) in paths {
        let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{file}: {e}"));
        let source_tokens = tokens(&source);
        for path in workspace_paths(&source_tokens) {
            if !known_paths.contains(path.as_str()) {
                unclassified.push(format!("{file}: {path}"));
            }
        }
        for (item, feature) in blind_spots(&source_tokens) {
            if let Some(fixtures) =
                reviewed.remove(&[file.to_owned(), item.to_owned(), feature.clone()])
            {
                assert!(
                    !fixtures.is_empty(),
                    "{file}: {item}: {feature}: no fixtures"
                );
                for fixture in fixtures {
                    if let Some((source, test)) = fixture.split_once(':') {
                        let text = fs::read_to_string(source).unwrap();
                        assert!(
                            tokens(&text).windows(2).any(|pair| pair == ["fn", test]),
                            "missing reader test {fixture}"
                        );
                    } else {
                        let path = Path::new(
                            corpus
                                .get(*fixture)
                                .unwrap_or_else(|| panic!("missing corpus fixture {fixture}")),
                        );
                        assert!(
                            !path.is_dir() || path.join("sunrise-mod.toml").is_file(),
                            "missing manifest for corpus fixture {fixture}"
                        );
                    }
                }
            } else {
                unreviewed.push(format!("{file}: {item}: {feature}"));
            }
        }
    }
    assert!(
        unclassified.is_empty(),
        "Unclassified workspace paths (add the declaration source or explain the exception):\n{}",
        unclassified.join("\n")
    );
    assert!(
        unreviewed.is_empty(),
        "Serde uses without a coverage review:\n{}",
        unreviewed.join("\n")
    );
    assert!(
        reviewed.is_empty(),
        "stale Serde exceptions: {:?}",
        reviewed.keys()
    );
}

fn sources(dir: &Path, files: &mut Vec<String>) {
    for entry in fs::read_dir(Path::new("src").join(dir)).unwrap() {
        let entry = entry.unwrap();
        let path = dir.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            sources(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path.to_str().unwrap().replace('\\', "/"));
        }
    }
}

fn workspace_paths(tokens: &[&str]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for (i, &name) in tokens.iter().enumerate() {
        // These two libraries use short `[lib] name`s, not their Cargo package names.
        if (name.starts_with("sunrise_") || matches!(name, "mod_vocab" | "mod_surface"))
            && (tokens[i + 1..].starts_with(&[":", ":"])
                || tokens[i + 1..].starts_with(&["as"])
                || i > 0 && tokens[i - 1] == "use")
        {
            collect_path(&mut &tokens[i..], "", &mut paths);
        }
    }
    paths
}

// Split grouped imports down to individual paths, keeping the same precision as
// for a fully qualified field type. `as` does not change the source being used.
fn collect_path(tokens: &mut &[&str], prefix: &str, paths: &mut BTreeSet<String>) {
    if let ["{", ..] = *tokens {
        *tokens = &tokens[1..];
        while !tokens.starts_with(&["}"]) {
            collect_path(tokens, prefix, paths);
            if let ["as", _, tail @ ..] = *tokens {
                *tokens = tail;
            }
            if let [",", tail @ ..] = *tokens {
                *tokens = tail;
            } else {
                break;
            }
        }
        assert!(
            tokens.starts_with(&["}"]),
            "unclosed grouped workspace import"
        );
        *tokens = &tokens[1..];
        return;
    }
    let (name, rest) = tokens.split_first().expect("workspace path segment");
    *tokens = rest;
    let path = match (*name, prefix) {
        (_, "") => (*name).to_owned(),
        ("self", prefix) => prefix.to_owned(),
        (name, prefix) => format!("{prefix}::{name}"),
    };
    // A turbofish starts type arguments, not another segment.
    if let [":", ":", next, ..] = rest
        && *next != "<"
    {
        *tokens = &rest[2..];
        collect_path(tokens, &path, paths);
    } else {
        paths.insert(path);
    }
}

fn blind_spots<'a>(tokens: &[&'a str]) -> Vec<(&'a str, String)> {
    let mut out = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let tail = &tokens[i + 1..];
        if *token == "#" && tail.first() == Some(&"[") {
            let attr_end = group_end(tail, "[", "]");
            let attr = &tail[1..attr_end];
            // Includes `cfg_attr(..., serde(...))`. Only top-level Serde
            // meta keys count, not identifiers in another key's arguments.
            for (n, pair) in attr.windows(2).enumerate() {
                if pair != ["serde", "("] {
                    continue;
                }
                let args = &attr[n + 1..];
                let mut args = &args[1..group_end(args, "(", ")")];
                while !args.is_empty() {
                    let mut end = 0;
                    while end < args.len() && args[end] != "," {
                        end += match args[end] {
                            "(" => group_end(&args[end..], "(", ")") + 1,
                            _ => 1,
                        };
                    }
                    if matches!(
                        args[0],
                        "flatten"
                            | "alias"
                            | "tag"
                            | "untagged"
                            | "deserialize_with"
                            | "with"
                            | "from"
                            | "try_from"
                            | "other"
                    ) {
                        out.push((
                            attribute_target(&tail[attr_end + 1..]),
                            args[..end].join(" "),
                        ));
                    }
                    args = &args[(end + 1).min(args.len())..];
                }
            }
        } else if *token == "impl" {
            let mut header = tail;
            if header.first() == Some(&"<") {
                header = &header[group_end(header, "<", ">") + 1..];
            }
            let trait_path = header
                .iter()
                .take_while(|&&t| t != "<" && t != "for" && t != "{" && t != "where");
            if trait_path.copied().last() == Some("Deserialize") {
                let header = header.split(|t| *t == "{").next().unwrap();
                if let Some(index) = header.iter().position(|t| *t == "for") {
                    let target = header[index + 1..]
                        .iter()
                        .copied()
                        .take_while(|&t| t != "<" && t != "where")
                        .last()
                        .unwrap();
                    out.push((target, "impl Deserialize".into()));
                }
            }
        }
    }
    out
}

fn attribute_target<'a>(mut tail: &[&'a str]) -> &'a str {
    while tail.starts_with(&["#", "["]) {
        tail = &tail[group_end(&tail[1..], "[", "]") + 2..];
    }
    if tail.first() == Some(&"pub") {
        tail = &tail[1..];
        if tail.first() == Some(&"(") {
            tail = &tail[group_end(tail, "(", ")") + 1..];
        }
    }
    if matches!(tail.first(), Some(&("enum" | "struct"))) {
        tail = &tail[1..];
    }
    tail.first().copied().unwrap_or("<unknown item>")
}

fn group_end(tokens: &[&str], open: &str, close: &str) -> usize {
    let mut depth = 0;
    for (i, token) in tokens.iter().enumerate() {
        if *token == open {
            depth += 1;
        } else if *token == close {
            depth -= 1;
            if depth == 0 {
                return i;
            }
        }
    }
    panic!("unclosed {open} in reader source");
}

// Tokenize only what the audit needs: words, punctuation, and opaque literals.
// Comments and string contents must not be mistaken for code, including fixture
// strings in the reader's own tests. Whitespace is immaterial.
fn tokens(mut text: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    while !text.is_empty() {
        text = text.trim_start();
        let Some(first) = text.chars().next() else {
            break;
        };
        if text.starts_with("//") {
            text = text.split_once('\n').map_or("", |(_, rest)| rest);
            continue;
        }
        if let Some(mut comment) = text.strip_prefix("/*") {
            let mut depth = 1;
            while depth > 0 {
                let next = comment.find(['/', '*']).expect("unclosed block comment");
                comment = &comment[next..];
                if let Some(rest) = comment.strip_prefix("/*") {
                    depth += 1;
                    comment = rest;
                } else if let Some(rest) = comment.strip_prefix("*/") {
                    depth -= 1;
                    comment = rest;
                } else {
                    comment = &comment[1..];
                }
            }
            text = comment;
            continue;
        }
        let raw = ["r", "br", "cr"].into_iter().find_map(|prefix| {
            let suffix = text.strip_prefix(prefix)?;
            let hashes = suffix.bytes().take_while(|b| *b == b'#').count();
            let body = suffix[hashes..].strip_prefix('"')?;
            let delimiter = format!("\"{}", "#".repeat(hashes));
            Some(
                text.len() - body.len()
                    + body.find(&delimiter).expect("unclosed raw string")
                    + delimiter.len(),
            )
        });
        let literal = |quote| {
            let mut escaped = false;
            text[1..].char_indices().find_map(|(i, c)| {
                let end = !escaped && c == quote;
                escaped = !escaped && c == '\\';
                end.then_some(i + 1 + c.len_utf8())
            })
        };
        let len = if let Some(len) = raw {
            len
        } else if first == '"'
            || (first == '\''
                && (text[1..].starts_with('\\') || text[1..].chars().nth(1) == Some('\'')))
        {
            literal(first).expect("unclosed literal")
        } else if first.is_alphanumeric() || first == '_' {
            text.find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(text.len())
        } else {
            first.len_utf8()
        };
        tokens.push(&text[..len]);
        text = &text[len..];
    }
    tokens
}
