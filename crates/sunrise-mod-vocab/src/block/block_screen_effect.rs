use crate::{BlockPos, BlockState, CameraDrawContext, ClientHost, ExtractBlockScreenEffectEvent};
use std::sync::Arc;

/// Which of `ScreenEffectRenderer`'s overlays a block puts over the first-person view
/// (NeoForge's `ExtractBlockScreenEffectEvent.OverlayType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// exhaustive: a closed set the game matches without a wildcard
#[derive(sunrise_mod_api_macros::Wire)]
pub enum BlockScreenEffect {
    /// The player is on fire: the fire sprite over the lower view.
    Fire,
    /// The camera is inside a view-blocking block: its particle texture.
    Block,
    /// The eye is under water in first person: the underwater texture.
    Water,
}

impl BlockScreenEffect {
    pub(crate) const COUNT: usize = Self::Water as usize + 1;
}

// no wire: the renderer is an in-process callback.
/// A renderer selected for one block overlay by [`ExtractBlockScreenEffectEvent::set_custom_renderer`].
/// Keep it in an [`Arc`] and clone that reference at extraction rather than allocating per frame.
pub trait BlockScreenEffectRenderer: Send + Sync {
    /// Submit this overlay as the selecting mod, with read-only access to the client. Return
    /// `true` to suppress its vanilla quads or `false` to leave them after yours. For block and water,
    /// cancellation at extraction still suppresses vanilla quads. A selected fire renderer
    /// decides whether vanilla fire follows, even if the event is canceled. A failed submission
    /// contributes no quads and leaves extraction's vanilla drawing decision unchanged. A panic
    /// in this callback or during engine-owned release of the renderer disables this mod's
    /// submissions for this overlay kind for the session; its extractor and submissions for other
    /// overlays still run. A phase refusal is reported on every call without disabling anything.
    /// Neither borrow outlives the call.
    fn submit(&self, host: &dyn ClientHost, render: BlockScreenEffectRender<'_>) -> bool;
}

// no wire: the drawing context is a frame-local borrow.
/// The overlay's original extraction inputs and its camera-space drawing scope.
#[non_exhaustive]
pub struct BlockScreenEffectRender<'a> {
    /// The overlay selected at extraction.
    pub effect: BlockScreenEffect,
    /// The block state associated with this overlay.
    pub state: BlockState,
    /// The position associated with this overlay.
    pub pos: BlockPos,
    /// The same world partial tick as at extraction.
    pub partial_tick: f32,
    /// Whether the HUD is hidden.
    pub hide_gui: bool,
    /// Camera-space quads, under the first-person hand's projection and before the GUI.
    pub draw: &'a mut dyn CameraDrawContext,
}

// hidden: only the engine has the original extraction, frame timing, and draw scope.
#[doc(hidden)]
pub fn block_screen_effect_render(
    effect: BlockScreenEffect,
    state: BlockState,
    pos: BlockPos,
    partial_tick: f32,
    hide_gui: bool,
    draw: &mut dyn CameraDrawContext,
) -> BlockScreenEffectRender<'_> {
    BlockScreenEffectRender {
        effect,
        state,
        pos,
        partial_tick,
        hide_gui,
        draw,
    }
}

impl ExtractBlockScreenEffectEvent<'_> {
    /// Whether a renderer is currently selected, including an earlier listener's choice or
    /// the built-in no-op selected when fire's cancellation is set with an empty slot.
    pub fn has_custom_renderer(&self) -> bool {
        self.extraction.has_custom_renderer()
    }

    /// Select or replace the renderer for this overlay. Clone a renderer you already own so
    /// extraction does not allocate. The last eligible listener to set or clear the choice wins.
    /// Cancellation skips subsequent listeners unless they opt into `#[listener(receive_cancelled)]`.
    /// The selected renderer still submits even if the overlay is canceled. In particular,
    /// it replaces the no-op that would otherwise suppress canceled fire. A failed listener
    /// leaves the preceding selection and cancellation in place.
    pub fn set_custom_renderer(&mut self, renderer: Arc<dyn BlockScreenEffectRenderer>) {
        // The event can only borrow the selection slot inside a subscribed listener.
        let owners = crate::runtime::installed().and_then(|installed| {
            let id = crate::content::crash::current_native_mod()?;
            installed.overlay_owners.get(id)
        });
        if let Some(owners) = owners {
            let owned = OwnedOverlayRenderer {
                owner: &owners[self.extraction.effect as usize],
                renderer: Some(renderer),
            };
            self.extraction.selected = Some(SelectedBlockScreenEffectRenderer(Some(owned)));
        } else {
            log::error!("extract_block_screen_effect: renderer selection has no listener owner");
        }
    }

    /// Clear the current choice without changing cancellation. Canceled block and water still
    /// do not draw. For fire, clearing the slot lets vanilla fire draw even if canceled.
    pub fn clear_custom_renderer(&mut self) {
        self.extraction.clear_custom_renderer();
    }

    /// Whether cancellation is set, independently of the current renderer choice.
    pub fn cancelled(&self) -> bool {
        self.extraction.cancelled()
    }

    /// Set cancellation. As in NeoForge, setting either value on fire with no renderer
    /// selects a built-in no-op. Clear the choice afterward if you want vanilla fire to draw.
    /// Returning [`crate::Flow::Cancel`] is equivalent to setting `true` as the listener ends.
    pub fn set_cancelled(&mut self, cancelled: bool) {
        self.extraction.set_cancelled(cancelled);
    }
}

