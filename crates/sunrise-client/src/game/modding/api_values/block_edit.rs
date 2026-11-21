//! Cold, all-or-nothing preparation of typed bulk edit values.
use super::{TryIntoApi, TryIntoEngine, block_metadata_table};
use mod_api::{BlockMetadata, StateConstraint, StatePredicate};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};
use sunrise_world::registry::{BlockRegistry, BlockStateId};

pub(in crate::game) struct PreparedBlockEdit<'a> {
    pub(in crate::game) palette: Vec<u32>,
    pub(in crate::game) filters: Vec<CompiledBlockFilter<'a>>,
}

/// Resolve every palette state and compile every filter before a world mutation is possible.
pub(in crate::game) fn prepare(edit: &mod_api::BlockEdit) -> Result<PreparedBlockEdit<'_>, String> {
    let palette = edit
        .palette
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let state: BlockStateId = state
                .try_into_engine(&())
                .map_err(|error| format!("palette entry {index}: {error:?}"))?;
            Ok(state.0)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let filters = edit
        .filters
        .iter()
        .enumerate()
        .map(|(index, predicate)| {
            compile_filter(predicate).map_err(|error| format!("filter {index}: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PreparedBlockEdit { palette, filters })
}

/// A resolved name and the predicate's original, already validated constraints.
pub(in crate::game) struct CompiledBlockFilter<'a> {
    id: Cow<'static, str>,
    tag: bool,
    constraints: &'a [StateConstraint],
}

impl CompiledBlockFilter<'_> {
    pub(in crate::game) fn matches(
        &self,
        registry: &BlockRegistry,
        tags: [&HashMap<String, HashSet<String>>; 2],
        state: BlockStateId,
    ) -> bool {
        crate::game::commands::block_predicate_matches_state(
            registry,
            tags,
            &self.id,
            self.tag,
            self.constraints
                .iter()
                .map(|constraint| (constraint.name.as_str(), constraint.value.as_str())),
            state,
        )
    }
}

fn compile_filter(predicate: &StatePredicate) -> Result<CompiledBlockFilter<'_>, String> {
    let (id, tag) = if let Some(key) = predicate.block_key() {
        let metadata =
            block_metadata_table().map_err(|error| format!("block metadata: {error:?}"))?;
        let name = metadata
            .name(key)
            .map_err(|error| format!("block: {error:?}"))?;
        (Cow::Borrowed(name), false)
    } else {
        // Only the tag's name is retained here; membership is read when applying the edit.
        let name = predicate
            .tag_key()
            .and_then(|key| mod_api::__private::with_tag_name(key, |name| name.map(str::to_owned)))
            .ok_or("block tag is unavailable")?;
        (Cow::Owned(name), true)
    };
    Ok(CompiledBlockFilter {
        id,
        tag,
        constraints: predicate.constraints(),
    })
}

/// Convert only the distinct numeric palette entries, preserving complete property states.
pub(in crate::game) fn column_palette(
    palette: Vec<BlockStateId>,
) -> Result<Vec<mod_api::BlockState>, String> {
    palette
        .try_into_api()
        .map_err(|error| format!("column palette: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> BlockRegistry {
        mod_platform::__private::part_tests::install_parts_fixture(&[]).unwrap();
        let facts = r#"{"defaults":{"minecraft:air":0,"minecraft:stairs":1},"states":[["a",true,false,"","minecraft:air"],["s",false,false,"",{"id":"minecraft:stairs","properties":{"left":"false","right":"false"}}],["s",false,false,"",{"id":"minecraft:stairs","properties":{"left":"true","right":"true"}}]]}"#;
        let registry = BlockRegistry::from_facts_json(facts.as_bytes()).unwrap();
        super::super::resolve_block_definitions(&registry).unwrap();
        registry
    }

    #[test]
    fn typed_bulk_sparse_predicates_use_existing_matcher_and_unknown_tag_properties_never_match() {
        let registry = registry();
        let metadata = block_metadata_table().unwrap();
        let state = BlockStateId(2).try_into_api().unwrap();
        let exact = StatePredicate::block(
            metadata,
            state.block(),
            &[StateConstraint::new("left", "true")],
        )
        .unwrap();
        let filter = compile_filter(&exact).unwrap();
        let tags = HashMap::from([(
            "stairs".to_owned(),
            HashSet::from(["minecraft:stairs".to_owned()]),
        )]);
        let tags = [&tags, &tags];
        assert!(filter.matches(&registry, tags, BlockStateId(2)));
        assert!(!filter.matches(&registry, tags, BlockStateId(1)));
        let wildcard = StatePredicate::block(metadata, state.block(), &[]).unwrap();
        let wildcard = compile_filter(&wildcard).unwrap();
        assert!(wildcard.matches(&registry, tags, BlockStateId(1)));
        assert!(wildcard.matches(&registry, tags, BlockStateId(2)));

        // Use resolved tag names and local membership, without interning test keys process-wide.
        let tag_matches = |id: &'static str, constraints: &[StateConstraint], state| {
            CompiledBlockFilter {
                id: Cow::Borrowed(id),
                tag: true,
                constraints,
            }
            .matches(&registry, tags, BlockStateId(state))
        };
        assert!(tag_matches("minecraft:stairs", &[], 2));
        assert!(!tag_matches("minecraft:stairs", &[], 0));
        assert!(!tag_matches(
            "minecraft:stairs",
            &[StateConstraint::new("unknown", "anything")],
            2,
        ));
        assert!(!tag_matches(
            "minecraft:stairs",
            &[StateConstraint::new("left", "invalid")],
            2,
        ));
        assert!(!tag_matches("test:empty", &[], 2));
    }

    #[test]
    fn typed_bulk_preparation_rejects_late_bad_values_before_any_write() {
        let _registry = registry();
        let state = BlockStateId(2).try_into_api().unwrap();
        let good =
            StatePredicate::block(block_metadata_table().unwrap(), state.block(), &[]).unwrap();
        let mut edit = mod_api::BlockEdit {
            palette: vec![state],
            filters: vec![good],
            runs: vec![],
        };
        let prepared = prepare(&edit).unwrap();
        assert_eq!(prepared.palette, [2]);
        assert_eq!(prepared.filters.len(), 1);
        edit.palette
            .push(mod_api::__private::block_state_from_ordinal(
                state.block(),
                u16::MAX,
            ));
        assert!(
            prepare(&edit).is_err(),
            "late bad palette state must refuse the whole prepared edit"
        );
        let palette = column_palette(vec![BlockStateId(1), BlockStateId(2)]).unwrap();
        assert_eq!(
            palette
                .iter()
                .map(|state| state.ordinal())
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(
            palette[0]
                .property(block_metadata_table().unwrap(), "left")
                .unwrap(),
            Some("false")
        );
        assert_eq!(
            palette[1]
                .property(block_metadata_table().unwrap(), "right")
                .unwrap(),
            Some("true")
        );
    }
}
