//! The `render_tooltip_gather_components` hooks and helpers: declarations the mod API generator reads (see `hooks/mod.rs`).

pub trait Hooks {
    /// `RenderTooltipEvent.GatherComponents`: the elements of `stack`'s
    /// tooltip (`ItemStack.getTooltipLines` as `TooltipElement::Text` lines,
    /// plus any `TooltipElement::Component(TooltipComponent)` supplied by the
    /// built-in bundle tooltip or the item's `ItemPart::tooltip_image`), once
    /// `item_tooltip` has run and before any wrapping, factory preparation, or
    /// measuring. Add, change, reorder or remove elements; each element is then
    /// prepared into a `PreparedTooltipComponent` (custom components via
    /// factories registered in `register_client_tooltip_component_factories`)
    /// before `render_tooltip_pre` runs. An unregistered custom type fails preparation
    /// for this tooltip and is reported with its producer; Pre and Texture do not run.
    /// `max_width` starts at -1 (no additional cap beyond screen fit); a positive value
    /// caps text wrapping in GUI pixels. The item's tooltip font is used for wrapping. Set `cancel` and nothing is drawn, and
    /// neither `render_tooltip_pre` nor `render_tooltip_texture` is asked.
    /// Asked each frame the tooltip shows, never for one the stack hides.
    /// Client-side.
    #[hook(
        receiver = "read",
        dispatch = "api_carrier",
        side = "client",
        version = 2,
        contract = "cancel",
        out = "cancel",
        neoforge = "RenderTooltipEvent.GatherComponents",
        cancellable = true
    )]
    fn render_tooltip_gather_components(
        stack: &ItemStack,
        screen_width: i32,
        screen_height: i32,
        elements: &mut Vec<TooltipElement>,
        max_width: &mut i32,
        cancel: &mut bool,
    );
}
