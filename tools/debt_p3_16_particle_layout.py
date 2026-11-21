#!/usr/bin/env python3
"""Move the existing named particle path and provider tests for debt P3-16.

Run once on the pre-move source, then rustfmt the three touched Rust files.
"""

from pathlib import Path
from textwrap import dedent, indent


def split_once(source, anchor):
    parts = source.split(anchor)
    if len(parts) != 2:
        raise SystemExit(f"Expected exactly one anchor: {anchor!r}")
    return parts


def replace_once(source, anchor, replacement):
    before, after = split_once(source, anchor)
    return before + replacement + after


root = Path(__file__).resolve().parents[1]
particles = root / "crates/sunrise-client-render/src/particles.rs"
native = particles.with_name("particles") / "native.rs"
tests = native.with_suffix("") / "tests.rs"
if tests.exists():
    raise SystemExit(f"Unexpected existing file: {tests}")

engine_source = particles.read_text(encoding="utf-8")
native_source = native.read_text(encoding="utf-8")

named_comment = (
    "        // A native mod's SimpleParticleType: vanilla's TextureSheetParticle with\n"
    "        // the sprite set its particles/<name>.json names, no tint, 20..40 ticks.\n"
)
engine_source = replace_once(engine_source, named_comment, "")
after_polytone = "            return Some(particle);\n        }\n"
named_start = "        if let Some(name) = &o.mod_particle {\n"
vanilla_start = "        let set = self.set_of(ty);\n"
engine_before, rest = split_once(engine_source, after_polytone + named_start)
named_body, engine_after = split_once(rest, "            return Some(q);\n        }\n" + vanilla_start)
engine_source = (
    engine_before
    + after_polytone
    + named_start
    + "            return self.make_named(name.as_ref(), p, v, seed);\n        }\n"
    + vanilla_start
    + engine_after
)
named_body = indent(dedent(named_body), "        ")
named_body = replace_once(named_body, "self.mod_sets.get(name.as_ref())?", "self.mod_sets.get(name)?")
named_body = replace_once(named_body, "native::Provided::new(", "Provided::new(")
make_named = (
    "    pub(super) fn make_named(\n"
    "        &mut self,\n"
    "        name: &str,\n"
    "        p: [f64; 3],\n"
    "        v: [f64; 3],\n"
    "        seed: i64,\n"
    "    ) -> Option<Particle> {\n"
    + named_comment
    + named_body
    + "        Some(q)\n    }\n"
)

tick_doc = "    /// The providers' part of a tick:"
group_doc = "    /// `ParticleGroup.add`:"
engine_before, rest = split_once(engine_source, tick_doc)
tick_tail, engine_after = split_once(rest, group_doc)
engine_source = engine_before + group_doc + engine_after
tick = replace_once(tick_doc + tick_tail, "    fn tick_providers(", "    pub(super) fn tick_providers(")
tick = replace_once(tick, "native::ProviderLevel {", "ProviderLevel {")

native_source = replace_once(
    native_source,
    "use super::{Particle, ParticleLevel, ParticleOptions};\n",
    "use super::{Behaviour, Particle, ParticleEngine, ParticleLevel, ParticleOptions, ParticleType, Sprite};\n",
)
native_source = replace_once(
    native_source,
    "use std::{collections::BTreeMap, fmt::Debug, sync::Arc, sync::OnceLock};\n",
    "use std::{collections::BTreeMap, fmt::Debug, sync::Arc, sync::OnceLock};\n"
    "use sunrise_core::random::{LegacyRandom, RandomSource};\n",
)
test_decl = "#[cfg(test)]\npub(super) mod tests"
native_before, test_body = split_once(native_source, test_decl + " {\n")
if not test_body.endswith("\n}\n"):
    raise SystemExit("Expected the inline tests module to end native.rs")
test_source = dedent(test_body.removesuffix("}\n"))

# Check all anchors before writing any file.
tests.parent.mkdir(exist_ok=True)
with tests.open("x", encoding="utf-8") as out:
    out.write(test_source)
native.write_text(
    native_before + "impl ParticleEngine {\n" + make_named + "\n" + tick + "}\n\n" + test_decl + ";\n",
    encoding="utf-8",
)
particles.write_text(engine_source, encoding="utf-8")
