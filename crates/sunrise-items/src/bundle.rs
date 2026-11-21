//! `BundleContents.Mutable`: what a bundle takes in and gives back, by
//! weight. The menus (`menu`, `lan::menu`) run `BundleItem`'s click
//! overrides on it.
use crate::item_data::BundleSelected;
use crate::items::{ItemInfo, ItemStack, Items};
use crate::properties::ItemBar;
use sunrise_core::nbt::Tag;

/// A bundle weight: vanilla's `Fraction`, exact and kept reduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Weight {
    n: i128,
    d: i128,
}

fn gcd(a: i128, b: i128) -> i128 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}

impl Weight {
    const ZERO: Self = Self { n: 0, d: 1 };
    const ONE: Self = Self { n: 1, d: 1 };

    fn new(n: i128, d: i128) -> Self {
        let g = gcd(n, d);
        Self { n: n / g, d: d / g }
    }

    fn add(self, o: Self) -> Option<Self> {
        let common = gcd(self.d, o.d);
        let a = self.n.checked_mul(o.d / common)?;
        let b = o.n.checked_mul(self.d / common)?;
        Some(Self::new(
            a.checked_add(b)?,
            self.d.checked_mul(o.d / common)?,
        ))
    }

    fn sub(self, o: Self) -> Option<Self> {
        self.add(Self { n: -o.n, d: o.d })
    }

    fn times(self, count: u32) -> Option<Self> {
        let count = i128::from(count);
        let common = gcd(count, self.d);
        Some(Self::new(
            self.n.checked_mul(count / common)?,
            self.d / common,
        ))
    }

    /// `this.divideBy(item).intValue()` (truncated), at least 0.
    fn how_many(self, item: Self) -> Option<u32> {
        if self.n <= 0 {
            return Some(0);
        }
        let num_common = gcd(self.n, item.n);
        let den_common = gcd(self.d, item.d);
        let n = (self.n / num_common).checked_mul(item.d / den_common)?;
        let d = (self.d / den_common).checked_mul(item.n / num_common)?;
        Some(u32::try_from(n / d).unwrap_or(u32::MAX))
    }

    fn room_for(self, item: Self) -> Option<u32> {
        // BundleContents.tryTransfer asks before taking anything from the slot.
        // The sum must also be representable for any amount the slot supplies.
        self.d.checked_mul(item.d / gcd(self.d, item.d))?;
        Self::ONE.sub(self)?.how_many(item)
    }
}

/// The sounds of `BundleItem` (`Entity.playSound` at the player): insert
/// and remove one at volume 0.8 and pitch `0.8 + random * 0.4`, a failed
/// insert at 1, 1; the drop of its contents is played at the player's block
/// like remove one.
pub const INSERT: &str = "minecraft:item.bundle.insert";
pub const INSERT_FAIL: &str = "minecraft:item.bundle.insert_fail";
pub const REMOVE_ONE: &str = "minecraft:item.bundle.remove_one";
pub const DROP_CONTENTS: &str = "minecraft:item.bundle.drop_contents";

pub(crate) fn max_stack(items: &Items, stack: &ItemStack) -> u32 {
    let info = items.get(&stack.item);
    if !stack.carries_component(info, "minecraft:max_stack_size") {
        return 1;
    }
    stack
        .data()
        .and_then(|data| data.other("minecraft:max_stack_size"))
        .and_then(crate::properties::component_int)
        .unwrap_or_else(|| info.map_or(1, |item| item.max_stack as i32))
        .max(1) as u32
}

/// `ItemInstance.get(BUNDLE_CONTENTS)`: borrow the explicit value (even if empty),
/// or inherit the prototype's value unless the stack removed the component.
pub fn contents_of<'a>(
    stack: &'a ItemStack,
    info: Option<&'a ItemInfo>,
) -> Option<&'a [ItemStack]> {
    if let Some(data) = stack.data() {
        if data.sets_component("minecraft:bundle_contents") {
            return Some(&data.bundle_contents);
        }
        if data.other("!minecraft:bundle_contents").is_some() {
            return None;
        }
    }
    let data = info?.component_prototype.data()?;
    data.sets_component("minecraft:bundle_contents")
        .then_some(data.bundle_contents.as_slice())
}

