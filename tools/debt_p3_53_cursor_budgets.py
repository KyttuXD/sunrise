#!/usr/bin/env python3
"""Make the shared ray and collision cursor budgets optional for P3-53.

Run once on the part-2 baseline, then rustfmt the changed Rust files. Every
existing concrete budget is preserved; only the two internal forwards pass the
new Option straight through. Check the inventory before writing any files.
"""

from collections import Counter, defaultdict
from pathlib import Path
import re
import subprocess

from ci.check_api_boundary import use_paths
from rust_tokens import tokens

ROOT = Path(__file__).resolve().parents[1]
TRAVERSAL = "crates/sunrise-core/src/geometry/traversal.rs"
RAY = "crates/sunrise-world/src/world_ray.rs"
SCAN = "crates/sunrise-world/src/block_collision_scan.rs"
TRAVERSE = ("sunrise_core", "geometry", "traverse_blocks")
CLIP = ("sunrise_world", "world_ray", "clip_world")
NEW = ("sunrise_world", "block_collision_scan", "BlockCollisionScan", "new")
ANY = (*NEW[:-1], "any_collision")
# Fully qualified symbol -> (arity, budget argument index, original call count).
CALLS = {TRAVERSE: (4, 2, 25), CLIP: (6, 5, 10), NEW: (5, 4, 21), ANY: (5, 4, 2)}
FORWARDS = {(RAY, TRAVERSE): "max_visits", (SCAN, NEW): "candidate_budget"}
IDENT = re.compile(r"[A-Za-z_][A-Za-z_0-9]*\Z")
BUDGET = re.compile(
    r"[0-9][0-9_]*|1 << 20|usize::MAX|COLLISION_CELLS|"
    r"budget|(?:request\.)?candidate_budget|max_visits"
)


def arguments(values, opening):
    """Yield token spans for arguments, excluding trivia and a trailing comma."""
    delimiters = {"(": ")", "[": "]", "{": "}"}
    expected = [")"]
    start = opening + 1
    for i in range(start, len(values)):
        value = values[i]
        if value == expected[-1]:
            expected.pop()
            if not expected:
                if start < i:
                    yield start, i
                return
        elif value in delimiters:
            expected.append(delimiters[value])
        elif value == "<" and (expected[-1] == ">" or values[i - 1] == "::"):
            expected.append(">")
        elif value in delimiters.values():
            raise ValueError("unbalanced call delimiters")
        elif value == "," and len(expected) == 1:
            if start == i:
                raise ValueError("empty argument")
            yield start, i
            start = i + 1
    raise ValueError("unterminated argument list")


def rewrite_calls(file, source, counts, forwarded):
    ts = []
    for value, offset in tokens(source):
        if value.startswith("//"):
            continue
        if value == ":" and ts and ts[-1] == (":", offset - 1):
            ts[-1] = ("::", offset - 1)
        else:
            ts.append((value, offset))
    values = [value for value, _ in ts]
    crate = file.split("/")[1].replace("-", "_")
    imports = defaultdict(set)
    for i, value in enumerate(values[:-1]):
        if value != "use":
            continue
        first = values[i + 1]
        if first not in ("{", "::") and not IDENT.fullmatch(first):
            continue
        end = values.index(";", i + 1)
        for path, name in use_paths(values[i + 1:end]):
            if name == "self":
                path, name = path[:-1], path[-2]
            if path[0] == "crate":
                path = (crate, *path[1:])
            elif path == ("super", "*") and file == TRAVERSAL:
                path = (*TRAVERSE[:-1], "*")
            imports[name].add(path)

    edits = []
    for i, name in enumerate(values):
        if name not in {"traverse_blocks", "clip_world", "new", "any_collision"}:
            continue
        start = i
        while start >= 2 and values[start - 1] == "::" and IDENT.fullmatch(values[start - 2]):
            start -= 2
        path = tuple(values[start:i + 1:2])
        if name == "new" and (len(path) < 2 or path[-2] not in {"BlockCollisionScan", "Self"}):
            continue
        if name == "new" and path == ("Self", "new") and file != SCAN:
            continue
        if start and values[start - 1] == "fn":
            continue
        opening = i + 1
        if values[opening:opening + 2] == ["::", "<"]:
            depth = 1
            opening += 2
            while depth and opening < len(values):
                depth += (values[opening] == "<") - (values[opening] == ">")
                opening += 1
        if values[opening:opening + 1] != ["("]:
            continue
        line = source.count("\n", 0, ts[i][1]) + 1
        where = f"{file}:{line}"
        imported = imports[path[0]]
        if len(imported) > 1:
            raise ValueError(f"{where}: ambiguous import for {path[0]}")
        if imported:
            candidates = {p + path[1:] for p in imported}
        elif path[0] == "crate":
            candidates = {(crate, *path[1:])}
        elif file == SCAN and path == ("Self", "new"):
            candidates = {NEW}
        else:
            candidates = {path, *(p[:-1] + path for p in imports["*"])}
        matched = candidates & CALLS.keys()
        if len(matched) != 1 or (start and values[start - 1] == "."):
            raise ValueError(f"{where}: unexpected call path {'::'.join(path)}")
        (target,) = matched
        arity, budget_index, _ = CALLS[target]
        args = list(arguments(values, opening))
        if len(args) != arity:
            raise ValueError(f"{where}: expected {arity} arguments, got {len(args)}")
        first, stop = args[budget_index]
        lo = ts[first][1]
        hi = ts[stop - 1][1] + len(values[stop - 1])
        budget = source[lo:hi]
        counts[target] += 1
        key = (file, target)
        if key in FORWARDS:
            if budget != FORWARDS[key] or key in forwarded:
                raise ValueError(f"{where}: unexpected forwarded budget {budget!r}")
            forwarded.add(key)
        else:
            if not BUDGET.fullmatch(budget):
                raise ValueError(f"{where}: unexpected budget expression {budget!r}")
            edits.append((lo, hi, f"Some({budget})"))
    for lo, hi, replacement in sorted(edits, reverse=True):
        source = source[:lo] + replacement + source[hi:]
    return source


