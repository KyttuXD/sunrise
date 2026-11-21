#!/usr/bin/env python3
"""Separate processor working block data from the template's stored BlockInfo."""
from pathlib import Path
import re

root = Path("crates/sunrise-world/src/worldgen/structure")
for name in ("processor.rs", "processor/pillars.rs", "processor/sea_level.rs", "processor/litho.rs"):
    path = root / name
    source = path.read_text()
    source = re.sub(r"\bBlockInfo\b", "WorkingBlock", source)
    source = source.replace("info: WorkingBlock,", "info: WorkingBlock<'a>,")
    source = source.replace("fn process<W:", "fn process<'a, W:")
    source = source.replace("-> ProcessResult {", "-> ProcessResult<'a> {")
    if name == "processor.rs":
        source = source.replace("use std::{collections", "use std::{borrow::Cow, collections")
        source = source.replace("super::{WorkingBlock,", "super::{BlockInfo,", 1)
        source = source.replace(
            "type ProcessResult = Result<Option<WorkingBlock>, PlacementQueryError>;",
            """/// The placement-local block info; a delegate may borrow the working tag.
#[derive(Clone, PartialEq)]
struct WorkingBlock<'a> {
    pos: [i32; 3],
    state: BlockStateId,
    nbt: Option<Cow<'a, Compound>>,
}

type ProcessResult<'a> = Result<Option<WorkingBlock<'a>>, PlacementQueryError>;""",
        )
        for function in ("process_list", "process_block"):
            source = source.replace(f"fn {function}<W:", f"fn {function}<'a, W:")
        source = source.replace("fn block_age(", "fn block_age<'a>(")
        source = source.replace("-> Option<WorkingBlock> {", "-> Option<WorkingBlock<'a>> {")
        source = source.replace(
            "fn apply(&self, info: WorkingBlock<'a>, random: &mut dyn ProcessorRandom) -> WorkingBlock {",
            "fn apply<'a>(&self, info: WorkingBlock<'a>, random: &mut dyn ProcessorRandom) -> WorkingBlock<'a> {",
        )
        source = source.replace("outcome.nbt.clone().map(Box::new)", "outcome.nbt.clone().map(Cow::Owned)")
        source = source.replace("nbt.get_or_insert_with(Default::default)\n                .insert", "nbt.get_or_insert_with(|| Cow::Owned(Compound::new()))\n                .to_mut()\n                .insert")
        source = re.sub(r"(\.apply\(info\.nbt\.as_deref\(\), &mut random\)\s*\.map)\(Box::new\)", r"\1(Cow::Owned)", source)
        source = source.replace("Some(Box::new(spawner.nbt(entity)))", "Some(Cow::Owned(spawner.nbt(entity)))")
        source = source.replace("-> Result<Vec<WorkingBlock>, PlacementQueryError>", "-> Result<Vec<BlockInfo>, PlacementQueryError>")
        source = source.replace("nbt: info.nbt.clone(),", "nbt: info.nbt.as_deref().cloned().map(Cow::Owned),", 1)
        source = source.replace("    Ok(processed)\n}", """    Ok(processed.into_iter().map(|info| BlockInfo {
        pos: info.pos,
        state: info.state,
        nbt: info.nbt.map(|nbt| Box::new(nbt.into_owned())),
    }).collect())
}""", 1)
        source = source.replace("Some(Box::default())", "Some(Cow::Owned(Compound::new()))")
    elif name == "processor/pillars.rs":
        source = source.replace("ProcessResult, ProcessorList, ProcessorRandom, process_list", "ProcessResult, ProcessorList, ProcessorRandom, WorkingBlock, process_list")
        source = source.replace("structure::{WorkingBlock, StructureResources}", "structure::StructureResources")
        source = source.replace("-> ProcessResult<'a> {\n    let info = WorkingBlock", "-> ProcessResult<'static> {\n    let info = WorkingBlock", 1)
    elif name == "processor/sea_level.rs":
        source = source.replace("ProcessResult, pillars", "ProcessResult, WorkingBlock, pillars")
        source = source.replace(", structure::WorkingBlock", "")
        source = source.replace("-> WorkingBlock {", "-> WorkingBlock<'static> {")
        source = source.replace("i: WorkingBlock", "i: WorkingBlock<'_>")
    elif name == "processor/litho.rs":
        source = source.replace("Option<WorkingBlock>", "Option<WorkingBlock<'static>>")
    path.write_text(source)
