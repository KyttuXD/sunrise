use super::*;
use crate::game::host::draw::{EngineTooltipFont, GameDrawContext, font_id_str};
use mod_api::{Identifier, PreparedTooltipComponent, TooltipComponent, TooltipElement};
use std::{borrow::Cow, sync::LazyLock};
use sunrise_client::{
    bundle, text::Text as EngineText, tooltip_components::MissingTooltipComponentFactory,
};

pub(super) static DEFAULT_FONT_ID: LazyLock<mod_api::Identifier> = LazyLock::new(|| {
    mod_api::Identifier::from_parts("minecraft", "default")
        .expect("minecraft:default is a valid identifier")
});

/// NeoForge `ClientHooks.gatherTooltipComponentsFromElements` wrapping width calculation.
pub(super) fn wrap_tooltip_width(
    mut tooltip_text_width: i32,
    mouse_x: i32,
    screen_width: i32,
    max_width: i32,
) -> (i32, bool) {
    let mut needs_wrap = false;
    let mut tooltip_x = mouse_x.saturating_add(12);
    if tooltip_x
        .saturating_add(tooltip_text_width)
        .saturating_add(4)
        > screen_width
    {
        tooltip_x = mouse_x
            .saturating_sub(16)
            .saturating_sub(tooltip_text_width);
        if tooltip_x < 4 {
            tooltip_text_width = if mouse_x > screen_width / 2 {
                mouse_x.saturating_sub(12).saturating_sub(8)
            } else {
                screen_width.saturating_sub(16).saturating_sub(mouse_x)
            };
            needs_wrap = true;
        }
    }
    if max_width > 0 && tooltip_text_width > max_width {
        tooltip_text_width = max_width;
        needs_wrap = true;
    }
    (tooltip_text_width, needs_wrap)
}

pub(super) enum PreparedComponent<'a> {
    Text(EngineText, u32),
    Bundle(Cow<'a, ItemStack>, bundle::Contents),
    Custom(PreparedTooltipComponent),
}

enum TooltipInput<'a> {
    Text(EngineText, u32),
    Bundle(&'a ItemStack),
    Api(TooltipComponent),
}

pub(super) struct GatheredTooltip<'a> {
    pub(super) components: Vec<PreparedComponent<'a>>,
    pub(super) font: Identifier,
}

impl PreparedComponent<'_> {
    fn pre_snapshot(&self) -> Option<PreparedTooltipComponent> {
        match self {
            Self::Text(text, color) => Some(PreparedTooltipComponent::__text(
                text.try_into_api().ok()?,
                *color,
            )),
            Self::Bundle(stack, contents) => Some(PreparedTooltipComponent::__bundle(
                stack.try_into_api().ok()?,
                contents.tooltip_size(0),
                contents.items().is_empty().then(|| {
                    mod_api::Text::translate(bundle::TOOLTIP_EMPTY_DESCRIPTION, Vec::new())
                }),
            )),
            Self::Custom(custom) => custom.__share_custom(),
        }
    }
}

#[cfg(test)]
impl PreparedComponent<'_> {
    pub(super) fn as_text(&self) -> Option<(&EngineText, u32)> {
        match self {
            Self::Text(text, color) => Some((text, *color)),
            _ => None,
        }
    }
    pub(super) fn as_bundle(&self) -> Option<&bundle::Contents> {
        match self {
            Self::Bundle(_, contents) => Some(contents),
            _ => None,
        }
    }
    pub(super) fn downcast_ref<C: mod_api::ClientTooltipComponent>(&self) -> Option<&C> {
        match self {
            Self::Custom(custom) => custom.downcast_ref(),
            _ => None,
        }
    }
    fn width(&self, font: &dyn mod_api::TooltipFont) -> i32 {
        self.pre_snapshot().unwrap().width(font)
    }
    fn height(&self, font: &dyn mod_api::TooltipFont) -> i32 {
        self.pre_snapshot().unwrap().height(font)
    }
}

