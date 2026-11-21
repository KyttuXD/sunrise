#!/usr/bin/env python3
"""Migrate the item-60 tooltip fixtures to the engine-side prepared list and integer anchor."""
from pathlib import Path
import re

root = Path("crates/sunrise-client/src/game/blocks")
for path in [root / "stations.rs", *sorted((root / "stations").glob("tooltip_*tests.rs"))]:
    source = path.read_text()
    source = re.sub(
        r"(\.gather_tooltip_components\(.*?\)\s*\.(?:unwrap\(\)|expect\([^\n]*?\)))",
        r"\1.components",
        source,
        flags=re.S,
    ) if path.name != "stations.rs" else source.replace(
        "let plain = gather(game).unwrap();", "let plain = gather(game).unwrap().components;"
    ).replace(
        "let elements = gather(game).unwrap();", "let elements = gather(game).unwrap().components;"
    ).replace(
        "let mixed = gather(game).unwrap();", "let mixed = gather(game).unwrap().components;"
    ).replace(
        "assert!(gather(game).is_none());", "assert!(gather(game).unwrap().components.is_empty());"
    )
    source = re.sub(
        r"PreparedTooltipComponent::__text\(\s*mod_api::Text::literal\(([^)]*)\),",
        r"PreparedComponent::Text(sunrise_client::text::Text::literal(\1),",
        source,
    )
    source = re.sub(
        r"(draw_prepared_tooltip\([^;]*?)\(10\.0, 20\.0\),(\s*true)",
        r"\1[10, 20], DEFAULT_FONT_ID.clone(),\2",
        source,
    )
    source = re.sub(
        r"(tooltip_pre_components\([^;]*?)\(10\.0, 20\.0\),(\s*\(320\.0, 240\.0\),)",
        r"\1[10, 20],\2 DEFAULT_FONT_ID.clone(),",
        source,
    ).replace("Some(((10.0, 20.0), DEFAULT_FONT_ID.clone()))", "Some(([10, 20], DEFAULT_FONT_ID.clone()))")
    source = re.sub(
        r"game\s*\.prepare_tooltip_component\((&TooltipComponent::custom\(\w+\))\)",
        r"sunrise_client::tooltip_components::create(\1).unwrap()",
        source,
    )
    path.write_text(source)
