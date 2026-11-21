#!/usr/bin/env python3
"""Apply the mechanical RegistryId migration to the P3-24 baseline.

Run once on 99c5682ffe6, then format the touched Rust files (edition 2024).
The existing registry name validation and interners are unchanged.
"""

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
COMMAND = Path("crates/sunrise-command/src")
NATIVE_KINDS = Path("crates/sunrise-client/src/game/modding/native_server/argument_kinds.rs")
PROBE = Path("crates/sunrise-client/examples/probe/command_parse_fixture.rs")
VARIANTS = ("Res", "ResKey", "ResOrTag", "ResOrTagKey", "ResourceSelector", "ResourceOrId")
LITERAL = re.compile(rf'(ArgType::(?:{"|".join(VARIANTS)})\()("[^"\n]+")\)')
REGISTRY_ID = '''/// The registry key carried by a registry-backed command argument.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryId(Cow<'static, str>);

impl RegistryId {
    /// Wraps a trusted static registry name without validation or allocation.
    pub const fn from_static(name: &'static str) -> Self {
        Self(Cow::Borrowed(name))
    }

    /// The registry name used for lookups.
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

'''


def replace(source, old, new, count=1):
    if source.count(old) != count:
        raise SystemExit(f"expected {count} occurrence(s) of {old!r}")
    return source.replace(old, new)


def main():
    changes = {}
    path = COMMAND / "args.rs"
    source = (ROOT / path).read_text()
    source = replace(source, "use std::{fmt, sync::Arc};", "use std::{borrow::Cow, fmt, sync::Arc};")
    declaration = "#[derive(Clone, Debug)]\npub enum ArgType {"
    source = replace(source, declaration, REGISTRY_ID + declaration)
    for variant in VARIANTS:
        source = replace(source, f"    {variant}(&'static str),", f"    {variant}(RegistryId),")

    for first_line in (
        "ArgType::Res(registry) => {\n                let id",
        "ArgType::ResOrTag(registry) | ArgType::ResOrTagKey(registry) => {\n                let key_only",
        "ArgType::ResourceSelector(registry) => {\n                let start",
        "ArgType::Res(registry) | ArgType::ResKey(registry) => {\n                // RecipeCommand",
        "ArgType::ResOrTag(registry) | ArgType::ResOrTagKey(registry) => {\n                suggest_resources",
    ):
        arm, line = first_line.split("\n", 1)
        source = replace(source, first_line, f"{arm}\n                let registry = registry.as_str();\n{line}")
    for old, new, count in (
        ("&[&id, registry]", "&[&id, &registry]", 3),
        ("&[&pattern, registry]", "&[&pattern, &registry]", 1),
        ("if *registry ==", "if registry ==", 1),
        ("resource_or_id(reader, reg, registry)?", "resource_or_id(reader, reg, registry.as_str())?", 1),
        (
            'ArgType::ResourceSelector(registry) => {\n                suggest_resources(builder, reg.ids(registry), "")',
            'ArgType::ResourceSelector(registry) => {\n                suggest_resources(builder, reg.ids(registry.as_str()), "")',
            1,
        ),
        (
            'ArgType::ResourceOrId(registry) => suggest_resources(builder, reg.ids(registry), ""),',
            'ArgType::ResourceOrId(registry) => suggest_resources(builder, reg.ids(registry.as_str()), ""),',
            1,
        ),
    ):
        source = replace(source, old, new, count)
    changes[path] = source

    path = COMMAND / "lib.rs"
    changes[path] = replace((ROOT / path).read_text(), "Registries, modify_custom_completions,", "Registries, RegistryId, modify_custom_completions,")

    for module, old_import, new_import in (
        ("network", "NodeKind, Suggests", "NodeKind, RegistryId, Suggests"),
        ("tree_data", "args::ArgType,", "args::{ArgType, RegistryId},"),
        ("tree_execute", "args::{ArgType, Resource}", "args::{ArgType, RegistryId, Resource}"),
        ("tree_extra", "args::ArgType,", "args::{ArgType, RegistryId},"),
        ("tree_misc", "args::ArgType,", "args::{ArgType, RegistryId},"),
        ("tree_world", "args::{ArgType, integer}", "args::{ArgType, RegistryId, integer}"),
        ("vanilla", "args::{ArgType, Resource, integer}", "args::{ArgType, RegistryId, Resource, integer}"),
    ):
        path = COMMAND / f"{module}.rs"
        source = replace((ROOT / path).read_text(), old_import, new_import)
        source, count = LITERAL.subn(r"\1RegistryId::from_static(\2))", source)
        if not count:
            raise SystemExit(f"no registry literals found in {path}")
        if module == "tree_data":
            source = replace(source, "ArgType::ResourceOrId(registry)", "ArgType::ResourceOrId(RegistryId::from_static(registry))")
        if module == "network":
            source = replace(
                source,
                'let registry = || match &p {\n        ArgumentProperties::Registry(r) => intern(r),\n        _ => "",\n    };',
                'let registry = || RegistryId::from_static(match &p {\n        ArgumentProperties::Registry(r) => intern(r),\n        _ => "",\n    });',
            )
            source = replace(
                source,
                "/// A registry name kept for the life of the game (argument types hold\n/// `&'static str`): each distinct name is stored once.",
                "/// A registry name borrowed by the argument type and kept for the life of the\n/// game: each distinct name is stored once.",
            )
        changes[path] = source

    source = (ROOT / NATIVE_KINDS).read_text()
    source = replace(source, "NodeKind, Range, Reader,", "NodeKind, Range, Reader, RegistryId,")
    source = replace(
        source,
        'return ArgType::Res(\n            self::registry(registry).expect("native command tree names validated registries"),\n        );',
        'return ArgType::Res(RegistryId::from_static(\n            self::registry(registry).expect("native command tree names validated registries"),\n        ));',
    )
    changes[NATIVE_KINDS] = replace(
        source,
        "/// A registry name a usage named, kept for the game's life (`ArgType::Res` holds\n/// `&'static str`), each name once.",
        "/// A registry name a usage named, borrowed by `ArgType::Res` and kept for the\n/// game's life, each name once.",
    )
    changes[PROBE] = replace((ROOT / PROBE).read_text(), "ArgType::ResourceOrId(r) => match *r {", "ArgType::ResourceOrId(r) => match r.as_str() {")

    for path, source in changes.items():
        (ROOT / path).write_text(source)


if __name__ == "__main__":
    main()
