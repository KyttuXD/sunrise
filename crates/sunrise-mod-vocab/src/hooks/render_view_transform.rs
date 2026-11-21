//! The `render_view_transform` hook: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// Composes a presentation-only transform at `GameRenderer.bobHurt` / `bobView`, after
    /// hurt tilt and view bobbing, before the level's portal or Nausea projection effect.
    ///
    /// `transform` starts as [`ViewTransform::IDENTITY`] each frame. Its transforms compose in
    /// listener order and affect the world and first-person hand passes, not the camera or
    /// sound listener, player position, or aim. An effect may read
    /// [`ClientGuiOptions::screen_effect_scale`] for the user's Distortion Effects preference.
    #[hook(
        receiver = "read",
        dispatch = "flat",
        side = "client",
        contract = "listen",
        isolate = true
    )]
    fn render_view_transform(partial: f32, transform: &mut mod_api::ViewTransform);
}