/// `BundleContents.getWeight`: a nested bundle weighs its contents plus
/// 1/16, a beehive with bees 1, the rest 1 / max stack size.
fn item_weight(
    items: &Items,
    stack: &ItemStack,
    parent: Option<&WeightPath<'_>>,
) -> Option<Weight> {
    if let Some(inside) = contents_of(stack, items.get(&stack.item)) {
        return contents_weight(items, inside, parent)?.add(Weight::new(1, 16));
    }
    let bees = stack
        .data()
        .and_then(|d| d.other("minecraft:bees"))
        .is_some_and(|b| matches!(b, Tag::List(l) if !l.is_empty()));
    Some(if bees {
        Weight::ONE
    } else {
        Weight::new(1, i128::from(max_stack(items, stack)))
    })
}

struct WeightPath<'a> {
    contents: &'a [ItemStack],
    parent: Option<&'a WeightPath<'a>>,
}

fn contents_weight(
    items: &Items,
    inside: &[ItemStack],
    parent: Option<&WeightPath<'_>>,
) -> Option<Weight> {
    if inside.is_empty() {
        return Some(Weight::ZERO);
    }
    // An edited prototype may refer to itself through a nested item. An
    // uncomputable weight is an error, as in BundleContents.computeContentWeight.
    let mut ancestor = parent;
    while let Some(path) = ancestor {
        if std::ptr::eq(inside, path.contents) {
            return None;
        }
        ancestor = path.parent;
    }
    let path = WeightPath {
        contents: inside,
        parent,
    };
    sunrise_core::nbt::with_tree_stack(|| {
        inside.iter().try_fold(Weight::ZERO, |w, s| {
            w.add(item_weight(items, s, Some(&path))?.times(s.count)?)
        })
    })
}

/// `BundleItem.isBarVisible`, `getBarWidth` and `getBarColor`, over the
/// effective component. A removed component has no bar.
pub(crate) fn stack_bar(items: &Items, stack: &ItemStack, info: &ItemInfo) -> Option<ItemBar> {
    let inside = contents_of(stack, Some(info))?;
    let Weight { n, d } = contents_weight(items, inside, None).unwrap_or(Weight::ONE);
    if n == 0 {
        return None;
    }
    Some(match n.checked_mul(12) {
        Some(scaled) if n < d => ItemBar {
            width: 1 + (scaled / d) as i32,
            color: 0x7087FF,
        },
        _ => ItemBar {
            width: 13,
            color: 0xFF5454,
        },
    })
}

/// `BundleItem.onDestroyed` (`ItemStack.onDestroyed` for a bundle): the stack
/// is emptied and every stack it held comes back as its own item copy, which
/// `ItemUtils.onContainerDestroyed` drops where the entity lay.
pub fn destroyed(stack: &mut ItemStack) -> Vec<ItemStack> {
    let Some(data) = stack.extra.as_deref_mut() else {
        return Vec::new();
    };
    if data.bundle_contents.is_empty() {
        return Vec::new();
    }
    // `set(BUNDLE_CONTENTS, BundleContents.EMPTY)`: no contents and no
    // selected index left.
    let spilled = std::mem::take(&mut data.bundle_contents);
    data.bundle_selected = BundleSelected(-1);
    stack.tidy();
    spilled
}

/// `BundleContents.Mutable`.
pub struct Contents {
    items: Vec<ItemStack>,
    weight: Option<Weight>,
    /// `selectedItem`: an index into `items`, -1 for none.
    selected: i32,
}