impl Game<'_> {
    /// `Item.getTooltipImage`: a mod-supplied payload or the original engine bundle.
    fn resolve_item_tooltip_image<'a>(&self, stack: &'a ItemStack) -> Option<TooltipInput<'a>> {
        if mod_api::__private::item_listens(mod_api::HookId::TooltipImage, &stack.item) {
            let mut component = None;
            self.native_tooltip_image(stack, &mut component);
            if let Some(component) = component {
                return Some(TooltipInput::Api(component));
            }
        }
        (sunrise_client::item_tooltip::tooltip_display(stack).shows("minecraft:bundle_contents")
            && stack.carries_component(self.items().get(&stack.item), "minecraft:bundle_contents"))
        .then_some(TooltipInput::Bundle(stack))
    }

    fn prepare_tooltip_image<'a>(
        &self,
        image: TooltipInput<'a>,
    ) -> Result<Option<PreparedComponent<'a>>, MissingTooltipComponentFactory> {
        let stack = match image {
            TooltipInput::Bundle(stack) => Cow::Borrowed(stack),
            TooltipInput::Api(component) => match component.as_bundle() {
                Some(stack) => match crate::game::api_values::stack_value(stack) {
                    Ok(stack) => Cow::Owned(stack),
                    Err(error) => {
                        log::warn!("invalid bundle tooltip data: {error:?}");
                        return Ok(None);
                    }
                },
                None => {
                    return sunrise_client::tooltip_components::create(&component)
                        .map(|component| component.map(PreparedComponent::Custom));
                }
            },
            TooltipInput::Text(..) => unreachable!("text is prepared by the text path"),
        };
        Ok(bundle::Contents::of(self.items(), &stack)
            .map(|contents| PreparedComponent::Bundle(stack, contents)))
    }

    /// `AbstractContainerScreen.showTooltipWithItemInHand` examines the selected image
    /// once before scheduling a separate Gather and tooltip render.
    pub(super) fn tooltip_with_carried(&self, stack: &ItemStack) -> bool {
        if self.carried.is_none() {
            return true;
        }
        let Some(image) = self.resolve_item_tooltip_image(stack) else {
            return false;
        };
        if matches!(&image, TooltipInput::Bundle(_)) {
            return true; // `ClientBundleTooltip.showTooltipWithItemInHand`.
        }
        match self.prepare_tooltip_image(image) {
            Ok(Some(PreparedComponent::Bundle(..))) => true,
            Ok(Some(PreparedComponent::Custom(component))) => {
                component.show_tooltip_with_item_in_hand()
            }
            Ok(_) => false,
            Err(error) => {
                log::warn!("tooltip for {} could not be prepared: {error}", stack.item);
                false
            }
        }
    }

    /// Gather raw elements, split only text in the item's font, and prepare images.
    /// Explicit cancellation produces an empty list; an unsupported type is a preparation error.
    pub(super) fn gather_tooltip_components<'a>(
        &self,
        stack: &'a ItemStack,
        lines: Vec<(EngineText, u32)>,
        mouse_x: i32,
        screen: (f32, f32),
    ) -> Result<GatheredTooltip<'a>, MissingTooltipComponentFactory> {
        let image = self.resolve_item_tooltip_image(stack);
        let font_id = self
            .item_tooltip_font(stack)
            .unwrap_or_else(|| DEFAULT_FONT_ID.clone());
        let mut elements: Vec<_> = lines
            .into_iter()
            .map(|(text, color)| TooltipInput::Text(text, color))
            .collect();
        if let Some(image) = image {
            elements.insert(usize::from(!elements.is_empty()), image);
        }
        let mut max_width = -1;
        if mod_api::__private::listened(mod_api::HookId::RenderTooltipGatherComponents)
            && let Ok(api_stack) = stack.try_into_api()
        {
            // Keep the original engine list intact if any element cannot be lent to Gather.
            let mut original_bundle = None;
            let snapshot: Option<Vec<_>> = elements
                .iter()
                .map(|element| {
                    Some(match element {
                        TooltipInput::Text(text, color) => {
                            TooltipElement::Text(text.try_into_api().ok()?, *color)
                        }
                        TooltipInput::Bundle(stack) => {
                            let component = TooltipComponent::bundle(stack.try_into_api().ok()?);
                            original_bundle = Some(component.clone());
                            TooltipElement::Component(component)
                        }
                        TooltipInput::Api(component) => {
                            TooltipElement::Component(component.clone())
                        }
                    })
                })
                .collect();
            if let Some(mut snapshot) = snapshot {
                let mut cancel = false;
                self.native_render_tooltip_gather_components(
                    mod_api::__private::RenderTooltipGatherComponentsCall {
                        stack: &api_stack,
                        screen_width: screen.0 as i32,
                        screen_height: screen.1 as i32,
                        elements: &mut snapshot,
                        max_width: &mut max_width,
                        cancel: &mut cancel,
                    },
                );
                if cancel {
                    return Ok(GatheredTooltip {
                        components: Vec::new(),
                        font: font_id,
                    });
                }
                // A moved or duplicated original bundle still points at the same engine data.
                let original_bundle = original_bundle
                    .as_ref()
                    .and_then(TooltipComponent::as_bundle);
                elements = snapshot
                    .into_iter()
                    .filter_map(|element| match element {
                        TooltipElement::Text(text, color) => match text.try_into_engine(&()) {
                            Ok(text) => Some(TooltipInput::Text(text, color)),
                            Err(error) => {
                                log::warn!(
                                    "invalid edited tooltip text for {}: {error:?}",
                                    stack.item
                                );
                                None
                            }
                        },
                        TooltipElement::Component(component) => {
                            let is_original_bundle = original_bundle
                                .zip(component.as_bundle())
                                .is_some_and(|(original, current)| std::ptr::eq(original, current));
                            Some(if is_original_bundle {
                                TooltipInput::Bundle(stack)
                            } else {
                                TooltipInput::Api(component)
                            })
                        }
                    })
                    .collect();
            }
        }

        let font = &self.gui.atlas.font;
        let font_name = font_id_str(&font_id);
        let mut text_width = 0;
        for element in &mut elements {
            if let TooltipInput::Text(text, _) = element {
                *text = text.resolve_legacy();
                text_width = text_width.max(font.width_text_with_font(text, &font_name) as i32);
            }
        }
        let (wrap_width, needs_wrap) =
            wrap_tooltip_width(text_width, mouse_x, screen.0 as i32, max_width);
        let mut components = Vec::with_capacity(elements.len());
        for element in elements {
            match element {
                TooltipInput::Text(text, color) if needs_wrap && !text.is_empty() => {
                    components.extend(
                        font.split_formatted_with_font(&text, wrap_width as f32, &font_name)
                            .into_iter()
                            .map(|line| PreparedComponent::Text(line, color)),
                    );
                }
                TooltipInput::Text(text, color) => {
                    components.push(PreparedComponent::Text(text, color))
                }
                image => components.extend(self.prepare_tooltip_image(image)?),
            }
        }
        Ok(GatheredTooltip {
            components,
            font: font_id,
        })
    }

    /// Lend Pre an API snapshot without making the ordinary renderer depend on its form.
    pub(super) fn tooltip_pre_components(
        &self,
        batch: &mut HudBatch,
        stack: Option<&ItemStack>,
        components: &[PreparedComponent<'_>],
        [x, y]: [i32; 2],
        screen: (f32, f32),
        mut font: Identifier,
    ) -> Option<([i32; 2], Identifier)> {
        if !mod_api::__private::listened(mod_api::HookId::RenderTooltipPre) {
            return Some(([x, y], font));
        }
        let api_stack = match stack.map(TryIntoApi::try_into_api).transpose() {
            Ok(stack) => stack,
            Err(_) => return Some(([x, y], font)),
        };
        let Some(snapshot) = components
            .iter()
            .map(PreparedComponent::pre_snapshot)
            .collect::<Option<Vec<_>>>()
        else {
            return Some(([x, y], font));
        };
        let mut place = mod_api::TooltipPlace { x, y };
        let mut cancel = false;
        let mut draw = GameDrawContext::new(self, batch);
        self.native_render_tooltip_pre(mod_api::__private::RenderTooltipPreCall {
            stack: api_stack.as_ref(),
            components: &snapshot,
            place: &mut place,
            screen: [screen.0, screen.1],
            positioner: &mod_api::DefaultTooltipPositioner,
            font: &mut font,
            draw: &mut draw,
            cancel: &mut cancel,
        });
        (!cancel).then_some(([place.x, place.y], font))
    }

    /// `GuiGraphicsExtractor.tooltip`: Pre, measurement without rewrapping, placement, Texture,
    /// then all text passes followed by all image passes. Returns the drawn content rectangle.
    pub(super) fn draw_prepared_tooltip(
        &self,
        batch: &mut HudBatch,
        stack: Option<&ItemStack>,
        components: &[PreparedComponent<'_>],
        at: [i32; 2],
        initial_font: Identifier,
        extra_space_after_first_line: bool,
    ) -> Option<[f32; 4]> {
        if components.is_empty() {
            return None;
        }
        let screen = (batch.width, batch.height);
        let (at, chosen_font) =
            self.tooltip_pre_components(batch, stack, components, at, screen, initial_font)?;
        let font = &self.gui.atlas.font;
        let tooltip_font = EngineTooltipFont {
            font,
            id: &chosen_font,
        };
        let font_name = font_id_str(&chosen_font);
        let mut w = 0;
        let mut h: i32 = if extra_space_after_first_line && components.len() != 1 {
            0
        } else {
            -2
        };
        let mut metrics = Vec::with_capacity(components.len());
        for component in components {
            let mut description = Vec::new();
            let [cw, ch] = match component {
                PreparedComponent::Text(text, _) => {
                    [font.width_text_with_font(text, &font_name) as i32, 10]
                }
                PreparedComponent::Bundle(_, contents) => {
                    if contents.items().is_empty() {
                        let text = EngineText::translate(bundle::TOOLTIP_EMPTY_DESCRIPTION, &[]);
                        description = font.split_formatted_with_font(
                            &text,
                            bundle::TOOLTIP_WIDTH as f32,
                            &font_name,
                        );
                    }
                    contents.tooltip_size(description.len())
                }
                PreparedComponent::Custom(custom) => {
                    [custom.width(&tooltip_font), custom.height(&tooltip_font)]
                }
            };
            w = w.max(cw);
            h = h.saturating_add(ch);
            metrics.push((ch, description));
        }
        let [x, y] = mod_api::ClientTooltipPositioner::position_tooltip(
            &mod_api::DefaultTooltipPositioner,
            [screen.0 as i32, screen.1 as i32],
            at,
            [w, h],
        );
        self.tooltip_box(batch, stack, x as f32, y as f32, w as f32, h as f32);

        let mut local_y = y;
        for (i, (component, (height, _))) in components.iter().zip(&metrics).enumerate() {
            match component {
                PreparedComponent::Text(text, color) => {
                    batch.text_styled_with_font(
                        &self.gui.atlas,
                        text,
                        x as f32,
                        local_y as f32,
                        *color,
                        true,
                        &font_name,
                    );
                }
                PreparedComponent::Custom(custom) => {
                    let paint = batch.paint_checkpoint();
                    let ok = custom.__draw_text(
                        &tooltip_font,
                        x,
                        local_y,
                        &mut GameDrawContext::new(self, batch),
                    );
                    if !ok {
                        paint.discard(batch);
                    }
                }
                PreparedComponent::Bundle(..) => {}
            }
            local_y = local_y.saturating_add(*height).saturating_add(
                if extra_space_after_first_line && i == 0 {
                    2
                } else {
                    0
                },
            );
        }

        let mut local_y = y;
        for (i, (component, (height, description))) in components.iter().zip(&metrics).enumerate() {
            match component {
                PreparedComponent::Bundle(_, contents) => {
                    let layout = contents.tooltip_layout(
                        x as f32,
                        local_y as f32,
                        w as f32,
                        description.len(),
                    );
                    self.draw_bundle_tooltip(batch, &layout, description, &font_name);
                    self.draw_bundle_selected_name(
                        batch,
                        &layout,
                        x as f32,
                        local_y as f32,
                        w as f32,
                        &font_name,
                    );
                }
                PreparedComponent::Custom(custom) => {
                    let paint = batch.paint_checkpoint();
                    let ok = custom.__draw_image(
                        &tooltip_font,
                        x,
                        local_y,
                        w,
                        h,
                        &mut GameDrawContext::new(self, batch),
                    );
                    if !ok {
                        paint.discard(batch);
                    }
                }
                PreparedComponent::Text(..) => {}
            }
            local_y = local_y.saturating_add(*height).saturating_add(
                if extra_space_after_first_line && i == 0 {
                    2
                } else {
                    0
                },
            );
        }
        Some([x as f32, y as f32, w as f32, h as f32])
    }

    /// A stack's tooltip: its ordered lines and a bundle's contents
    /// (`ClientBundleTooltip`: the 4 wide grid, the `+N` of what it leaves
    /// out, and the weight bar).
    pub(in crate::game) fn draw_stack_tooltip(
        &mut self,
        batch: &mut HudBatch,
        stack: &ItemStack,
        x: f32,
        y: f32,
    ) {
        self.draw_stack_tooltip_with(batch, stack, false, x, y, Vec::new());
    }

    /// [`Self::draw_stack_tooltip`] with `TooltipFlag.isCreative` set when
    /// `creative` (a creative tab's own slot), and `after_name` inserted from
    /// line 1, as the creative screen adds its tab names.
    pub(in crate::game) fn draw_stack_tooltip_with(
        &mut self,
        batch: &mut HudBatch,
        stack: &ItemStack,
        creative: bool,
        x: f32,
        y: f32,
        after_name: Vec<(sunrise_client::text::Text, u32)>,
    ) {
        let mut lines = self.stack_tooltip_lines(stack, false, creative);
        if lines.is_empty()
            && !sunrise_client::item_tooltip::tooltip_display(stack).name_visible(creative)
        {
            return;
        }
        let after_name_index = lines.len().min(1);
        lines.splice(after_name_index..after_name_index, after_name);
        self.draw_stack_tooltip_lines(batch, stack, lines, x, y);
    }

    /// Draw already-built item lines, keeping the stack's image, style and gather hooks.
    pub(in crate::game) fn draw_stack_tooltip_lines(
        &mut self,
        batch: &mut HudBatch,
        stack: &ItemStack,
        lines: Vec<(sunrise_client::text::Text, u32)>,
        x: f32,
        y: f32,
    ) {
        let at = [x.round() as i32, y.round() as i32];
        match self.gather_tooltip_components(stack, lines, at[0], (batch.width, batch.height)) {
            Ok(tooltip) => {
                self.draw_prepared_tooltip(
                    batch,
                    Some(stack),
                    &tooltip.components,
                    at,
                    tooltip.font,
                    true,
                );
            }
            Err(error) => log::warn!("tooltip for {} could not be prepared: {error}", stack.item),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "daily"]
    fn rendered_image_callback_receives_whole_box() {
        use mod_api::{
            ClientHost, ClientTooltipComponent, ClientTooltipComponentFactoriesRegistrar,
            DrawContext, Hooks, ItemPart, ModEntry, Mods, Registrar,
            RenderTooltipGatherComponentsEvent, TooltipComponent, TooltipComponentData,
            TooltipFont, TooltipImageEvent,
        };
        use std::sync::atomic::{AtomicBool, Ordering};
        use sunrise_client::tooltip_components::{TooltipComponentFactories, with_test_factories};

        static TEXT_DRAWN: AtomicBool = AtomicBool::new(false);
        static IMAGE_DRAWN: AtomicBool = AtomicBool::new(false);

        struct BoxData;
        impl TooltipComponentData for BoxData {}

        struct BoxClient;
        impl ClientTooltipComponent for BoxClient {
            fn height(&self, _: &dyn TooltipFont) -> i32 {
                16
            }
            fn width(&self, _: &dyn TooltipFont) -> i32 {
                70
            }
            fn draw_text(&self, _: &dyn TooltipFont, x: i32, y: i32, _: &mut dyn DrawContext) {
                assert_eq!((x, y), (22, 20));
                assert!(!IMAGE_DRAWN.load(Ordering::Relaxed));
                TEXT_DRAWN.store(true, Ordering::Relaxed);
            }
            fn draw_image(
                &self,
                _: &dyn TooltipFont,
                x: i32,
                y: i32,
                width: i32,
                height: i32,
                _: &mut dyn DrawContext,
            ) {
                assert!(TEXT_DRAWN.load(Ordering::Relaxed));
                assert_eq!((x, y), (22, 20));
                assert_eq!((width, height), (70, 46));
                IMAGE_DRAWN.store(true, Ordering::Relaxed);
            }
        }

        #[derive(Clone)]
        struct Image;
        #[mod_api::sunrise_part]
        impl ItemPart for Image {
            fn tooltip_image(
                &self,
                _: &dyn ClientHost,
                _: &mut TooltipImageEvent<'_>,
            ) -> Option<TooltipComponent> {
                Some(TooltipComponent::custom(BoxData))
            }
        }

        struct Mod;
        #[mod_api::__subscriptions]
        impl Hooks for Mod {
            fn register(&self, r: &mut dyn Registrar, _: &mod_api::RegistrationContext) {
                let key = r.item_key("minecraft:stick");
                r.item_part(key, Image);
            }
            fn render_tooltip_gather_components(
                &self,
                _: &dyn ClientHost,
                event: &mut RenderTooltipGatherComponentsEvent<'_>,
            ) -> mod_api::Flow {
                assert!(matches!(event.elements[1], TooltipElement::Component(_)));
                let component = event.elements[1].clone();
                *event.elements = vec![
                    TooltipElement::Text(mod_api::Text::literal("A"), u32::MAX),
                    component,
                    TooltipElement::Text(mod_api::Text::literal("B"), u32::MAX),
                    TooltipElement::Text(mod_api::Text::literal("C"), u32::MAX),
                ];
                mod_api::Flow::Continue
            }
        }

        let mut factories = TooltipComponentFactories::default();
        let registrar: &mut dyn ClientTooltipComponentFactoriesRegistrar = &mut factories;
        registrar.register::<BoxData, BoxClient>(|_| BoxClient);

        let mods: Mods = vec![ModEntry::new("image", Box::new(Mod))].into();
        crate::game::api_values::install_parts(&mods).unwrap();
        mod_platform::testing::with_mods(mods, || {
            with_test_factories(factories, || {
                crate::game::native_entity_refs::tests::with_client_game(|game| {
                    TEXT_DRAWN.store(false, Ordering::Relaxed);
                    IMAGE_DRAWN.store(false, Ordering::Relaxed);
                    game.draw_stack_tooltip(
                        &mut HudBatch::scratch([320, 240]),
                        &ItemStack::new("minecraft:stick", 1),
                        10.0,
                        20.0,
                    );
                    assert!(TEXT_DRAWN.load(Ordering::Relaxed));
                    assert!(IMAGE_DRAWN.load(Ordering::Relaxed));
                });
            });
        });
    }

    #[test]
    fn wrap_tooltip_width_matches_neoforge_client_hooks() {
        // A: fits on the right side without wrapping (including exact screen_width boundary).
        assert_eq!(wrap_tooltip_width(60, 10, 320, -1), (60, false));
        assert_eq!(wrap_tooltip_width(100, 100, 216, -1), (100, false));
        // B: overflows right, flips left cleanly without wrapping (including exact tooltip_x == 4).
        assert_eq!(wrap_tooltip_width(80, 260, 320, -1), (80, false));
        assert_eq!(wrap_tooltip_width(80, 100, 190, -1), (80, false));
        // C: overflows both sides; cursor on right half (mouse_x > screen_width / 2).
        assert_eq!(wrap_tooltip_width(180, 180, 320, -1), (160, true));
        // D: overflows both sides; cursor on left half (mouse_x < screen_width / 2).
        assert_eq!(wrap_tooltip_width(210, 100, 320, -1), (204, true));
        // E: exact midpoint (mouse_x == screen_width / 2) uses the left-half formula.
        assert_eq!(wrap_tooltip_width(200, 160, 320, -1), (144, true));
        // F: explicit positive max_width narrows and wraps; >= width or <= 0 leaves wrapping off.
        assert_eq!(wrap_tooltip_width(100, 10, 320, 40), (40, true));
        assert_eq!(wrap_tooltip_width(100, 10, 320, 100), (100, false));
        assert_eq!(wrap_tooltip_width(100, 10, 320, 140), (100, false));
        assert_eq!(wrap_tooltip_width(100, 10, 320, 0), (100, false));
        // G: max_width tighter than screen wrap narrows further.
        assert_eq!(wrap_tooltip_width(210, 180, 320, 100), (100, true));
        // H: max_width wider than screen wrap keeps screen wrap width.
        assert_eq!(wrap_tooltip_width(210, 180, 320, 180), (160, true));
        // Extreme values never overflow.
        let _ = wrap_tooltip_width(i32::MAX, i32::MAX, i32::MAX, i32::MAX);
        let _ = wrap_tooltip_width(i32::MIN, i32::MIN, i32::MIN, i32::MIN);
    }
}

#[cfg(test)]
#[path = "tooltip_bundle_tests.rs"]
mod bundle_tests;

#[cfg(test)]
#[path = "tooltip_component_tests.rs"]
mod component_tests;
