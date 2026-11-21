#!/usr/bin/env python3
"""Remove the visibility-storage charge threaded through section meshing (debt p3-29).

Run from the pre-change tree. The visibility scheduler keeps its own lease;
meshing no longer takes a second copy of the amount charged by that lease.
"""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-client-render/src"


def rewrite(name, edits):
    path = ROOT / name
    text = path.read_text()
    for old, new in edits:
        assert old in text, (name, old)
        text = text.replace(old, new)
    path.write_text(text)


rewrite("mesher.rs", [
    ("scratch, &visibility_plan::NoMasks, 0)", "scratch, &visibility_plan::NoMasks)"),
    ("    /// are nonoccluding air, whether the listeners returned a mask or abstained. `visibility_bytes`\n"
     "    /// is this section's preparation charge against its shared section-geometry ceiling.\n",
     "    /// are nonoccluding air, whether the listeners returned a mask or abstained.\n"),
    ("        visibility_bytes: usize,\n", ""),
    ("                visibility_bytes,\n", ""),
    ("out, scratch, plan, visibility_bytes)", "out, scratch, plan)"),
    ("mod_geometry::add(self, &mut tints, &mut out, visibility_bytes);",
     "mod_geometry::add(self, &mut tints, &mut out);"),
])
rewrite("mesher/mod_geometry.rs", [
    ("    visibility_bytes: usize,\n", ""),
    ("Budget::left_after(out, visibility_bytes)", "Budget::left_after(out, 0)"),
])
rewrite("mesher/visibility_plan_mesh_tests.rs", [
    ("&mut scratch, plan, 0)", "&mut scratch, plan)"),
    ("&mut scratch, None, 0)", "&mut scratch, None)"),
    ("        Some(&keep_all),\n        0,", "        Some(&keep_all),"),
])
rewrite("streaming/mesh_job.rs", [
    ("            super::section_visibility::REQUEST_BYTES,\n", ""),
])