impl Contents {
    /// A component collection may belong to any item, including an item with
    /// no previous bundle_contents component. Reuse the same weight model.
    pub fn from_items(items: &Items, inside: Vec<ItemStack>) -> Self {
        let weight = contents_weight(items, &inside, None);
        Self {
            items: inside,
            weight,
            selected: -1,
        }
    }
    /// The effective contents; None when the stack lacks a bundle contents component.
    pub fn of(items: &Items, stack: &ItemStack) -> Option<Self> {
        let inside = contents_of(stack, items.get(&stack.item))?;
        let weight = contents_weight(items, inside, None);
        let selected = stack.data().map_or(-1, |d| d.bundle_selected.0);
        Some(Self {
            items: inside.to_vec(),
            weight,
            selected,
        })
    }

    /// Remaining capacity for `offered` in a stack's bundle component, without cloning contents.
    pub fn room_in_stack(items: &Items, bundle: &ItemStack, offered: &ItemStack) -> Option<u32> {
        let inside = contents_of(bundle, items.get(&bundle.item))?;
        let room = contents_weight(items, inside, None)
            .and_then(|weight| weight.room_for(item_weight(items, offered, None)?));
        Some(room.unwrap_or(0))
    }

    /// `getMaxAmountToAdd` for `stack`'s kind of item.
    pub fn room_for(&self, items: &Items, stack: &ItemStack) -> u32 {
        self.weight
            .and_then(|weight| weight.room_for(item_weight(items, stack, None)?))
            .unwrap_or(0)
    }

    /// `tryInsert`: what fits moves to the front, merged with its kind. `fits` is the stack's
    /// `canFitInsideContainerItems` answer, which `canItemBeInBundle` asks and the caller
    /// gives, since the parts of the stack's item may change it; an empty stack never goes in.
    pub fn try_insert(&mut self, items: &Items, stack: &mut ItemStack, fits: bool) -> u32 {
        if stack.count == 0 || !fits {
            return 0;
        }
        let Some((amount, new_weight)) = self.weight.and_then(|current| {
            let weight = item_weight(items, stack, None)?;
            let amount = stack.count.min(current.room_for(weight)?);
            Some((amount, current.add(weight.times(amount)?)?))
        }) else {
            return 0;
        };
        if amount == 0 {
            return 0;
        }
        self.weight = Some(new_weight);
        // ItemStack.isStackable: a damaged stack is damageable.
        let stackable = max_stack(items, stack) > 1 && stack.damage == 0;
        let same = self
            .items
            .iter()
            .position(|s| stackable && s.same_item_same_components(stack));
        let added = match same {
            Some(i) => {
                let mut merged = self.items.remove(i);
                merged.count += amount;
                stack.count -= amount;
                merged
            }
            None => {
                let mut part = stack.clone();
                part.count = amount;
                stack.count -= amount;
                part
            }
        };
        self.items.insert(0, added);
        amount
    }

    /// `removeOne`: the selected stack, else the first; nothing is selected after.
    pub fn remove_one(&mut self, items: &Items) -> Option<ItemStack> {
        if self.items.is_empty() {
            return None;
        }
        let at =
            usize::try_from(self.selected).map_or(0, |i| if i < self.items.len() { i } else { 0 });
        let stack = &self.items[at];
        self.weight = Some(
            self.weight?
                .sub(item_weight(items, stack, None)?.times(stack.count)?)?,
        );
        let stack = self.items.remove(at);
        self.selected = -1;
        Some(stack)
    }

    /// `getSelectedItemIndex`.
    pub fn selected(&self) -> i32 {
        self.selected
    }

    /// The stacks inside, in display order (the first is the next out).
    pub fn items(&self) -> &[ItemStack] {
        &self.items
    }

    /// `getNumberOfItemsToShow`: a full grid shows 11 and a `+N`, so the last
    /// row keeps its fourth cell for the count.
    pub fn shown_count(&self) -> usize {
        let count = self.items.len();
        let available: usize = if count > 12 { 11 } else { 12 };
        let on_last_row = count % 4;
        let empty = if on_last_row == 0 { 0 } else { 4 - on_last_row };
        count.min(available.saturating_sub(empty))
    }

