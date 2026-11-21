//! The `break_block` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0038

pub trait Hooks {
    /// A player's attempt to break the live block `state` at `pos`:
    /// NeoForge's `CommonHooks.fireBlockBreak` at the start of `ServerPlayerGameMode.destroyBlock`.
    /// An item, game-mode or game-master refusal starts the event with `event.cancelled` set;
    /// only listeners with `#[listener(receive_cancelled)]` see it in that state. Clearing
    /// cancellation can override the item's refusal, but not a game-mode or game-master
    /// restriction. If the event stays cancelled, nothing is removed or dropped by this attempt.
    /// A nested player break that already removed the subject supersedes the outer attempt,
    /// even if it left water or a smaller turtle-egg stack behind.
    ///
    /// Set `exp_to_drop` to `Some(amount)` to replace the experience sample later, after item
    /// drops are prepared; `None` keeps sampling. On an attempt that starts uncancelled, a
    /// subscribed block part's `exp_drop` answer is supplied as this event's initial amount.
    /// `block_drops` sees and may change the amount afterwards; cancellation or disabled block
    /// drops suppresses it. Server-side, for every player of the world (the helpers read the
    /// acting one).
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_break_block",
        dispatch = "flat",
        side = "server",
        version = 2,
        contract = "cancel",
        out = "canceled",
        neoforge = "BreakBlockEvent"
    )]
    fn break_block(
        pos: [i32; 3],
        state: BlockState,
        canceled: &mut bool,
        exp_to_drop: &mut Option<u32>,
    );
}
