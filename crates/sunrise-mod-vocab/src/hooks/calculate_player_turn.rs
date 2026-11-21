//! The `calculate_player_turn` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// The mouse sensitivity and cinematic camera option a grabbed player's
    /// mouse turn reads this frame (`MouseHandler.turnPlayer`). Change either
    /// for this turn only; the options are unchanged. Client-side. This shapes how far the
    /// mouse turns the view; `ClientHost::set_local_rotation` sets the view directly, and the
    /// two meet in the same angles the tick's move packet sends. Use `adjust_player_turn` for
    /// relative input after the device-specific calculations. An independent free camera does not
    /// fire this hook. A sensitivity that cannot produce a finite turn scale is refused.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true,
        output_validator = "super::client::camera::validate_mouse_turn",
        neoforge = "CalculatePlayerTurnEvent",
        cancellable = false,
        version = 2
    )]
    fn calculate_player_turn(
        player_entity_id: i32,
        player_uuid: u128,
        mouse_sensitivity: &mut f64,
        cinematic_camera_enabled: &mut bool,
    );
}
