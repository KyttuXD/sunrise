#!/usr/bin/env python3
"""Separate native declarations from wire IDs on the P3-04 baseline.

Run once, then format the touched Rust files with rustfmt --edition 2024.
"""

from pathlib import Path
from subprocess import check_output


ROOT = Path(__file__).resolve().parents[1]
CALL = "BlockEntityTypes::new"
CORE = "crates/sunrise-client-core/src/block_entity_types.rs"
CLIENT = "crates/sunrise-client/src"
PRODUCER = f"{CLIENT}/game/modding/mods/mod.rs"


def declarations(before, ids, count=1):
    """Insert an explicit Rust list before the end of a multiline constructor."""
    args, close = before.rsplit("\n", 1)
    indent = close.removesuffix(")")
    return before, f"{args}\n{indent}    {ids},\n{close}", count


CHUNK_WIRE = '''            builtin
                .get("minecraft:block_entity_type")
                .unwrap()
                .names
                .clone(),
        )'''

# The last field of a replacement counts identical source fragments in that file.
FIXTURES = {
    CORE: [
        declarations(
            '''            vec![
                "minecraft:vault".into(),
                "mod:own".into(),
                "mod:other".into(),
            ],
        )''',
            '["mod:own".into(), "mod:other".into()]',
        ),
        (
            "BlockEntityTypes::new(BTreeMap::new(), Vec::new())",
            "BlockEntityTypes::new(BTreeMap::new(), Vec::new(), [])",
            1,
        ),
        declarations(
            '''            vec![
                "minecraft:chest".into(),
                "test:machine".into(),
                "test:single".into(),
                "test:empty".into(),
            ],
        )''',
            '["test:machine".into(), "test:single".into(), "test:empty".into()]',
        ),
        declarations(
            '''                vec!["minecraft:sign".into(), "test:machine".into()],
            )''',
            '["test:machine".into()]',
        ),
    ],
    f"{CLIENT}/block_entity_item.rs": [
        declarations(
            '''            vec!["example:crate".into(), "example:tank".into()],
        )''',
            '["example:crate".into(), "example:tank".into()]',
        ),
    ],
    f"{CLIENT}/content_identity.rs": [
        declarations(
            '''            vec!["minecraft:chest".into(), "minecraft:trapped_chest".into()],
        )''',
            "[]",
        ),
    ],
    f"{CLIENT}/game/administration/commands/data/mod_block_entity_tests.rs": [
        declarations(
            '''            vec![ENTITY.to_owned(), "fixture:replacement".to_owned()],
        )''',
            '[ENTITY.to_owned(), "fixture:replacement".to_owned()]',
        ),
    ],
    f"{CLIENT}/game/network/remote/block_entity_data_tests.rs": [
        declarations(
            '''                vec!["notice:counter".into(), "notice:silent".into()],
            )''',
            '["notice:counter".into(), "notice:silent".into()]',
        ),
    ],
    f"{CLIENT}/lan/chunks.rs": [
        declarations(
            '''            [
                ("test:press".into(), "test:machine".into()),
                ("test:mill".into(), "test:machine".into()),
            ]
            .into(),
''' + CHUNK_WIRE,
            '["test:machine".into(), "test:casing".into()]',
        ),
        declarations(
            '''            [("test:machine".into(), "test:machine".into())].into(),
''' + CHUNK_WIRE,
            '["test:machine".into()]',
            3,
        ),
    ],
    f"{CLIENT}/lan/hub.rs": [
        declarations(CHUNK_WIRE, '["test:machine".into()]'),
    ],
    f"{CLIENT}/save.rs": [
        declarations(
            '''            vec![
                "test:machine".into(),
                "test:other".into(),
                "minecraft:chest".into(),
            ],
        )''',
            '["test:machine".into(), "test:other".into()]',
        ),
    ],
    f"{CLIENT}/structure_blocks.rs": [
        declarations(
            '''            vec!["test:machine".into()],
        )''',
            '["test:machine".into()]',
        ),
    ],
    "crates/sunrise-client/tests/lan_bot.rs": [
        declarations(
            '''        vec!["test:machine_entity".into()],
    )''',
            '["test:machine_entity".into()]',
        ),
    ],
    "crates/sunrise-command/src/item.rs": [
        declarations(
            '''            vec!["example:crate".into()],
        )''',
            '["example:crate".into()]',
        ),
    ],
}

DEFINITION = [
    (
        "    keys: BTreeSet<Arc<str>>,\n",
        "    keys: BTreeSet<Arc<str>>,\n    declared: BTreeSet<String>,\n",
        1,
    ),
    (
        '''    /// Membership has already been checked by the content plan.
    pub fn new(blocks: BTreeMap<String, String>, wire: Vec<String>) -> Self {''',
        '''    /// The content plan has already validated the block factories and admitted native types.
    /// `declared` includes native types with no blocks and excludes built-in types.
    pub fn new(
        blocks: BTreeMap<String, String>,
        wire: Vec<String>,
        declared: impl IntoIterator<Item = String>,
    ) -> Self {''',
        1,
    ),
    (
        "            keys,\n            registered,",
        "            keys,\n            declared: declared.into_iter().collect(),\n            registered,",
        1,
    ),
    (
        '''    /// Whether a mod declared the block entity type `id`.
    /// Wire keys include vanilla types; `mod_content::Plan::new` refuses native
    /// declarations in `minecraft:`.
    pub fn declares(&self, id: &str) -> bool {
        !id.starts_with("minecraft:") && self.keys.contains(id)
    }''',
        '''    /// Whether the content plan admitted `id` as a native block entity type.
    pub fn declares(&self, id: &str) -> bool {
        self.declared.contains(id)
    }''',
        1,
    ),
]

PRODUCTION = [
    (
        '''    let membership = mods
        .iter()
        .flat_map(|m| &m.content.block_entity_types)
        .flat_map(|kind| {''',
        '''    let declarations = mods.iter().flat_map(|m| &m.content.block_entity_types);
    let membership = declarations
        .clone()
        .flat_map(|kind| {''',
        1,
    ),
    (
        "    let mut types = BlockEntityTypes::new(membership, names.clone());",
        '''    let mut types = BlockEntityTypes::new(
        membership,
        names.clone(),
        declarations.map(|kind| kind.id.clone()),
    );''',
        1,
    ),
]


def main():
    expected = {
        path: sum(count for _, _, count in changes) for path, changes in FIXTURES.items()
    }
    expected[PRODUCER] = 1
    callers = set(
        check_output(
            ["git", "grep", "-l", "-F", CALL, "--", "*.rs"], cwd=ROOT, text=True
        ).splitlines()
    )
    if callers != expected.keys():
        raise SystemExit(f"Constructor file mismatch: {sorted(callers ^ expected.keys())}")

    changes_by_file = {**FIXTURES, PRODUCER: PRODUCTION}
    changes_by_file[CORE] = DEFINITION + FIXTURES[CORE]
    pending = {}
    for path, changes in changes_by_file.items():
        source = (ROOT / path).read_text()
        if source.count(CALL) != expected[path]:
            raise SystemExit(f"Unexpected constructor count in {path}")
        for before, after, count in changes:
            found = source.count(before)
            if found != count:
                raise SystemExit(f"{path}: expected {count}, got {found} for:\n{before}")
            source = source.replace(before, after)
        pending[path] = source

    for path, source in pending.items():
        (ROOT / path).write_text(source)
    print(f"Migrated {sum(expected.values())} constructors across {len(expected)} Rust files.")


if __name__ == "__main__":
    main()