// no wire: the extraction holds an in-process renderer selection.
/// Cancellation and renderer selection shared by the native hook and the listener event.
#[doc(hidden)]
#[derive(Clone)]
// exhaustive: engine-only bookkeeping with private fields.
pub struct BlockScreenEffectExtraction {
    effect: BlockScreenEffect,
    cancelled: bool,
    selected: Option<SelectedBlockScreenEffectRenderer>,
}

impl BlockScreenEffectExtraction {
    /// Start extraction uncanceled and with no renderer selected.
    pub fn new(effect: BlockScreenEffect) -> Self {
        Self {
            effect,
            cancelled: false,
            selected: None,
        }
    }

    /// Whether cancellation is set, independently of the current renderer choice.
    pub fn cancelled(&self) -> bool {
        self.cancelled
    }

    /// Set cancellation. Either value selects a built-in no-op for fire if no renderer is
    /// selected at the time of the write. An existing renderer is left in place.
    pub fn set_cancelled(&mut self, cancelled: bool) {
        self.cancelled = cancelled;
        if self.effect == BlockScreenEffect::Fire && self.selected.is_none() {
            self.selected = Some(SelectedBlockScreenEffectRenderer::NO_OP);
        }
    }

    /// Whether a renderer is selected, including the built-in fire no-op.
    pub fn has_custom_renderer(&self) -> bool {
        self.selected.is_some()
    }

    /// Clear the current choice without changing cancellation; vanilla fire can draw again.
    pub fn clear_custom_renderer(&mut self) {
        self.selected = None;
    }

    /// Move the final choice out for submission.
    pub fn into_renderer(self) -> Option<SelectedBlockScreenEffectRenderer> {
        self.selected
    }
}

// no wire: a selected callback and its attribution belong to this process.
/// Engine-owned selection, carrying the listener's attribution to submission.
// hidden: only the game may submit a selected callback.
#[doc(hidden)]
#[derive(Clone)]
// exhaustive: only the extraction event can construct this private-field selection.
pub struct SelectedBlockScreenEffectRenderer(Option<OwnedOverlayRenderer>);

#[derive(Clone)]
struct OwnedOverlayRenderer {
    owner: &'static crate::callback_owner::CallbackOwner,
    renderer: Option<Arc<dyn BlockScreenEffectRenderer>>,
}

impl Drop for OwnedOverlayRenderer {
    fn drop(&mut self) {
        // Any clone, including a listener's dispatch snapshot, can hold the final reference.
        self.owner.dispose(
            "block screen effect renderer",
            "block_screen_effect_renderer::drop",
            self.renderer.take(),
        );
    }
}

impl SelectedBlockScreenEffectRenderer {
    const NO_OP: Self = Self(None);

    /// Submit under the selecting listener's mod scope, returning no answer on failure.
    pub fn submit(
        &self,
        host: &dyn ClientHost,
        render: BlockScreenEffectRender<'_>,
    ) -> Option<bool> {
        let Some(owned) = &self.0 else {
            // The engine's no-op for fire claims the overlay without submitting any quads.
            return Some(true);
        };
        // A poisoned submission is skipped on later frames, without building a disabled error.
        if !owned.owner.is_enabled() {
            return None;
        }
        let renderer = owned.renderer.as_deref().expect("live overlay renderer");
        owned
            .owner
            .call(
                "block screen effect renderer",
                "block_screen_effect_renderer::submit",
                || renderer.submit(host, render),
            )
            .ok()
    }
}
