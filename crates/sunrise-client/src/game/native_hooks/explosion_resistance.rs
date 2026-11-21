use super::*;

impl GameLevel<'_> {
    /// `IBlockExtension.getExplosionResistance`: the parts of `state` at `pos` may answer the
    /// block's resistance against `explosion`. The level is read-only.
    #[inline(always)]
    pub(in crate::game) fn native_explosion_resistance(
        &self,
        pos: BlockPos,
        state: BlockStateId,
        explosion: &ExplosionStart<'_>,
        resistance: &mut Option<f32>,
    ) {
        self.fire_explosion_resistance(pos, state, explosion, resistance);
    }
}

impl Game<'_> {
    /// The first finite resistance an attached part supplies, in place of the static one.
    /// Dispatch handles refusals and isolates panics per listener.
    pub(in crate::game) fn part_explosion_resistance(
        &mut self,
        pos: [i32; 3],
        state: BlockStateId,
        explosion: &ExplosionStart<'_>,
    ) -> Option<f32> {
        if !self.is_logical_server()
            || !crate::game::api_values::block_state::block_listens(
                mod_api::HookId::ExplosionResistance,
                state,
            )
        {
            return None;
        }
        let mut resistance = None;
        let pos = BlockPos::new(pos[0], pos[1], pos[2]);
        self.with_level(|_, level, _| {
            level.native_explosion_resistance(pos, state, explosion, &mut resistance);
        });
        resistance
    }
}
