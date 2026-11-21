//! The `gui_layer_post` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0145

pub trait Hooks {
    /// After the complete body of a vanilla or mod HUD layer (`RenderGuiLayerEvent.Post`).
    /// Fires once with the ID from `gui_layer_pre`, even if a wrapper omitted its previous
    /// body. Does not fire when `gui_layer_pre` cancels the layer. Use `event.draw` to
    /// paint above that layer. For paint after the HUD as a whole, use `render_gui_post`;
    /// for paint over focused chat or any open screen, use `screen_render_post`.
    #[hook(
        target = "sunrise_client::game::native_hooks::base::Game<'_>::native_gui_layer_post",
        receiver = "read",
        dispatch = "flat",
        side = "client",
        version = 3,
        contract = "listen",
        neoforge = "RenderGuiLayerEvent.Post",
        cancellable = false
    )]
    fn gui_layer_post(layer: &str, draw: &mut dyn DrawContext);
}
