//! The `client_player_logging_out` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// NeoForge's `ClientPlayerNetworkEvent.LoggingOut`. Runs on the client
    /// before it tears down its player, game mode and level. A connection
    /// leaving play or disconnecting before login is reported only once.
    /// Changing dimensions or reconfiguring a connection does not log out.
    ///
    /// `player` is the live local player, and `game_mode` is the active
    /// multiplayer controller's mode. Either may be absent; in particular,
    /// a disconnect before login or the reset before starting a new world
    /// can have neither. Use the player handle during the callback; it
    /// stops resolving when the level is torn down.
    #[hook(
        dispatch = "flat",
        side = "client",
        contract = "listen",
        event_per_listener = true,
        neoforge = "ClientPlayerNetworkEvent.LoggingOut",
        cancellable = false,
        version = 2
    )]
    fn client_player_logging_out(player: Option<PlayerRef>, game_mode: Option<GameMode>);
}
