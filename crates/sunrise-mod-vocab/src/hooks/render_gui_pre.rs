//! The `render_gui_pre` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Before the in-world HUD layer sequence (NeoForge's `RenderGuiEvent.Pre`), even if F1
    /// hides ordinary HUD paint. Return `Flow::Cancel` to skip HUD layers, their Pre/Post hooks,
    /// and `render_gui_post`. The screen and later toast pass are outside the veto.
    /// Does not fire on frames that do not extract the in-world HUD, such as the title or
    /// the connection screen. Client-side.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "cancel",
        out = "canceled",
        neoforge = "RenderGuiEvent.Pre",
        cancellable = true,
        version = 3
    )]
    fn render_gui_pre(canceled: &mut bool);
}
