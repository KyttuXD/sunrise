//! The `adjust_player_turn` hook at the local player's relative input turn.

pub trait Hooks {
    /// The relative deltas a local input is about to pass to `Entity.turn`, after any
    /// device-specific sensitivity, smoothing and inversion. Mouse, controller and other
    /// relative input share this hook; absolute rotation writes, server corrections and
    /// independent cameras do not.
    ///
    /// Edit `yaw_delta` and `pitch_delta` for this turn only. The units are `Entity.turn`
    /// inputs, not degrees: the game converts them as vanilla does, then applies the player's
    /// pitch and vehicle constraints. Later listeners see the last accepted pair. Values that
    /// cannot be represented as finite `float` inputs to Java's turn arithmetic are refused.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "super::client::camera::validate_turn_delta",
        neoforge = "none (vanilla Entity.turn; CalculatePlayerTurnEvent adjusts mouse options upstream)"
    )]
    fn adjust_player_turn(yaw_delta: &mut f64, pitch_delta: &mut f64);
}
