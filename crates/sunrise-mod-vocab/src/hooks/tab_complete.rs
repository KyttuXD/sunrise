//! The `tab_complete` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0333

pub trait Hooks {
    /// A guest asked to complete `command`; owned API `Suggestion` records may be
    /// added to or filtered from `suggestions`, including custom argument completions.
    /// `range` is the dispatcher's initial replacement span in bytes. Each final entry's
    /// range is relative to `command`; the engine merges those ranges after listeners run.
    /// The game snapshots completions only when a listener subscribes.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_tab_complete",
        dispatch = "flat",
        side = "server",
        contract = "collect",
        out = "suggestions",
        neoforge = "none (NeoForge has no generic tab completion event)",
        version = 2
    )]
    fn tab_complete(
        player: u128,
        name: &str,
        command: &str,
        range: [usize; 2],
        suggestions: &mut Vec<Suggestion>,
    );
}
