use super::Game;

/// The hook points. Each does nothing in Sunrise.
impl Game<'_> {
    /// `RenderTooltipEvent.GatherComponents`: the elements of `stack`'s
    /// tooltip (`ItemStack.getTooltipLines` as text lines, plus any component
    /// supplied by the built-in bundle tooltip or the item's `tooltip_image`
    /// part), once `item_tooltip` has run and before any wrapping, factory
    /// preparation or measuring. Add, change, reorder or remove elements; each
    /// element is then prepared into a `PreparedTooltipComponent` before
    /// `render_tooltip_pre` runs. `max_width` starts at -1, no limit, which
    /// leaves the lines as they are; a mod that sets it wraps every line wider
    /// than that, in GUI pixels. Set `cancel` and nothing is drawn, and neither
    /// `render_tooltip_pre` nor `render_tooltip_texture` is asked. Asked each
    /// frame the tooltip shows, never for one the stack hides. Client-side.
    pub(in crate::game) fn native_render_tooltip_gather_components(
        &self,
        call: mod_api::__private::RenderTooltipGatherComponentsCall<'_>,
    ) {
        self.fire_render_tooltip_gather_components(call);
    }
}
