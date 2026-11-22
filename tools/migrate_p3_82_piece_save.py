#!/usr/bin/env python3
"""Convert built-in LegacyPiece serializers to the typed save result in P3-82."""

import re
from pathlib import Path

STRUCTURE = Path(__file__).resolve().parents[1] / "crates/sunrise-world/src/worldgen/structure"
SIGNATURE = re.compile(
    r"(?P<indent> +)fn save\(&self\) -> Option<(?:sunrise_core::nbt::)?Compound> \{\n"
    r"(?P<body>.*?)^(?P=indent)\}",
    re.MULTILINE | re.DOTALL,
)


def convert(match):
    indent, body = match.group("indent", "body")
    result, count = re.subn(r"^( +)Some\(", r"\1Ok(", body, flags=re.MULTILINE)
    if count != 1:
        raise ValueError(f"expected a single successful built-in serialization: {body}")
    legacy = "super::super::legacy" if len(indent) == 8 else "super::legacy"
    return f"{indent}fn save(&self) -> {legacy}::PieceSave {{\n{result}{indent}}}"


for path in sorted(STRUCTURE.glob("*.rs")):
    if path.name in {"legacy.rs", "start.rs"}:
        continue
    source = path.read_text()
    result = SIGNATURE.sub(convert, source)
    if result != source:
        path.write_text(result)
