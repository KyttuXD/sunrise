use super::*;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// Before client disconnect teardown, with the live player and current mode when present.
    pub(in crate::game) fn native_client_player_logging_out(
        &mut self,
        player: Option<mod_api::PlayerRef>,
        game_mode: Option<mod_api::GameMode>,
    ) {
        self.fire_client_player_logging_out(player, game_mode);
    }
}
