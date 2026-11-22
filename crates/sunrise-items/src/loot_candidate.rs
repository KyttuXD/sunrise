//! Live candidate backing the items loot evaluator without whole-stack cloning or
//! detached `LootStack` snapshots.
use crate::items::{DYNAMIC, ItemStack, Items, patch_loot_component, prototypes_of};
use serde_json::Value;
use std::sync::{Arc, OnceLock};
use sunrise_core::nbt::Tag;
use sunrise_world::loot::{
    ApiExplorationMap, ExplorationMap, LootExplorationView, LootListAccess, LootStackAccess,
    LootStackEditAccess, LootToolAccess,
};

#[derive(Clone, Copy, Debug, Default)]
struct Dirty {
    count: bool,
    damage: bool,
    enchantments: bool,
    potion: bool,
    ominous_amplifier: bool,
    stew_effects: bool,
    names: bool,
    components: bool,
}

impl Dirty {
    fn any(self) -> bool {
        self.count
            || self.damage
            || self.enchantments
            || self.potion
            || self.ominous_amplifier
            || self.stew_effects
            || self.names
            || self.components
    }
}

/// One live candidate in the items loot evaluator.
///
/// The resident [`ItemStack`] stays in place: untouched modeled fields (such as a
/// written book's contents, bundle contents, or banner layers) are never rebuilt
/// or stripped when a custom function or global loot modifier leaves them alone.
/// Intermediate signed count/damage, map/exploration flags, and ordered component
/// edits stay live across consecutive custom functions and global loot modifiers.
#[derive(Clone, Debug)]
pub(crate) struct LootCandidate {
    stack: ItemStack,
    count: i32,
    damage: i32,
    map: bool,
    exploration: Option<Box<ExplorationMap>>,
    ominous_amplifier: Option<i32>,
    potion: Option<String>,
    enchantments: OnceLock<Vec<(String, i32)>>,
    stew_effects: OnceLock<Vec<(String, i32)>>,
    components: OnceLock<Vec<(String, Option<Tag>)>>,
    baseline_components: Option<Vec<(String, Option<Tag>)>>,
    names: Vec<(String, Value)>,
    dirty: Dirty,
}

impl LootCandidate {
    pub(crate) fn from_stack(stack: ItemStack) -> Self {
        let count = stack.count as i32;
        let damage = stack.damage as i32;
        let ominous_amplifier = stack
            .data()
            .and_then(|d| d.ominous_amplifier)
            .map(i32::from);
        let potion = stack.potion().map(str::to_owned);
        Self {
            stack,
            count,
            damage,
            map: false,
            exploration: None,
            ominous_amplifier,
            potion,
            enchantments: OnceLock::new(),
            stew_effects: OnceLock::new(),
            components: OnceLock::new(),
            baseline_components: None,
            names: Vec::new(),
            dirty: Dirty::default(),
        }
    }

    pub(crate) fn from_item(item: impl Into<Arc<str>>, count: i32) -> Self {
        let mut candidate = Self::from_stack(ItemStack {
            item: item.into(),
            count: count.max(0) as u32,
            damage: 0,
            extra: None,
        });
        candidate.count = count;
        candidate
    }

    pub(crate) fn from_loot_stack(value: &sunrise_world::loot::LootStack) -> ItemStack {
        let mut candidate = Self::from_item(value.item.as_str(), value.count);
        candidate.dirty.count = true;
        candidate.set_damage(value.damage);
        if !value.enchantments.is_empty() {
            candidate.set_enchantments(value.enchantments.clone());
        }
        if value.potion.is_some() {
            candidate.set_potion_id(value.potion.clone());
        }
        if value.ominous_amplifier.is_some() {
            candidate.set_ominous_amplifier(value.ominous_amplifier);
        }
        if !value.stew_effects.is_empty() {
            candidate.set_stew_effects(value.stew_effects.clone());
        }
        if !value.names.is_empty() {
            candidate.set_names_values(value.names.clone());
        }
        if !value.components.is_empty() {
            candidate.set_components(value.components.clone());
        }
        candidate.into_item_stack_with(&|_, _| None)
    }

    pub(crate) fn item(&self) -> &str {
        &self.stack.item
    }

    pub(crate) fn is_dynamic_marker(&self) -> bool {
        self.stack.item.starts_with(DYNAMIC)
    }

    pub(crate) fn count(&self) -> i32 {
        self.count
    }

    pub(crate) fn set_signed_count(&mut self, count: i32) {
        self.count = count;
        self.dirty.count = true;
    }

