//! The `client_player_logging_out` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Before client disconnect teardown (`Minecraft.disconnect`, NeoForge's
    /// `ClientPlayerNetworkEvent.LoggingOut`). Fires once per actual client
    /// disconnect before the client level, handles, or session are torn down,
    /// including quitting from the title, starting a connection, or losing a
    /// connection during configuration. Both `player` and `game_mode` are
    /// `None` when there is no client player. Canceling a connection attempt,
    /// a login handshake failure, a dimension change, and entering
    /// reconfiguration do not fire the hook. Client-side.
    #[hook(
        version = 2,
        dispatch = "flat",
        side = "client",
        contract = "listen",
        neoforge = "ClientPlayerNetworkEvent.LoggingOut",
        cancellable = false
    )]
    fn client_player_logging_out(player: Option<PlayerRef>, game_mode: Option<GameMode>);
}
