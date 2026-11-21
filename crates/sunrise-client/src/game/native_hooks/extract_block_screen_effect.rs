use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// `LevelExtractor.extractPlayerState`: let client listeners choose the overlay renderer
    /// using the overlay's state and position and the world and player interpolation values.
    /// Set cancellation on `extraction`; for fire this selects a no-op if no renderer is
    /// selected. Clearing the renderer afterward restores vanilla fire without uncanceling.
    pub(in crate::game) fn native_extract_block_screen_effect(
        &self,
        effect: mod_api::BlockScreenEffect,
        state: mod_api::BlockState,
        pos: BlockPos,
        partial_tick: f32,
        player_partial_tick: f32,
        extraction: &mut mod_api::__private::BlockScreenEffectExtraction,
    ) {
        self.fire_extract_block_screen_effect(
            effect,
            state,
            pos,
            partial_tick,
            player_partial_tick,
            extraction,
        );
    }
}