    /// Run a vanilla `ItemStack` edit on the resident stack after flushing any
    /// pending mod edits, preserving signed count/damage when vanilla leaves them
    /// unchanged and keeping deferred map/exploration metadata intact.
    pub(crate) fn with_resident_mut<T>(
        &mut self,
        items: &Items,
        edit: impl FnOnce(&mut ItemStack) -> T,
    ) -> T {
        self.flush_to_resident(&prototypes_of(items));
        let flushed_count = self.stack.count;
        let flushed_damage = self.stack.damage;
        let prev_count = self.count;
        let prev_damage = self.damage;
        let result = edit(&mut self.stack);
        if self.stack.count != flushed_count {
            self.count = self.stack.count as i32;
        } else {
            self.count = prev_count;
        }
        if self.stack.damage != flushed_damage {
            self.damage = self.stack.damage as i32;
        } else {
            self.damage = prev_damage;
        }
        self.potion = self.stack.potion().map(str::to_owned);
        self.ominous_amplifier = self
            .stack
            .data()
            .and_then(|d| d.ominous_amplifier)
            .map(i32::from);
        self.enchantments.take();
        self.stew_effects.take();
        self.components.take();
        self.baseline_components = None;
        result
    }

    pub(crate) fn max_stack(&mut self, items: &Items) -> i32 {
        if self.dirty.components {
            self.flush_to_resident(&prototypes_of(items));
        }
        crate::bundle::max_stack(items, &self.stack) as i32
    }

    pub(crate) fn split_into(mut self, items: &Items, out: &mut Vec<Self>) {
        if self.count <= 1 || self.is_dynamic_marker() {
            out.push(self);
            return;
        }
        let max = self.max_stack(items).max(1);
        if self.count <= max {
            out.push(self);
            return;
        }
        let mut remaining = self.count;
        while remaining > max {
            let mut part = self.clone();
            part.count = max;
            part.stack.count = max as u32;
            part.dirty.count = false;
            out.push(part);
            remaining -= max;
        }
        self.count = remaining;
        self.stack.count = remaining as u32;
        self.dirty.count = false;
        out.push(self);
    }

    pub(crate) fn push_into_list(self, target: &mut dyn LootListAccess) {
        let slot = target.push_empty(&self.stack.item);
        slot.set_count(self.count);
        slot.set_damage(self.damage);
        slot.set_enchantments(self.enchantments_slice().to_vec());
        slot.set_potion_id(self.potion.clone());
        slot.set_map(self.map);
        slot.set_exploration(self.exploration.as_deref().map(|map| ApiExplorationMap {
            destination: map.destination.clone(),
            decoration: map.decoration.clone(),
            zoom: map.zoom,
            search_radius: map.search_radius,
            skip_existing_chunks: map.skip_existing_chunks,
        }));
        slot.set_ominous_amplifier(self.ominous_amplifier);
        slot.set_stew_effects(self.stew_effects_slice().to_vec());
        slot.set_names_values(self.names.clone());
        slot.set_components(self.components_slice().to_vec());
    }

    pub(crate) fn into_item_stack(self, items: &Items) -> ItemStack {
        self.into_item_stack_with(&prototypes_of(items))
    }

    pub(crate) fn into_item_stack_with(
        mut self,
        prototype_has: &impl Fn(&str, &str) -> Option<bool>,
    ) -> ItemStack {
        self.flush_to_resident(prototype_has);
        self.stack
    }

    fn flush_to_resident(&mut self, prototype_has: &impl Fn(&str, &str) -> Option<bool>) {
        if !self.dirty.any() {
            return;
        }
        if self.dirty.count {
            self.stack.count = self.count.max(0) as u32;
        }
        if self.dirty.damage {
            self.stack.damage = self.damage.max(0) as u32;
        }
        if self.dirty.enchantments
            && let Some(enchantments) = self.enchantments.get()
        {
            let levels = enchantments
                .iter()
                .filter(|(_, level)| *level > 0)
                .map(|(id, level)| (Arc::from(id.as_str()), *level as u32))
                .collect();
            self.stack.set_crafting_enchantments(levels);
        }
        if self.dirty.potion {
            self.stack.data_mut().potion = self.potion.as_deref().map(Arc::from);
        }
        if self.dirty.ominous_amplifier {
            self.stack.data_mut().ominous_amplifier =
                self.ominous_amplifier.map(|a| a.clamp(0, 255) as u8);
        }
        if self.dirty.stew_effects
            && let Some(stew) = self.stew_effects.get()
        {
            self.stack.data_mut().stew_effects = stew
                .iter()
                .map(|(effect, ticks)| (Arc::from(effect.as_str()), *ticks))
                .collect();
        }
        if self.dirty.names {
            for (id, text) in &self.names {
                self.stack.data_mut().set_loot_name(id, text);
            }
        }
        if self.dirty.components
            && let Some(after) = self.components.get()
        {
            let before = self.baseline_components.as_deref().unwrap_or(&[]);
            for (id, value) in after {
                let unchanged = before
                    .iter()
                    .filter(|(key, _)| key == id)
                    .map(|(_, value)| value)
                    .eq(after
                        .iter()
                        .filter(|(key, _)| key == id)
                        .map(|(_, value)| value));
                if !unchanged {
                    patch_loot_component(&mut self.stack, id, value, prototype_has);
                }
            }
            for (id, _) in before {
                if !after.iter().any(|(i, _)| i == id) {
                    patch_loot_component(&mut self.stack, id, &None, prototype_has);
                }
            }
        }
        self.stack.tidy();
        self.dirty = Dirty::default();
    }

