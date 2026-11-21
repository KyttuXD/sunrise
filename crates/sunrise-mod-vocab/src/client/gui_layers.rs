//! HUD layers a native mod registers, replaces or wraps (NeoForge's `RegisterGuiLayersEvent`).

use crate::{ClientHost, DrawContext};

// no wire: the callable and its captures are kept in the client process.
/// Draws one registered HUD layer with the current client and drawing context.
/// The client owns the closure for the session and calls it as the mod that installed it.
/// Capture per-layer data; the host and canvas are borrowed only for this draw.
/// A disabled owner is skipped. A refused or panicking draw discards its paint;
/// only a panic disables this callback for the session.
pub type GuiLayerDraw = dyn Fn(&dyn ClientHost, &mut dyn DrawContext) + Send + Sync + 'static;

// no wire: the wrapper and its captures are kept in the client process.
/// Draws around the previous body of a layer, or omits that body for this draw.
/// The client owns this callback for its lifetime. A disabled wrapper (including one
/// disabled by panic) leaves the previous layer body to draw directly.
/// On refusal or panic, the wrapper's own paint is discarded; paint it already delegated
/// to the previous body stays in order without drawing that body again.
pub type GuiLayerWrapper = dyn Fn(&mut dyn GuiLayerWrapContext) + Send + Sync + 'static;

// no wire: lends the current client and the HUD batch only while a wrapper runs.
/// The per-draw context for a registered HUD layer wrapper.
pub trait GuiLayerWrapContext {
    /// The current client host.
    fn host(&self) -> &dyn ClientHost;

    /// Paint into the current HUD batch with a draw context scoped to this call.
    fn with_draw(&mut self, draw: &mut dyn FnMut(&mut dyn DrawContext));

    /// Paint the previous body at this point in the same batch without firing layer hooks again.
    /// The previous mod body runs as its own mod; a previous vanilla body runs as the engine.
    /// A vanilla paint panic still escapes the layer as an engine fault, even if the wrapper
    /// catches its unwind. Omit this call to skip the previous body for this draw.
    fn draw_previous(&mut self);
}

// no wire: the registrar borrows the live HUD registration for one hook call.
/// The HUD layer registrar handed to `register_gui_layers` once at client startup.
/// Registrations are applied in call order; the registrar is only available during that hook.
pub trait GuiLayerRegistrar {
    /// Install `draw` at `placement` under a namespaced ID such as `mymod:compass`.
    /// Returns false and logs a warning if the ID is malformed, in the `minecraft` namespace,
    /// already registered, or relative to a layer that is not registered yet. A rejected
    /// registration does not reserve the ID; use `replace_layer` or `wrap_layer` to edit one.
    fn register(
        &mut self,
        id: String,
        placement: GuiLayerPlacement,
        draw: Box<GuiLayerDraw>,
    ) -> bool;

    /// Replace an already registered layer (including a vanilla layer) with `draw`.
    /// Discards the entire previous body, including any wrappers, but keeps the layer's ID,
    /// position and draw conditions.
    /// Returns false and logs a warning if `id` is not registered.
    fn replace_layer(&mut self, id: &str, draw: Box<GuiLayerDraw>) -> bool;

    /// Wrap an already registered layer in place, keeping its ID, position and draw conditions.
    /// The newest wrapper runs first and can draw the previous body through its context.
    /// Returns false and logs a warning if `id` is not registered.
    fn wrap_layer(&mut self, id: &str, wrap: Box<GuiLayerWrapper>) -> bool;
}

// no wire: the ID and geometry describe one client HUD extraction, not a payload.
/// The bounds of one HUD layer's submitted geometry for the current frame.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct GuiLayerBounds {
    /// The same namespaced ID used when registering or observing the layer.
    pub layer: &'static str,
    /// Bounding rectangle `[x, y, width, height]` in scaled GUI pixels, within the window.
    /// This describes geometry, not per-pixel visibility through other layers or textures.
    pub rect: [f32; 4],
}

impl GuiLayerBounds {
    // hidden: only extraction supplies the current frame's geometry bounds.
    #[doc(hidden)]
    pub const fn new(layer: &'static str, rect: [f32; 4]) -> Self {
        Self { layer, rect }
    }
}

/// Where a mod's HUD layer goes, the four orderings of `RegisterGuiLayersEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
// exhaustive: NeoForge's four orderings, matched without a wildcard
#[derive(sunrise_mod_api_macros::Wire)]
pub enum GuiLayerPlacement {
    /// Below every other layer, the crosshair included (`registerBelowAll`).
    BelowAll,
    /// Right below the layer with this ID (`registerBelow`): a vanilla layer, like
    /// `minecraft:hotbar`, or one registered before this one.
    Below(String),
    /// Right above the layer with this ID (`registerAbove`), with the same rule.
    Above(String),
    /// Above every other layer, the tab list included (`registerAboveAll`).
    AboveAll,
}
