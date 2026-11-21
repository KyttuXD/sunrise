impl Game<'_> {
    /// `ItemEntity.setItem` on a resident row, keeping its before-image for an open transaction.
    /// This only sets the stack; the caller or the next item tick owns any discard.
    pub(super) fn journal_set_item_entity(&mut self, index: usize, stack: Option<ItemStack>) {
        let item = &mut self.dropped[index];
        if let Some(level) = self.transactions.last_mut() {
            level
                .item_entities
                .entry(item.id)
                .or_insert_with(|| item.stack.clone());
        }
        // ItemStack.EMPTY: share the empty stack's backing rather than allocating an air id on each clear.
        static EMPTY: std::sync::LazyLock<ItemStack> =
            std::sync::LazyLock::new(|| ItemStack::new("minecraft:air", 0));
        item.stack = stack
            .filter(|stack| &*stack.item != "minecraft:air")
            .unwrap_or_else(|| ItemStack::clone(&EMPTY));
    }
}