    fn enchantments_slice(&self) -> &[(String, i32)] {
        self.enchantments.get_or_init(|| {
            self.stack
                .crafting_enchantments()
                .iter()
                .map(|(id, level)| (id.to_string(), *level as i32))
                .collect()
        })
    }

    fn enchantments_mut(&mut self) -> &mut Vec<(String, i32)> {
        let _ = self.enchantments_slice();
        self.dirty.enchantments = true;
        self.enchantments
            .get_mut()
            .expect("enchantments initialized")
    }

    fn stew_effects_slice(&self) -> &[(String, i32)] {
        self.stew_effects.get_or_init(|| {
            self.stack
                .data()
                .map(|d| {
                    d.stew_effects
                        .iter()
                        .map(|(effect, ticks)| (effect.to_string(), *ticks))
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    fn stew_effects_mut(&mut self) -> &mut Vec<(String, i32)> {
        let _ = self.stew_effects_slice();
        self.dirty.stew_effects = true;
        self.stew_effects
            .get_mut()
            .expect("stew effects initialized")
    }

    fn components_slice(&self) -> &[(String, Option<Tag>)] {
        self.components
            .get_or_init(|| capture_components(&self.stack))
    }

    fn components_mut(&mut self) -> &mut Vec<(String, Option<Tag>)> {
        let _ = self.components_slice();
        if self.baseline_components.is_none() {
            self.baseline_components = self.components.get().cloned();
        }
        self.dirty.components = true;
        self.components.get_mut().expect("components initialized")
    }
}

fn capture_components(stack: &ItemStack) -> Vec<(String, Option<Tag>)> {
    let skip_dedicated_positive = |id: &str| {
        matches!(
            id,
            "minecraft:enchantments"
                | "minecraft:stored_enchantments"
                | "minecraft:ominous_bottle_amplifier"
                | "minecraft:suspicious_stew_effects"
        ) || (id == "minecraft:potion_contents"
            && stack.data().is_none_or(|data| data.potion_extra.is_empty()))
    };
    let mut components = Vec::new();
    if let Some(Tag::Compound(entries)) = crate::item_data::encode_extra_components(stack) {
        for (key, value) in entries {
            let id = key.to_string_lossy();
            if !id.starts_with('!') && skip_dedicated_positive(&id) {
                continue;
            }
            components.push((id, Some(value)));
        }
    }
    components
}

impl LootToolAccess for LootCandidate {
    fn item_id(&self) -> &str {
        &self.stack.item
    }

    fn enchantment_at(&self, index: usize) -> Option<(&str, i32)> {
        self.enchantments_slice()
            .get(index)
            .map(|(id, level)| (id.as_str(), *level))
    }
}

impl LootStackAccess for LootCandidate {
    fn count(&self) -> i32 {
        self.count
    }

    fn damage(&self) -> i32 {
        self.damage
    }

    fn map(&self) -> bool {
        self.map
    }

    fn components(&self) -> &[(String, Option<Tag>)] {
        self.components_slice()
    }

    fn enchantments(&self) -> &[(String, i32)] {
        self.enchantments_slice()
    }

    fn potion(&self) -> Option<&str> {
        self.potion.as_deref()
    }

    fn stew_effects(&self) -> &[(String, i32)] {
        self.stew_effects_slice()
    }

    fn ominous_amplifier(&self) -> Option<i32> {
        self.ominous_amplifier
    }

    fn exploration(&self) -> Option<LootExplorationView<'_>> {
        self.exploration.as_deref().map(|map| {
            LootExplorationView::new(
                &map.destination,
                &map.decoration,
                map.zoom,
                map.search_radius,
                map.skip_existing_chunks,
            )
        })
    }

    fn names(&self) -> &[(String, Value)] {
        &self.names
    }
}

impl LootStackEditAccess for LootCandidate {
    fn as_view(&self) -> &dyn LootStackAccess {
        self
    }

    fn set_item_id(&mut self, id: &str) {
        self.stack.item = Arc::from(id);
    }

    fn set_count(&mut self, count: i32) {
        self.count = count;
        self.dirty.count = true;
    }

    fn set_damage(&mut self, damage: i32) {
        self.damage = damage;
        self.dirty.damage = true;
    }

    fn set_map(&mut self, map: bool) {
        self.map = map;
    }

    fn set_ominous_amplifier(&mut self, amplifier: Option<i32>) {
        self.ominous_amplifier = amplifier;
        self.dirty.ominous_amplifier = true;
    }

    fn set_potion_id(&mut self, potion: Option<String>) {
        self.potion = potion;
        self.dirty.potion = true;
    }

    fn set_exploration(&mut self, exploration: Option<ApiExplorationMap>) {
        self.exploration = exploration.map(|map| {
            Box::new(ExplorationMap {
                destination: map.destination,
                decoration: map.decoration,
                zoom: map.zoom,
                search_radius: map.search_radius,
                skip_existing_chunks: map.skip_existing_chunks,
            })
        });
    }

    fn push_enchantment(&mut self, id: String, level: i32) {
        self.enchantments_mut().push((id, level));
    }

    fn set_enchantment(&mut self, id: String, level: i32) {
        let enchantments = self.enchantments_mut();
        match enchantments.iter_mut().find(|(name, _)| *name == id) {
            Some(entry) => entry.1 = level,
            None => enchantments.push((id, level)),
        }
    }

    fn set_enchantments(&mut self, enchantments: Vec<(String, i32)>) {
        *self.enchantments_mut() = enchantments;
    }

    fn clear_enchantments(&mut self) {
        self.enchantments_mut().clear();
    }

    fn push_stew_effect(&mut self, id: String, duration_ticks: i32) {
        self.stew_effects_mut().push((id, duration_ticks));
    }

    fn set_stew_effects(&mut self, effects: Vec<(String, i32)>) {
        *self.stew_effects_mut() = effects;
    }

    fn clear_stew_effects(&mut self) {
        self.stew_effects_mut().clear();
    }

    fn push_name_value(&mut self, id: String, value: Value) {
        self.names.push((id, value));
        self.dirty.names = true;
    }

    fn set_name_value(&mut self, id: String, value: Value) {
        self.names.retain(|(existing, _)| *existing != id);
        self.names.push((id, value));
        self.dirty.names = true;
    }

    fn set_names_values(&mut self, names: Vec<(String, Value)>) {
        self.names = names;
        self.dirty.names = true;
    }

    fn clear_names(&mut self) {
        self.names.clear();
        self.dirty.names = true;
    }

    fn push_component(&mut self, id: String, value: Option<Tag>) {
        self.components_mut().push((id, value));
    }

    fn set_components(&mut self, components: Vec<(String, Option<Tag>)>) {
        *self.components_mut() = components;
    }

    fn clear_components(&mut self) {
        self.components_mut().clear();
    }

    fn retain_components(&mut self, keep: &mut dyn FnMut(&str, Option<&Tag>) -> bool) {
        self.components_mut()
            .retain(|(id, value)| keep(id.as_str(), value.as_ref()));
    }
}

impl LootListAccess for Vec<LootCandidate> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn get(&self, index: usize) -> Option<&dyn LootStackAccess> {
        self.as_slice()
            .get(index)
            .map(|stack| stack as &dyn LootStackAccess)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut dyn LootStackEditAccess> {
        self.as_mut_slice()
            .get_mut(index)
            .map(|stack| stack as &mut dyn LootStackEditAccess)
    }

    fn push_empty(&mut self, item_id: &str) -> &mut dyn LootStackEditAccess {
        self.push(LootCandidate::from_item(item_id, 1));
        self.last_mut().expect("just pushed")
    }

    fn insert_empty(
        &mut self,
        index: usize,
        item_id: &str,
    ) -> Option<&mut dyn LootStackEditAccess> {
        if index > self.len() {
            return None;
        }
        self.insert(index, LootCandidate::from_item(item_id, 1));
        Some(&mut self[index])
    }

    fn remove(&mut self, index: usize) -> bool {
        if index >= self.len() {
            return false;
        }
        Vec::remove(self, index);
        true
    }

    fn clear(&mut self) {
        Vec::clear(self);
    }

    fn truncate(&mut self, len: usize) {
        Vec::truncate(self, len);
    }

    fn swap(&mut self, a: usize, b: usize) -> bool {
        if a >= self.len() || b >= self.len() {
            return false;
        }
        self.as_mut_slice().swap(a, b);
        true
    }

    fn reverse(&mut self) {
        self.as_mut_slice().reverse();
    }

    fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.len() || to >= self.len() {
            return false;
        }
        match from.cmp(&to) {
            std::cmp::Ordering::Less => self[from..=to].rotate_left(1),
            std::cmp::Ordering::Greater => self[to..=from].rotate_right(1),
            std::cmp::Ordering::Equal => {}
        }
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
