//! One stored display answer per crack stage, never per frame.
use super::*;
use crate::game::api_values::block_state::block_listens;

impl Game<'_> {
    pub(in crate::game) fn with_break_progress_positions(
        &mut self,
        pos: [i32; 3],
        progress: u8,
        publish: impl FnOnce(&mut Self, Vec<[i32; 3]>),
    ) {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        if !self.native_window() || !block_listens(mod_api::HookId::BreakProgressPositions, state) {
            publish(self, Vec::new());
            return;
        }
        let Some(level) = self.native_client_level() else {
            publish(self, Vec::new());
            return;
        };
        let seal = self.transaction_open();
        let mut extras = Vec::new();
        self.native_break_progress_positions(super::fire::BreakProgressPositionsCall {
            level,
            pos: mod_api::BlockPos::new(pos[0], pos[1], pos[2]),
            state,
            progress,
            add_position: &mut |candidate| extras.push([candidate.x, candidate.y, candidate.z]),
        });
        if self.native_client_level() == Some(level) {
            publish(self, extras);
        } else {
            log::warn!("break_progress_positions discarded extras after the client level changed");
        }
        self.transaction_close(&seal, true);
    }

    pub(in crate::game) fn native_break_progress_positions(
        &mut self,
        call: super::fire::BreakProgressPositionsCall<'_>,
    ) {
        self.fire_break_progress_positions(call);
    }
}
