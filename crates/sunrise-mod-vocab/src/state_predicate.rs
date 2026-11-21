//! Typed partial block predicates for prepared bulk edits.
use crate::{BlockKey, BlockMetadata, BlockState, BlockStateError, StateConstraint, TagKey, Tags};

/// A block or tag filter, validated on construction, with unspecified properties left as
/// wildcards. An unknown or missing property on a tag member never matches.
// no wire: a native filter over process-local block and tag keys.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct StatePredicate {
    target: Target,
    constraints: Vec<StateConstraint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Block(BlockKey),
    Tag(TagKey),
}
impl StatePredicate {
    /// A block filter; unknown properties, illegal values and duplicate properties refuse.
    /// A partial filter need not produce the default state's complete combination.
    pub fn block(
        metadata: &dyn BlockMetadata,
        block: BlockKey,
        constraints: &[StateConstraint],
    ) -> Result<Self, BlockStateError> {
        let target = Target::Block(block);
        validate(metadata, target, constraints)?;
        Ok(Self {
            target,
            constraints: constraints.to_vec(),
        })
    }
    /// A tag filter; duplicate properties refuse, but arbitrary properties and empty tags remain valid.
    /// Membership is evaluated against authoritative tags at application, not frozen here.
    pub fn tag(
        metadata: &dyn BlockMetadata,
        _tags: &dyn Tags,
        tag: TagKey,
        constraints: &[StateConstraint],
    ) -> Result<Self, BlockStateError> {
        let target = Target::Tag(tag);
        validate(metadata, target, constraints)?;
        Ok(Self {
            target,
            constraints: constraints.to_vec(),
        })
    }
    /// The exact block this predicate selects, if it is not a tag filter.
    pub fn block_key(&self) -> Option<BlockKey> {
        match self.target {
            Target::Block(key) => Some(key),
            Target::Tag(_) => None,
        }
    }
    /// The tag this predicate selects, if it is not an exact block filter.
    pub fn tag_key(&self) -> Option<TagKey> {
        match self.target {
            Target::Tag(key) => Some(key),
            Target::Block(_) => None,
        }
    }
    /// Explicit property requirements; all must match, but omitted properties are unrestricted.
    pub fn constraints(&self) -> &[StateConstraint] {
        &self.constraints
    }
}
fn validate(
    metadata: &dyn BlockMetadata,
    target: Target,
    constraints: &[StateConstraint],
) -> Result<(), BlockStateError> {
    let default = match target {
        Target::Block(block) => Some(BlockState::of(metadata, block, &[])?),
        Target::Tag(_) => None,
    };
    for (input, constraint) in constraints.iter().enumerate() {
        if constraints[..input]
            .iter()
            .any(|previous| previous.name == constraint.name)
        {
            return Err(BlockStateError::DuplicateProperty(input as u32));
        }
        if let Some(default) = default {
            match default.with(metadata, &constraint.name, &constraint.value) {
                // This is schema validation, not filling unspecified fields from defaults.
                // A legal partial property can match a nondefault combination even if this
                // particular complete default-plus-one combination does not exist.
                Ok(_) | Err(BlockStateError::InvalidCombination) => {}
                Err(BlockStateError::UnknownProperty(_)) => {
                    return Err(BlockStateError::UnknownProperty(input as u32));
                }
                Err(BlockStateError::InvalidValue(_)) => {
                    return Err(BlockStateError::InvalidValue(input as u32));
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "state_predicate/tests.rs"]
mod tests;
