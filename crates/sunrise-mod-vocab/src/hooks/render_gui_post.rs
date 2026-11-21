//! The `render_gui_post` declarations read by the mod API generator.

pub trait Hooks {
    /// After the in-world HUD layers, including registered mod layers, and before the open screen,
    /// toasts or debug overlay (NeoForge's `RenderGuiEvent.Post`). Does not run if `render_gui_pre`
    /// canceled the HUD pass. F1 hides ordinary HUD paint, not this event; use `host.hud_mode()`
    /// if your overlay should hide as well.
    ///
    /// `event.draw` paints in scaled GUI pixels. `event.layers` lends the paint bounds of the
    /// layers that submitted geometry this pass, in paint order. Hidden, canceled and empty
    /// layers contribute no entry. Paint from a wrapper or replacement belongs to its slot;
    /// paint from `gui_layer_pre` and `gui_layer_post` does not. Use the bounds of a layer such
    /// as `minecraft:effects` for placement, not a shared corner allocator.
    #[notes = r#"## HUD and screen drawing

`Hooks::render_gui_post` paints after HUD layers, before screens and toasts, and does not run
when `Hooks::render_gui_pre` cancels. Use `event.draw.size()` for the scaled GUI dimensions
and `event.layers` to read rectangles such as `minecraft:effects`. For paint over an open
screen, use `Hooks::screen_render_post` instead. Window focus is `ClientHost::window_focused()`.
`Hooks::menu_background` can adjust the requested blur radius or set it to
`None` before the game validates it.

`Hooks::entity_outline` receives the entity id, kind, and mutable optional RGB
outline for the current client render. Leave the existing value unchanged to
preserve vanilla glowing; `None` disables it. These read-only client hooks
receive `&dyn ClientHost`. They do not grant server writes.

`Hooks::client_disconnecting` runs before an actual connection is discarded,
not for a dimension change. Initialization and final shutdown still use the
existing `client_setup` and `client_stopping` hooks, with no parallel aliases."#]
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        cancellable = false,
        version = 2,
        neoforge = "RenderGuiEvent.Post"
    )]
    fn render_gui_post(draw: &mut dyn DrawContext, layers: &[GuiLayerBounds]);
}
