//! The item evaluator's candidate, kept through entry, pool, table and the modifier pass.
use super::*;
use std::cell::OnceCell;
use sunrise_world::loot::{
    ExplorationMap, LootStack,
    global_modifiers::{LootCandidateAccess, LootStackAccess, LootStackEditAccess},
};

#[derive(Clone)]
pub(super) struct Candidate {
    resident: ItemStack,
    count: i32,
    map: bool,
    exploration: Option<Box<ExplorationMap>>,
    // When a native rule opens the candidate, its signed values and ordered
    // patch stay authoritative across rules. Only stock item operations or
    // the inventory consumer need the resident form again.
    edits: OnceCell<LootStack>,
    edited: bool,
}

impl From<ItemStack> for Candidate {
    fn from(resident: ItemStack) -> Self {
        Self {
            count: resident.count as i32,
            resident,
            map: false,
            exploration: None,
            edits: OnceCell::new(),
            edited: false,
        }
    }
}

impl Candidate {
    pub(super) fn raw(&self) -> &LootStack {
        self.edits.get_or_init(|| {
            let mut raw = loot_of(&self.resident);
            raw.count = self.count;
            raw.map = self.map;
            raw.exploration = self.exploration.clone();
            raw
        })
    }

    pub(super) fn raw_mut(&mut self) -> &mut LootStack {
        self.raw();
        self.edited = true;
        self.edits.get_mut().expect("opened candidate")
    }

    pub(super) fn count(&self) -> i32 {
        self.edits.get().map_or(self.count, |raw| raw.count)
    }

    pub(super) fn item_id(&self) -> &str {
        self.edits
            .get()
            .map_or(self.resident.item.as_ref(), |raw| raw.item.as_str())
    }

    pub(super) fn set_count(&mut self, count: i32) {
        if let Some(raw) = self.edits.get_mut() {
            raw.count = count;
        }
        self.count = count;
        self.resident.count = count.max(0) as u32;
    }

    pub(super) fn stock(&mut self, items: &Items) -> (&mut ItemStack, &mut i32) {
        if let Some(raw) = self.edits.take().filter(|_| self.edited) {
            let mut before = loot_of(&self.resident);
            before.count = self.count;
            apply_loot(&before, &raw, &mut self.resident, prototypes_of(items));
            self.count = raw.count;
            self.map = raw.map;
            self.exploration = raw.exploration;
        }
        self.edited = false;
        self.resident.count = self.count.max(0) as u32;
        (&mut self.resident, &mut self.count)
    }

    pub(super) fn into_stack(mut self, items: &Items) -> ItemStack {
        self.stock(items);
        if let Some(search) = self.exploration {
            self.resident.defer_exploration_map(&search);
        }
        self.resident
    }
}

impl LootCandidateAccess for Candidate {
    fn blank(id: &str) -> Self {
        ItemStack::new(id, 1).into()
    }

    fn lend(&self) -> &dyn LootStackAccess {
        self.raw()
    }

    fn lend_mut(&mut self) -> &mut dyn LootStackEditAccess {
        self.raw_mut()
    }
}
