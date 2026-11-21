use super::*;
use std::collections::BTreeMap;
use sunrise_mod_runtime::block_definitions::BlockDefinition;

#[test]
fn strict_block_predicates_validate_schema_without_default_bias() {
    crate::parts::install_input(Default::default());
    let id = "test:connected";
    let off = BTreeMap::from([
        ("left".into(), "false".into()),
        ("right".into(), "false".into()),
    ]);
    let on = BTreeMap::from([
        ("left".into(), "true".into()),
        ("right".into(), "true".into()),
    ]);
    assert_eq!(
        StatePredicate::block(BlockKey(0), &[]),
        Err(BlockStateError::Unavailable)
    );
    registry_keys::resolve_block_keys([id], |_| Ok(BlockDefinition::new(&[&off, &on], 0).unwrap()))
        .unwrap();
    let key = registry_keys::block_key(id).unwrap();
    assert_eq!(
        StatePredicate::block(BlockKey(u32::MAX), &[]),
        Err(BlockStateError::UnknownBlock)
    );
    // The default is false/false, so setting just left=true would not be a complete state.
    // It is nevertheless a legal partial predicate matching the independent true/true row.
    let predicate = StatePredicate::block(key, &[StateConstraint::new("left", "true")]).unwrap();
    assert_eq!(predicate.block_key(), Some(key));
    assert_eq!(predicate.tag_key(), None);
    assert_eq!(
        predicate.constraints(),
        &[StateConstraint::new("left", "true")]
    );
    for (name, value, error) in [
        ("missing", "true", BlockStateError::UnknownProperty(1)),
        ("right", "invalid", BlockStateError::InvalidValue(1)),
        ("left", "false", BlockStateError::DuplicateProperty(1)),
    ] {
        assert_eq!(
            StatePredicate::block(
                key,
                &[
                    StateConstraint::new("left", "true"),
                    StateConstraint::new(name, value),
                ],
            ),
            Err(error)
        );
    }
}

#[test]
fn tag_predicates_keep_vague_properties_and_refuse_duplicates() {
    let tag = TagKey(0);
    let constraints = [StateConstraint::new(
        "property_no_member_has",
        "arbitrary_value",
    )];
    let predicate = StatePredicate::tag(tag, &constraints).unwrap();
    assert_eq!(predicate.constraints(), &constraints);
    assert_eq!(predicate.block_key(), None);
    assert_eq!(predicate.tag_key(), Some(tag));
    assert_eq!(
        StatePredicate::tag(
            tag,
            &[
                StateConstraint::new("unknown", "a"),
                StateConstraint::new("unknown", "b"),
            ],
        ),
        Err(BlockStateError::DuplicateProperty(1))
    );
}
