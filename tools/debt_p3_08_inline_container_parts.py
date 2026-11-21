#!/usr/bin/env python3
"""Store the one or two backing blocks of a container view inline for slot loans."""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
MACHINES = ROOT / "crates/sunrise-client/src/game/blocks/machines"


def replace(source, old, new):
    if source.count(old) != 1:
        raise SystemExit(f"expected one occurrence of {old!r}")
    return source.replace(old, new)


def uses(source, singles, indexes, loops):
    for pattern, replacement, expected in (
        (r"(?m)^( +)parts: vec!\[(.*)\],$", r"\1first: \2,\n\1second: None,", singles),
        (r"\b(view|hopper|source)\.parts\[0\]", r"\1.first", indexes),
        (r"for &\((\w+), _\) in &view\.parts", r"for (\1, _) in view.parts()", loops),
    ):
        source, count = re.subn(pattern, replacement, source)
        if count != expected:
            raise SystemExit(f"{pattern}: expected {expected} occurrences, got {count}")
    return source


machines = MACHINES / "mod.rs"
native_items = MACHINES / "native_items.rs"
source = uses(machines.read_text(), singles=8, indexes=13, loops=2)
source = replace(
    source,
    "    parts: Vec<(Pos, usize)>,",
    """    first: (Pos, usize),
    /// The second half of vanilla's `CompoundContainer`, if this is a double chest.
    second: Option<(Pos, usize)>,""",
)
source = replace(
    source,
    """    fn len(&self) -> usize {
        self.parts.iter().map(|p| p.1).sum()
    }
    fn locate(&self, mut slot: usize) -> Option<(Pos, usize)> {
        for &(pos, size) in &self.parts {""",
    """    fn parts(&self) -> impl Iterator<Item = (Pos, usize)> {
        std::iter::once(self.first).chain(self.second)
    }
    fn len(&self) -> usize {
        self.parts().map(|(_, size)| size).sum()
    }
    fn locate(&self, mut slot: usize) -> Option<(Pos, usize)> {
        for (pos, size) in self.parts() {""",
)
source = replace(
    source,
    """                    let parts = if own.kind == "right" {
                        vec![(pos, size), (other, 27)]
                    } else {
                        vec![(other, 27), (pos, size)]
                    };
                    return Some(View {
                        parts,""",
    """                    let (first, second) = if own.kind == "right" {
                        ((pos, size), (other, 27))
                    } else {
                        ((other, 27), (pos, size))
                    };
                    return Some(View {
                        first,
                        second: Some(second),""",
)
items = uses(native_items.read_text(), singles=1, indexes=1, loops=3)
machines.write_text(source)
native_items.write_text(items)
