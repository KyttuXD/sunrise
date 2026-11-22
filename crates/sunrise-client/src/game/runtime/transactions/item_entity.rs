impl Game<'_> {
    /// `ItemEntity.setItem` on a resident index. Keep the previous stack and its identity
    /// for an open transaction. Only a consumer that takes the last item schedules discard;
    /// otherwise `ItemEntity.tick` handles an empty stack.
    pub(super) fn journal_set_item_entity(&mut self, index: usize, stack: Option<ItemStack>) {
        let item = &mut self.dropped[index];
        if let Some(level) = self.transactions.last_mut() {
            level
                .item_entities
                .entry(item.id)
                .or_insert_with(|| (item.stack.clone(), item.stack_revision));
        }
        item.stack = stack
            .filter(|stack| stack.count > 0 && &*stack.item != "minecraft:air")
            .unwrap_or_else(|| ItemStack::new("minecraft:air", 0));
        item.stack_revision = item.stack_revision.wrapping_add(1);
        // Like dirty DATA_ITEM, this also reaches clients tracking a stationary item.
        item.needs_sync = true;
    }
}
