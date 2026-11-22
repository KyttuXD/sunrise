//! The `screen_init_post` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// A screen finished opening or changed rectangle (`ScreenEvent.Init.Post`).
    /// A new chat instance initializes once even when it replaces one with the same
    /// ID and rectangle. A vetoed opening does not initialize a screen.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        neoforge = "ScreenEvent.Init.Post",
        event_per_listener = true,
        version = 2,
        cancellable = false
    )]
    fn screen_init_post(screen: &ScreenView<'_>);
}
