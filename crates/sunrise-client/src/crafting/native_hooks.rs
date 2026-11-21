//! Load and match hook adapters at the engine boundary.
use mod_api::{IngredientDefinition, RecipePropertyAddition};
use sunrise_world::native_load_context::LoadContext;

/// Ask for this occurrence's custom ingredient definition before parsing the recipe tables.
/// The first answer supplies ordered candidates and the owning mod's match predicate.
#[inline(never)]
pub fn native_ingredient(
    context: &sunrise_world::native_load_context::LoadContext,
    kind: &str,
    data: &sunrise_core::nbt::Compound,
    definition: &mut Option<mod_api::Answered<IngredientDefinition>>,
) {
    mod_api::dispatch_ingredient(
        &sunrise_client_render::api_context::LoadReader(context).context(),
        kind,
        data,
        definition,
    );
}
/// Collect attributed contributions to recipe property sets once in the owning recipe load,
/// after recipe resolution and before station acceptance and recipe sync project the sets.
#[inline(never)]
pub fn native_recipe_property_sets(
    context: &LoadContext,
    additions: &mut Vec<mod_api::Answered<RecipePropertyAddition>>,
) {
    mod_api::dispatch_recipe_property_sets(
        &sunrise_client_render::api_context::LoadReader(context).context(),
        additions,
    );
}
/// Whether two of `stack`'s item may be combined in a crafting grid to repair it
/// (`IItemExtension.isCombineRepairable`, which `RepairItemRecipe.canCombine` asks) is being
/// decided: `repairable` starts with vanilla's answer, whether the stack has durability, and
/// the parts of the stack's item may change it. Asked inside recipe matching on both sides.
#[inline(never)]
pub fn native_is_combine_repairable(
    context: &sunrise_world::native_load_context::LoopContext,
    stack: &mod_api::ItemStack,
    repairable: &mut bool,
) {
    mod_api::dispatch_is_combine_repairable(
        &sunrise_client_render::api_context::LoopReader(context).context(),
        stack,
        repairable,
    );
}
