#!/usr/bin/env python3
"""Update fixtures and probes for per-packet and per-record write errors."""
from pathlib import Path

root = Path("crates/sunrise-client")


def update(file, *replacements):
    path = root / file
    source = path.read_text()
    for before, after in replacements:
        assert before in source, (file, before)
        source = source.replace(before, after)
    path.write_text(source)


calls = (
    "encoder.body_in(&centre, &world)",
    "encoder.body_with(&centre, &world, &proto)",
    "encoder.body_with(chunk, &world, &|_, _| Some(SectionMask::ALL))",
    "encoder.body_in(chunk, &world)",
    "encoder.body_in(&world.chunks[&(0, 0)], &world)",
    "encoder.body(&chunk, &entities, around)",
    "encoder.body(&chunk, &entities, SectionMask::ALL)",
    "alone.body_in(c, &world)",
    "encoder.body_in(c, &world)",
)
update("src/lan/chunks.rs", *((call, f"{call}.unwrap()") for call in calls))
update("examples/chunk_encoding_bench.rs", ("encoder.body_in(c, world)", "encoder.body_in(c, world).unwrap()"))
update("examples/probe/light_parity/ordered_padding.rs", ("encoder.body_in(chunk, &world);", "encoder.body_in(chunk, &world)?;"))
update(
    "src/lan/viewed_levels/tests.rs",
    ("oversized.data.clone(), body((1, 0))", "Ok(oversized.data.clone()), body((1, 0))"),
    ("&taken, &bodies);", "&taken, &bodies).unwrap();"),
    ("&[oversized.data]);", "&[Ok(oversized.data)]).unwrap();"),
)
refused = '|(x, z), error| panic!("chunk ({x}, {z}): {error}")'
update(
    "src/save.rs",
    ("folder, &[((0, 0), &record)]).unwrap();", f"folder, &[((0, 0), &record)], {refused}).unwrap();"),
    ("&[((1, 0), &current)],\n        )", f"&[((1, 0), &current)],\n            {refused},\n        )"),
)
update("src/save/far_regions.rs", ('"region", records).unwrap();', f'"region", records, {refused}).unwrap();'))