    /// The items the tooltip shows, and how many sit beyond them.
    pub fn shown(&self) -> (&[ItemStack], u32) {
        let n = self.shown_count();
        let hidden = self.items[n..].iter().map(|s| s.count).sum();
        (&self.items[..n], hidden)
    }

    /// The `Fraction` weight as the float the progress bar reads.
    pub fn weight_fraction(&self) -> f32 {
        let Weight { n, d } = self.weight.unwrap_or(Weight::ONE);
        n as f32 / d as f32
    }

    /// `Fraction.ONE`: a full bundle draws the full bar and says so.
    pub fn is_full(&self) -> bool {
        self.weight.is_none_or(|weight| weight.n >= weight.d)
    }

    /// `toImmutable`, written back into the bundle's component. Empty contents
    /// still set the component if they differ from the item's prototype.
    pub fn store(self, items: &Items, bundle: &mut ItemStack) {
        if self.weight.is_none() {
            return;
        }
        let prototype = items
            .get(&bundle.item)
            .and_then(|info| contents_of(&info.component_prototype, None));
        let empty_patch = self.items.is_empty() && prototype.is_none_or(|items| !items.is_empty());
        let data = bundle.data_mut();
        data.set_other("!minecraft:bundle_contents", None);
        data.set_other(
            "minecraft:bundle_contents",
            empty_patch.then(|| Tag::List(Vec::new())),
        );
        data.bundle_contents = self.items;
        data.bundle_selected = BundleSelected(self.selected);
        bundle.tidy();
    }
}

