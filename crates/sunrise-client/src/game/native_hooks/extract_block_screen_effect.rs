use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// `LevelExtractor.extractPlayerState`: let client listeners choose the overlay renderer
    /// using the overlay's state and position and the world and player interpolation values.
    /// Native replacements set cancellation with `renderer.set_cancelled(value)`, so a write
    /// to an empty fire slot selects the same no-op as it does in a generated listener.
    pub(in crate::game) fn native_extract_block_screen_effect(
        &self,
        effect: mod_api::BlockScreenEffect,
        state: mod_api::BlockState,
        pos: BlockPos,
        partial_tick: f32,
        player_partial_tick: f32,
        renderer: &mut mod_api::__private::BlockScreenEffectRendererSlot,
    ) {
        self.fire_extract_block_screen_effect(
            effect,
            state,
            pos,
            partial_tick,
            player_partial_tick,
            renderer,
        );
    }
}
