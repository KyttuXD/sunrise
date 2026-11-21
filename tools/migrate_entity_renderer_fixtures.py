#!/usr/bin/env python3
"""Migrate entity renderer route fixtures to attributed callbacks and per-draw outcomes."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TESTS = ROOT / "crates/sunrise-client/src/game/modding"
POLICY = ROOT / "tools/ci/ignored-tests"


def replace(path, old, new, count=1):
    source = path.read_text(encoding="utf-8")
    if source.count(old) != count:
        raise SystemExit(f"{path}: expected {count} occurrences of {old!r}")
    path.write_text(source.replace(old, new), encoding="utf-8")


for route, count in [("experience_orb", 1), ("item_frame", 2), ("painting", 1), ("tnt", 1)]:
    path = TESTS / f"native_{route}_renderers_tests.rs"
    replace(path, "native_entity_renderers::EntityRendererTable", "native_entity_renderers::{EntityRendererTable, fixture_renderer}")
    replace(path, "Arc::new(Mark) as Arc<dyn EntityRenderer>", "fixture_renderer(Mark)", count)

path = TESTS / "native_general_renderers_tests.rs"
replace(path, "native_entity_renderers::EntityRendererTable", "native_entity_renderers::{EntityRendererTable, RenderersByType, fixture_renderer}")
replace(path, "    collections::BTreeMap,\n", "")
replace(path, "BTreeMap<String, Arc<dyn EntityRenderer>>", "RenderersByType")
replace(path, "Arc::new(Blank) as Arc<dyn EntityRenderer>", "fixture_renderer(Blank)")

for route, shape, policy in [
    ("experience_orb", "quad", "experience-orb-renderers"),
    ("item_frame", "model", "item-frame-renderers"),
    ("painting", "quads", "painting-renderers"),
    ("tnt", "cube", "tnt-renderers"),
    ("living", "types_model", "issue-4451"),
]:
    prefix = "a_failed_renderer" if route == "living" else f"a_failed_{route}_renderer"
    old = f"{prefix}_draws_the_{shape}_until_a_resource_reload"
    new = f"{prefix}_skips_only_the_failed_draw"
    replace(TESTS / f"native_{route}_renderers_tests.rs", old, new, 2)
    replace(POLICY / f"daily-sunrise-client-{policy}.txt", old, new)

old = "a_failing_renderer_is_dropped_until_reload_and_a_type_without_one_draws_nothing"
new = "a_panicking_renderer_stays_disabled_and_a_type_without_one_draws_nothing"
replace(path, old, new, 2)
replace(POLICY / "daily-sunrise-client-general-renderers.txt", old, new)