/// `BundleItem.toggleSelectedItem`: select `index`, or nothing when it is the
/// selected one already or outside the effective contents.
pub fn toggle_selected(items: &Items, bundle: &mut ItemStack, index: i32) {
    let Some(inside) = contents_of(bundle, items.get(&bundle.item)) else {
        return;
    };
    let previous = bundle.data().map_or(-1, |d| d.bundle_selected.0);
    let selected = if index != previous && (0..inside.len() as i32).contains(&index) {
        index
    } else {
        -1
    };
    if selected != previous {
        bundle.data_mut().bundle_selected = BundleSelected(selected);
        // Selection is ignored by StackData equality, so tidying a selection-only
        // patch would drop it while the contents are inherited.
        if selected == -1 {
            bundle.tidy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Weight;

    #[test]
    fn weights_add_up_exactly() {
        let w = Weight::new(1, 64)
            .times(32)
            .unwrap()
            .add(Weight::new(1, 16).times(4).unwrap())
            .unwrap();
        assert_eq!(w, Weight::new(3, 4));
        assert_eq!(
            Weight::ONE.sub(w).unwrap().how_many(Weight::new(1, 64)),
            Some(16)
        );
        assert_eq!(
            Weight::ONE.sub(Weight::ONE).unwrap().how_many(Weight::ONE),
            Some(0)
        );
        // An over-full bundle (made by a command) has no room.
        let over = Weight::ONE.sub(Weight::new(5, 4)).unwrap();
        assert_eq!(over.how_many(Weight::new(1, 64)), Some(0));
    }
}

// ------------------------------------------------------------- the tooltip

/// `ClientBundleTooltip`'s geometry, relative to the tooltip box's top-left:
/// the 96 pixel column of a 4 wide slot grid and the progress bar under it.
#[derive(Clone, Debug, PartialEq)]
pub struct TooltipLayout {
    /// The component's own width (96) and height, the text's excluded.
    pub width: f32,
    pub height: f32,
    /// Each slot's top-left and the stack in it, in drawing order.
    pub slots: Vec<([f32; 2], ItemStack)>,
    /// The position in `slots` of the selected item, which is drawn highlighted.
    pub selected: Option<usize>,
    /// The `+N` of the items past the grid: its top-left and the count.
    pub overflow: Option<([f32; 2], u32)>,
    /// The progress bar's top-left, its fill's width out of 94, and whether
    /// the weight fills it (the full sprite).
    pub bar: [f32; 2],
    pub fill: f32,
    pub full: bool,
    /// `item.minecraft.bundle.empty` / `.full`, centred over the bar.
    pub label: Option<&'static str>,
    /// An empty bundle's description, wrapped at `width` from here.
    pub empty_description: Option<[f32; 2]>,
}

/// The tooltip of a bundle's contents (`ClientBundleTooltip`), or None when
/// the stack is not a bundle. `x`/`y` are the box's top-left, `box_width` the
/// tooltip's width, which the component is centred in.
pub fn tooltip_layout(
    items: &Items,
    stack: &ItemStack,
    x: f32,
    y: f32,
    box_width: f32,
    description_lines: usize,
) -> Option<TooltipLayout> {
    let contents = Contents::of(items, stack)?;
    contents.weight?;
    let offset = ((box_width - 96.0) / 2.0).floor();
    if contents.items().is_empty() {
        // The description takes the top; the bar follows it. Its lines are
        // the caller's font's (`getEmptyBundleDescriptionTextHeight`).
        let description_height = 9.0 * description_lines as f32;
        return Some(TooltipLayout {
            width: 96.0,
            height: description_height + 13.0 + 8.0,
            slots: Vec::new(),
            selected: None,
            overflow: None,
            bar: [x + offset, y + description_height + 4.0],
            fill: 0.0,
            full: false,
            label: Some("item.minecraft.bundle.empty"),
            empty_description: Some([x + offset, y]),
        });
    }
    let (shown, hidden) = contents.shown();
    // `gridSizeY` counts `slotCount()` (at most twelve), not the slots the
    // overflow count leaves room for.
    let rows = contents.items().len().min(12).div_ceil(4).max(1);
    let grid_height = rows as f32 * 24.0;
    let x_start = x + offset + 96.0;
    let y_start = y + grid_height;
    let mut slots = Vec::with_capacity(shown.len());
    let mut selected = None;
    let mut overflow = None;
    // Rows fill bottom-up and columns right-to-left, as vanilla draws them.
    let mut slot = 0;
    for row in 1..=rows {
        for column in 1..=4 {
            let at = [x_start - column as f32 * 24.0, y_start - row as f32 * 24.0];
            if hidden > 0 && row == 1 && column == 1 {
                overflow = Some((at, hidden));
            } else if slot < shown.len() {
                // `extractSlot`: the cell of slot number n holds item `size - n`.
                let index = shown.len() - 1 - slot;
                if usize::try_from(contents.selected()) == Ok(index) {
                    selected = Some(slots.len());
                }
                slots.push((at, shown[index].clone()));
                slot += 1;
            }
        }
    }
    let weight = contents.weight_fraction();
    Some(TooltipLayout {
        width: 96.0,
        height: grid_height + 13.0 + 8.0,
        slots,
        selected,
        overflow,
        bar: [x + offset, y + grid_height + 4.0],
        // `Mth.mulAndTruncate(weight, 94)`, kept within the bar.
        fill: (weight * 94.0).trunc().clamp(0.0, 94.0),
        full: contents.is_full(),
        label: if contents.is_full() {
            Some("item.minecraft.bundle.full")
        } else {
            None
        },
        empty_description: None,
    })
}

#[cfg(test)]
mod tooltip_tests {
    use super::*;

    /// The prepared resources, when they are on this machine: the tooltip
    /// asks the item registry whether a stack is a bundle.
    fn items() -> Option<Items> {
        crate::items::prepared_items()
    }

    fn bundle_with(counts: &[(&str, u32)]) -> ItemStack {
        let mut bundle = ItemStack::new("minecraft:bundle", 1);
        bundle.data_mut().bundle_contents = counts
            .iter()
            .map(|(item, n)| ItemStack::new(item, *n))
            .collect();
        bundle
    }

    #[test]
    fn a_grid_shows_twelve_stacks_and_counts_the_rest() {
        let Some(items) = items() else { return };
        let twelve: Vec<(&str, u32)> = (0..12).map(|_| ("minecraft:stone", 1)).collect();
        let layout = tooltip_layout(&items, &bundle_with(&twelve), 0.0, 0.0, 96.0, 2).unwrap();
        assert_eq!(layout.slots.len(), 12);
        assert_eq!(layout.overflow, None);
        assert_eq!(layout.height, 3.0 * 24.0 + 13.0 + 8.0);
        // Rows fill bottom-up and columns right-to-left: the twelfth item is in
        // the bottom-right cell, the first in the top-left.
        assert_eq!(layout.slots[0].0, [96.0 - 24.0, 3.0 * 24.0 - 24.0]);
        assert_eq!(&*layout.slots[0].1.item, "minecraft:stone");
        assert_eq!(layout.slots[11].0, [0.0, 0.0]);
        // A wider tooltip centres the 96 pixel column in it.
        let centred = tooltip_layout(&items, &bundle_with(&twelve), 0.0, 0.0, 120.0, 2).unwrap();
        assert_eq!(centred.slots[0].0[0], 12.0 + 96.0 - 24.0);
        // Thirteen stacks: `getNumberOfItemsToShow` leaves the last row room
        // for the count, so 8 slots show and `+6` (four stones and two dirt)
        // takes the first cell.
        let mut counts = twelve;
        counts.push(("minecraft:dirt", 2));
        let layout = tooltip_layout(&items, &bundle_with(&counts), 0.0, 0.0, 96.0, 2).unwrap();
        assert_eq!(layout.slots.len(), 8);
        assert_eq!(layout.overflow, Some(([96.0 - 24.0, 48.0], 6)));
    }

    #[test]
    fn the_bar_fills_with_the_weight_and_an_empty_bundle_explains_itself() {
        let Some(items) = items() else { return };
        let layout = tooltip_layout(&items, &bundle_with(&[]), 4.0, 8.0, 96.0, 2).unwrap();
        assert!(layout.slots.is_empty());
        assert_eq!(layout.fill, 0.0);
        assert_eq!(layout.label, Some("item.minecraft.bundle.empty"));
        assert_eq!(layout.empty_description, Some([4.0, 8.0]));
        assert_eq!(layout.bar, [4.0, 8.0 + 18.0 + 4.0]);
        // A bundle holding one 64 stack is full: the whole bar, the full label.
        let layout = tooltip_layout(
            &items,
            &bundle_with(&[("minecraft:stone", 64)]),
            0.0,
            0.0,
            96.0,
            2,
        )
        .unwrap();
        assert!(layout.full);
        assert_eq!(layout.fill, 94.0);
        assert_eq!(layout.label, Some("item.minecraft.bundle.full"));
        // Something that is not a bundle has no component.
        let dirt = ItemStack::new("minecraft:dirt", 1);
        assert!(tooltip_layout(&items, &dirt, 0.0, 0.0, 96.0, 2).is_none());
    }
    /// `getEmptyBundleBackgroundHeight`: the description's wrapped height (9 a
    /// line) plus the bar's 13 and the tooltip's 8, not a fixed two lines.
    #[test]
    fn an_empty_bundles_height_follows_its_description_lines() {
        let Some(items) = items() else { return };
        let layout = tooltip_layout(&items, &bundle_with(&[]), 4.0, 8.0, 96.0, 3).unwrap();
        assert_eq!(layout.height, 3.0 * 9.0 + 13.0 + 8.0);
        assert_eq!(layout.bar, [4.0, 8.0 + 3.0 * 9.0 + 4.0]);
        assert_eq!(layout.empty_description, Some([4.0, 8.0]));
    }
}
