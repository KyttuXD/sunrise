use super::*;

impl Game<'_> {
    /// NeoForge `EventHooks.firePlayerSmeltedEvent` at
    /// `FurnaceResultSlot.checkTakeAchievements` on either logical side.
    pub(in crate::game) fn native_item_smelted(
        &mut self,
        call: mod_api::__private::ItemSmeltedCall<'_>,
    ) {
        self.fire_item_smelted(call);
    }

    /// Give both menu callers the same borrowed view of each nonzero take.
    pub(in crate::game) fn publish_item_smelted(
        &mut self,
        player: PlayerRef,
        taken: &[(ItemStack, u32)],
    ) {
        use crate::game::api_values::{project_stack, stack_read_metadata};

        for (stack, amount_removed) in taken {
            let metadata = match stack_read_metadata(stack) {
                Ok(metadata) => metadata,
                Err(error) => {
                    log::warn!(
                        "item_smelted: cannot read result stack {}: {error:?}",
                        stack.item
                    );
                    continue;
                }
            };
            let project = |metadata| match project_stack(stack, metadata) {
                Ok(stack) => Some(stack),
                Err(error) => {
                    log::warn!(
                        "item_smelted: cannot project result stack {}: {error:?}",
                        stack.item
                    );
                    None
                }
            };
            let stack = mod_api::ItemStackView::resident(metadata, &project);
            self.native_item_smelted(mod_api::__private::ItemSmeltedCall {
                player,
                stack: &stack,
                amount_removed: *amount_removed,
            });
        }
    }
}
