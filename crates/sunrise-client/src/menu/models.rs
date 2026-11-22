//! Engine menu records and slot access.
use super::{Env, ItemStack};
use mod_api::MenuContainer;
/// Operations on the clicked slot, including its placement and take rules.
pub trait SlotView {
    /// Run against staged owner state; commit all changes only when accepted.
    fn with_staged(&mut self, edit: &mut dyn FnMut(&mut dyn SlotView) -> bool) -> bool;
    fn items(&self) -> &sunrise_items::items::Items;
    fn item(&self) -> Option<ItemStack>;
    fn may_place(&self, stack: &ItemStack) -> bool;
    fn may_pickup(&self) -> bool;
    fn allow_modification(&self) -> bool;
    fn safe_insert(&mut self, stack: Option<ItemStack>, count: u32) -> Option<ItemStack>;
    fn safe_take(&mut self, count: u32, max: u32) -> Option<ItemStack>;
}
/// A pickup click as `item_stacked_on_other` is asked about it: the cursor and the clicked slot's
/// stack as the click finds them, before any listener runs.
#[derive(Clone, Debug, PartialEq)]
pub struct StackedClick {
    pub player: Option<u128>,
    pub player_entity_id: Option<i32>,
    pub slot: usize,
    pub container: usize,
    pub container_slot: usize,
    pub primary: bool,
    pub carried: Option<ItemStack>,
    pub clicked: Option<ItemStack>,
}
/// What the mods decided about a pickup click (`item_stacked_on_other`), validated by the game
/// before the click: `canceled` skips the bundle override, and a cancelled click that is also
/// `handled` is done, leaving the cursor and the slot as the `Some` stacks say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StackedOutcome {
    pub canceled: bool,
    pub handled: bool,
    /// The cursor after a cancelled, handled click.
    pub carried: Option<Option<ItemStack>>,
    /// The clicked slot after a cancelled, handled click.
    pub clicked: Option<Option<ItemStack>>,
    /// Set by the click when the clicked slot's own rules refuse `clicked` (it changes a slot
    /// whose `onTake` would not run, takes what `mayPickup` keeps, or puts in what `mayPlace` or
    /// the slot's size refuses): the click changed nothing and its decision is undone.
    pub refused: std::cell::Cell<bool>,
}
/// Who and where a `menu_slot_rules` ask is about: the player whose menu it is, the menu's
/// block, the container the asked slot indexes, and the menu's and player's live slots as they
/// stand at the time of the ask (read only, borrowed: player inventory `0..36`, armor `36..40`
/// feet-to-head, offhand `40`). None where the menu has no such thing: a client's prediction
/// knows no other player, and a menu opened with no block has no `pos`.
#[derive(Clone, Copy, Default)]
pub struct SlotContext<'a> {
    pub player: Option<u128>,
    pub pos: Option<[i32; 3]>,
    pub container: MenuContainer,
    pub(crate) slots: &'a [Option<ItemStack>],
    pub(crate) slots_second: &'a [Option<ItemStack>],
    pub(crate) linked: Option<crate::lan::menu::LinkedSlots<'a>>,
    /// A flattened LAN container stores grid/result values in Main's unused positions.
    /// These positions are empty in the prediction's separate Main container.
    pub(crate) main_crafting: Option<crate::containers::ModCrafting>,
    pub(crate) grid: &'a [Option<ItemStack>],
    pub(crate) result: &'a [Option<ItemStack>],
    pub(crate) player_slots: &'a [Option<ItemStack>],
    pub(crate) player_armor: Option<&'a [Option<ItemStack>; 4]>,
    pub(crate) player_offhand: Option<&'a Option<ItemStack>>,
}

/// `menu_slot_rules` inside a menu click, which cannot reach the game: the game lends the menu
/// one for the click.
pub trait SlotRules {
    /// Whether a part answers the menu type `menu`; a menu type none answers is never asked.
    fn listens(&self, menu: &str) -> bool;
    /// The slot's own answers arrive in `may_place`, `may_pickup` and `max_stack`; the parts may
    /// change them.
    /// `context` is who and where the ask is about (see [`SlotContext`]).
    fn rule(
        &self,
        menu: &str,
        slot: usize,
        stack: &ItemStack,
        context: SlotContext<'_>,
        may_place: &mut bool,
        may_pickup: &mut bool,
        max_stack: &mut u32,
    );
}

/// The anvil's inputs while asking about the result vanilla just calculated.
pub struct AnvilUpdateInputs<'a> {
    pub left: &'a ItemStack,
    pub right: Option<&'a ItemStack>,
    pub name: Option<&'a str>,
}
/// An admitted answer to `GrindstoneEvent.OnPlaceItem`: a replacement result
/// and its experience award, or no replacement to keep the vanilla calculation.
pub struct GrindstonePlaceEdits {
    pub output: Option<ItemStack>,
    /// `-1` for the vanilla award, or a nonnegative experience amount.
    pub xp: i32,
}

impl Default for GrindstonePlaceEdits {
    fn default() -> Self {
        Self {
            output: None,
            xp: -1,
        }
    }
}

/// The pre-take inputs and initial experience of a deferred grindstone result take.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingGrindstoneTake {
    pub top: Option<ItemStack>,
    pub bottom: Option<ItemStack>,
    /// `-1` for vanilla's award, or the amount set when the result was placed.
    pub xp: i32,
}

impl PendingGrindstoneTake {
    /// `GrindstoneMenu.getExperienceAmount`, before the server rolls Level.random.
    pub fn vanilla_amount(&self, env: &Env<'_>) -> i32 {
        crate::stations::grindstone_xp_base(
            env.enchantments,
            self.top.as_ref(),
            self.bottom.as_ref(),
        )
    }
}

/// The admitted experience and replacements for a taken grindstone result.
pub struct GrindstoneTakeEdits {
    /// `-1` for the award from the pre-take inputs, or a nonnegative amount.
    pub xp: i32,
    pub new_top: Option<ItemStack>,
    pub new_bottom: Option<ItemStack>,
}

impl GrindstoneTakeEdits {
    /// Finish the take without listener edits: keep the initial award and empty both slots.
    pub fn initial(xp: i32) -> Self {
        Self {
            xp,
            new_top: None,
            new_bottom: None,
        }
    }
}

/// An anvil craft as `AnvilCraftEvent` reports it: the inputs and the result
/// taken, before the crafting changed anything.
#[derive(Clone, Debug, PartialEq)]
// exhaustive: the game builds it with a literal; mods only read it
pub struct AnvilCraftSnapshot {
    pub left: Option<ItemStack>,
    pub right: Option<ItemStack>,
    pub output: ItemStack,
}
