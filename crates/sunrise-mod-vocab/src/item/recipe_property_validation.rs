//! Scoped, attributed declarations for [`crate::RecipePropertySetsEvent`].
use super::RecipePropertyAddition;
use crate::Answered;

impl crate::RecipePropertySetsEvent<'_> {
    /// Add items to a recipe property set. Later additions extend the set without changing
    /// its first contributor; no listener can change earlier additions.
    ///
    /// A bare key names a vanilla set. A qualified key names a vanilla set only if it
    /// matches a built-in set exactly; any other valid qualified key creates a custom set.
    /// Malformed keys are reported with the mod ID and skipped.
    pub fn register(&mut self, addition: RecipePropertyAddition) {
        let mod_id = self.mod_id;
        if addition.set.contains(':')
            && crate::Identifier::parse_namespaced(&addition.set).is_none()
        {
            log::warn!(
                "recipe_property_sets: mod {mod_id}: invalid set key {}",
                addition.set
            );
            return;
        }
        self.additions.push(Answered {
            answer: addition,
            mod_id,
        });
    }
}
