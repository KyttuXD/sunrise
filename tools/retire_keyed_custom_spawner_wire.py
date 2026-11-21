#!/usr/bin/env python3
"""Remove the tests of the retired key-based custom spawner Wire forms."""

from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def remove(path: str, start: str, end: str) -> None:
    file = ROOT / path
    source = file.read_text()
    if start not in source:
        return
    begin = source.index(start)
    stop = source.index(end, begin + len(start))
    file.write_text(source[:begin] + source[stop:])


VOCAB = "crates/sunrise-mod-vocab/src/"
remove(
    VOCAB + "wire/wire_types.rs",
    '        round(crate::SpawnerKey::new("example:spawner").unwrap());\n',
    '        round(crate::Identifier::parse("example:thing").unwrap());\n',
)
remove(
    VOCAB + "wire/wire_types.rs",
    '        out.clear();\n        out.put_str("not namespaced");\n',
    '        assert!(crate::wire::intern(&"x".repeat(257)).is_err());\n',
)
remove(
    VOCAB + "wire/wire_layout_tests.rs",
    "    let spawners = vec![\n        SpawnerEntry::Builtin(SpawnerKind::Cat),\n",
    '    let modifier = AttributeModifier::new(id("example:boost"), 1.75, AttributeOperation::AddValue);\n',
)
layout = ROOT / VOCAB / "wire/wire_layout_tests.rs"
layout.write_text(
    layout.read_text().replace(
        "fn spawner_and_attribute_records_have_declared_field_bytes()",
        "fn attribute_records_have_declared_field_bytes()",
    )
)
remove(
    VOCAB + "payload/codec_tests.rs",
    '    ordinary_only(&crate::SpawnerKey::new("test:spawner").unwrap());\n',
    "    assert_eq!(\n        decode_payload::<String>",
)
remove(
    "crates/sunrise-mod-api/src/parts/payloads/typed_tests.rs",
    '        refused::<crate::SpawnerKey>(&mut view, "test:spawner"),\n',
    "    ];\n",
)
