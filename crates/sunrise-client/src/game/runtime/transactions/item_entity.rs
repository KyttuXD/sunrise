impl Game<'_> {
    /// `ItemEntity.setItem` on a resident row, keeping its before-image for an open transaction.
    /// This only sets the stack; the caller or the next item tick owns any discard.
    pub(super) fn journal_set_item_entity(&mut self, index: usize, stack: Option<ItemStack>) {
        let item = &mut self.dropped[index];
        if let Some(level) = self.transactions.last_mut() {
            level
                .item_entities
                .entry(item.id)
                .or_insert_with(|| (item.stack.clone(), item.needs_sync));
        }
        item.stack = stack
            .filter(|stack| &*stack.item != "minecraft:air")
            .unwrap_or_else(ItemStack::empty);
        // `ItemEntity.setItem` writes synced data; an idle item must not skip tracking it.
        item.needs_sync = true;
    }
}
