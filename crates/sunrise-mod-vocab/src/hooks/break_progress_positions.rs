//! Additional positions for a client's breaking-crack display.

pub trait Hooks {
    /// When a crack is set or changes stage, supply additional positions drawn
    /// at that same stage. Only parts on the cracked `state` are called, for
    /// both the local player's predicted crack and other players' cracks.
    /// Call `add_position` for each extra position in the supplied `level`.
    /// The original `pos` is included without a submission. The positions
    /// share the original crack's lifetime and are not requested during rendering.
    ///
    /// The renderer applies the same camera-distance and overlay checks to
    /// every position and draws only the highest stage at a shared position.
    /// Positions already submitted belong to that stage even if the part fails
    /// later in the call. A panic logs and shows an error naming the mod and
    /// disables that mod's `break_progress_positions` callbacks for the session;
    /// other parts still run. A refusal is reported each time without disabling
    /// the callback. This hook changes neither break speed nor what breaks.
    #[hook(
        side = "client",
        contract = "collect",
        out = "add_position",
        event_per_listener = true,
        isolate = true,
        scope = "block",
        key = "state",
        version = 2,
        neoforge = "none (vanilla ClientLevel.destroyBlockProgress)"
    )]
    fn break_progress_positions(
        level: LevelRef,
        pos: BlockPos,
        state: BlockState,
        progress: u8,
        add_position: &mut dyn FnMut(BlockPos),
    );
}