# Checked exact replacements for the three implementations; scan skipping
# still charges every outstanding cell up to the end of a budgeted scan.
CURSORS = {
    TRAVERSAL: [
        ("    max_visits: usize,", "    mut max_visits: Option<usize>,", 1),
        ("/// Non-finite geometry and exceeding the caller's visit budget return typed\n",
         "/// Non-finite geometry and exceeding an optional visit budget return typed\n", 1),
        ("""    let mut visited = 0;
    loop {
        if visited == max_visits {
            return Err(TraversalError::VisitLimit);
        }
        visited += 1;""", """    loop {
        if let Some(remaining) = &mut max_visits {
            if *remaining == 0 {
                return Err(TraversalError::VisitLimit);
            }
            *remaining -= 1;
        }""", 1),
    ],
    RAY: [
        ("    max_visits: usize,", "    max_visits: Option<usize>,", 1),
        ("/// Finite rays only, with an explicit traversal budget.",
         "/// Finite rays only, with an optional traversal budget.", 1),
    ],
    SCAN: [
        ("    budget: usize,", "    budget: Option<usize>,", 1),
        ("        candidate_budget: usize,", "        candidate_budget: Option<usize>,", 2),
        ("""    /// Budget counts every cursor cell, including skipped corners. After an error
    /// iteration terminates; already performed queries are not rolled back.""", """    /// An optional budget counts every cursor cell, including skipped corners.
    /// After an error, iteration terminates; already performed queries are not rolled back.""", 1),
        ("    /// same row and chunk, never more than the budget has left: what can be passed at once.",
         "    /// same row and chunk, limited by the remaining budget if one was supplied.", 1),
        ("""        (self.sizes[0] - 1 - x)
            .min(15 - (block_x & 15) as usize)
            .min(self.budget.saturating_sub(self.index))""", """        let rest = (self.sizes[0] - 1 - x).min(15 - (block_x & 15) as usize);
        self.budget
            .map_or(rest, |budget| rest.min(budget.saturating_sub(self.index)))""", 1),
        ("            if self.index >= self.budget {",
         "            if self.budget.is_some_and(|budget| self.index >= budget) {", 1),
    ],
}


def main():
    files = subprocess.check_output(
        ["git", "grep", "-lz", "-wE", "traverse_blocks|clip_world|BlockCollisionScan|any_collision",
         "--", "*.rs"], cwd=ROOT, text=True,
    ).split("\0")[:-1]
    counts, forwarded, rewritten = Counter(), set(), {}
    for file in files:
        path = ROOT / file
        original = path.read_text()
        source = rewrite_calls(file, original, counts, forwarded)
        for old, new, times in CURSORS.get(file, ()):
            if source.count(old) != times:
                raise ValueError(f"{file}: expected {times} matches for {old!r}")
            source = source.replace(old, new)
        if source != original:
            rewritten[path] = source
    expected = {target: count for target, (_, _, count) in CALLS.items()}
    if counts != expected or forwarded != FORWARDS.keys():
        raise ValueError(f"unexpected call inventory: {dict(counts)}; forwards: {forwarded}")
    for path, source in rewritten.items():
        path.write_text(source)
    print(f"Checked {sum(counts.values())} calls ({len(forwarded)} direct forwards) "
          f"and rewrote {len(rewritten)} Rust files")


if __name__ == "__main__":
    main()
