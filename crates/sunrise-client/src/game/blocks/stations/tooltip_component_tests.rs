//! Unit tests for typed client tooltip components: registration & override, NeoForge wrapping
//! fixtures A–K, two-pass draw order & geometry (including audit [87] zero-sized components),
//! `render_tooltip_pre` font replacement & cancellation, `render_tooltip_texture` & line-only
//! `ItemResource::tooltip`, `show_tooltip_with_item_in_hand`, audit [88], and panic containment.
use super::*;
use crate::game::native_entity_refs::tests::with_client_game;
use mod_api::{
    __private::{ModPhase, PhaseOwner, game::RegistrationContextNew},
    ClientHost, ClientTooltipComponent, DrawContext, Flow, Hooks, Identifier, ItemPart,
    ItemResource, ModEntry, Mods, PreparedTooltipComponent,
    RegisterClientTooltipComponentFactoriesEvent, Registrar, RegistrationContext,
    RenderTooltipGatherComponentsEvent, RenderTooltipPreEvent, RenderTooltipTextureEvent,
    TooltipComponent, TooltipComponentData, TooltipElement, TooltipFont, TooltipImageEvent,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicUsize, Ordering},
};
use sunrise_client::tooltip_components::{TooltipComponentFactories, with_test_factories};

fn collect_factories() -> TooltipComponentFactories {
    let mut factories = TooltipComponentFactories::default();
    let _phase = PhaseOwner::scope(ModPhase::Registration);
    Game::native_register_client_tooltip_component_factories(
        &RegistrationContext::new(),
        &mut factories,
    );
    factories
}

fn with_mod_factories(mods: Mods, test: impl FnOnce(&mut Game<'_>)) {
    crate::game::api_values::install_parts(&mods).unwrap();
    mod_platform::testing::with_mods(mods, || {
        let factories = collect_factories();
        with_test_factories(factories, || {
            with_client_game(test);
        });
    });
}

fn populated_bundle() -> ItemStack {
    let mut stack = ItemStack::new("minecraft:bundle", 1);
    stack.data_mut().bundle_contents = vec![ItemStack::new("minecraft:stone", 1)];
    stack
}

fn text_lines(components: &[PreparedTooltipComponent]) -> Vec<String> {
    components
        .iter()
        .filter_map(|component| {
            let (api_text, _) = component.as_text()?;
            let engine: sunrise_client::text::Text = api_text.try_into_engine(&()).ok()?;
            Some(engine.plain())
        })
        .collect()
}

/// Covers NeoForge wrapping fixtures A–K end-to-end through `gather_tooltip_components`,
/// factory registration & last-wins override, and graceful omission of unregistered components.
#[test]
#[ignore = "daily"]
fn gather_wrapping_fixtures_preserve_empty_lines_and_exclude_components() {
    struct WideData(i32, i32);
    impl TooltipComponentData for WideData {}

    struct UnregisteredData;
    impl TooltipComponentData for UnregisteredData {}

    struct WideClient(i32, i32);
    impl ClientTooltipComponent for WideClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            self.1
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            self.0
        }
    }

    struct FirstFactory;
    #[mod_api::__subscriptions]
    impl Hooks for FirstFactory {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            event
                .factories
                .register::<WideData, WideClient>(|d| WideClient(d.0, d.1));
        }
    }

    struct OverrideFactory;
    #[mod_api::__subscriptions]
    impl Hooks for OverrideFactory {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            // Last registration wins for the same TooltipComponentData type.
            event
                .factories
                .register::<WideData, WideClient>(|d| WideClient(d.0 * 2, d.1));
        }
    }

    struct GatherConfig {
        max_width: AtomicI32,
        mode: AtomicU8,
        bundle: Mutex<Option<mod_api::ItemStack>>,
    }

    struct GatherMod(Arc<GatherConfig>);
    #[mod_api::__subscriptions]
    impl Hooks for GatherMod {
        fn render_tooltip_gather_components(
            &self,
            _: &dyn ClientHost,
            event: &mut RenderTooltipGatherComponentsEvent<'_>,
        ) -> Flow {
            event.max_width = self.0.max_width.load(Ordering::Relaxed);
            match self.0.mode.load(Ordering::Relaxed) {
                1 => {
                    // Fixtures I & J: empty line + custom component + bundle + unregistered + tail.
                    let bundle_stack = self.0.bundle.lock().unwrap().clone().unwrap();
                    event
                        .elements
                        .push(TooltipElement::Text(mod_api::Text::literal(""), u32::MAX));
                    event
                        .elements
                        .push(TooltipElement::Component(TooltipComponent::custom(
                            WideData(250, 12),
                        )));
                    event
                        .elements
                        .push(TooltipElement::Component(TooltipComponent::bundle(
                            bundle_stack,
                        )));
                    event
                        .elements
                        .push(TooltipElement::Component(TooltipComponent::custom(
                            UnregisteredData,
                        )));
                    event.elements.push(TooltipElement::Text(
                        mod_api::Text::literal("tail"),
                        u32::MAX,
                    ));
                }
                2 => {
                    // Fixture K: wide custom (500px) & bundle (96px) beside short text.
                    let bundle_stack = self.0.bundle.lock().unwrap().clone().unwrap();
                    event
                        .elements
                        .push(TooltipElement::Component(TooltipComponent::custom(
                            WideData(250, 12),
                        )));
                    event
                        .elements
                        .push(TooltipElement::Component(TooltipComponent::bundle(
                            bundle_stack,
                        )));
                }
                _ => {}
            }
            Flow::Continue
        }
    }

    let config = Arc::new(GatherConfig {
        max_width: AtomicI32::new(-1),
        mode: AtomicU8::new(0),
        bundle: Mutex::new(None),
    });
    let mods: Mods = vec![
        ModEntry::new("first_factory", Box::new(FirstFactory)),
        ModEntry::new("override_factory", Box::new(OverrideFactory)),
        ModEntry::new("gather_mod", Box::new(GatherMod(config.clone()))),
    ]
    .into();

    with_mod_factories(mods, |game| {
        let stick = ItemStack::new("minecraft:stick", 1);
        *config.bundle.lock().unwrap() = Some(populated_bundle().try_into_api().unwrap());
        let font = &game.gui.atlas.font;
        let long_text = sunrise_client::text::Text::literal(
            "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron",
        );
        assert!(font.width_text(&long_text) > 320.0);

        let split_plain = |width: i32| -> Vec<String> {
            font.split_formatted(&long_text, width as f32)
                .into_iter()
                .map(|line| line.plain())
                .collect()
        };

        // Fixtures A & B: fits right and right-edge flip without wrapping.
        let short = sunrise_client::text::Text::literal("alpha beta");
        for mouse_x in [10, 260] {
            let gathered = game
                .gather_tooltip_components(
                    &stick,
                    vec![(short.clone(), u32::MAX)],
                    mouse_x,
                    (320.0, 240.0),
                )
                .unwrap().components;
            assert_eq!(text_lines(&gathered), ["alpha beta"]);
        }

        // Fixture C: right-half narrow screen wrap (mouse_x > screen_width / 2 -> 180 - 12 - 8 = 160).
        let right_wrap = game
            .gather_tooltip_components(
                &stick,
                vec![(long_text.clone(), u32::MAX)],
                180,
                (320.0, 240.0),
            )
            .unwrap().components;
        assert_eq!(text_lines(&right_wrap), split_plain(160));

        // Fixture D: left-half narrow screen wrap (mouse_x < screen_width / 2 -> 320 - 16 - 100 = 204).
        let left_wrap = game
            .gather_tooltip_components(
                &stick,
                vec![(long_text.clone(), u32::MAX)],
                100,
                (320.0, 240.0),
            )
            .unwrap().components;
        assert_eq!(text_lines(&left_wrap), split_plain(204));

        // Fixture E: exact midpoint (mouse_x == screen_width / 2 -> 320 - 16 - 160 = 144).
        let mid_wrap = game
            .gather_tooltip_components(
                &stick,
                vec![(long_text.clone(), u32::MAX)],
                160,
                (320.0, 240.0),
            )
            .unwrap().components;
        assert_eq!(text_lines(&mid_wrap), split_plain(144));

        // Fixture F: explicit max_width wraps a tooltip that otherwise fits.
        config.max_width.store(50, Ordering::Relaxed);
        let max_wrap = game
            .gather_tooltip_components(
                &stick,
                vec![(long_text.clone(), u32::MAX)],
                10,
                (320.0, 240.0),
            )
            .unwrap().components;
        assert_eq!(text_lines(&max_wrap), split_plain(50));

        // Fixture G: max_width tighter than screen wrap (80 < 160) wins.
        config.max_width.store(80, Ordering::Relaxed);
        let tighter = game
            .gather_tooltip_components(
                &stick,
                vec![(long_text.clone(), u32::MAX)],
                180,
                (320.0, 240.0),
            )
            .unwrap().components;
        assert_eq!(text_lines(&tighter), split_plain(80));

        // Fixture H: max_width wider than screen wrap (200 > 160) keeps screen wrap width.
        config.max_width.store(200, Ordering::Relaxed);
        let wider = game
            .gather_tooltip_components(
                &stick,
                vec![(long_text.clone(), u32::MAX)],
                180,
                (320.0, 240.0),
            )
            .unwrap().components;
        assert_eq!(text_lines(&wider), split_plain(160));

        // Fixtures I & J: empty line preserved, custom override wins, bundle kept, unregistered omitted.
        config.max_width.store(60, Ordering::Relaxed);
        config.mode.store(1, Ordering::Relaxed);
        let mixed = game
            .gather_tooltip_components(
                &stick,
                vec![(long_text.clone(), u32::MAX)],
                10,
                (320.0, 240.0),
            )
            .unwrap().components;
        let expected_wrapped = split_plain(60);
        let n = expected_wrapped.len();
        assert!(n > 1);
        assert_eq!(mixed.len(), n + 4);
        assert_eq!(text_lines(&mixed[..n]), expected_wrapped);
        // Empty text line preserved as a single empty text component right after wrapped text.
        assert_eq!(text_lines(&mixed[n..=n]), [""]);
        // Override factory won: WideData(250, 12) -> WideClient(500, 12).
        let tooltip_font = crate::game::host::draw::EngineTooltipFont {
            font,
            id: &DEFAULT_FONT_ID,
        };
        let wide = mixed[n + 1]
            .downcast_ref::<WideClient>()
            .expect("override factory should prepare WideClient");
        assert_eq!((wide.0, wide.1), (500, 12));
        assert_eq!(
            (
                mixed[n + 1].width(&tooltip_font),
                mixed[n + 1].height(&tooltip_font)
            ),
            (500, 12)
        );
        // Bundle component kept in order; UnregisteredData omitted without panic.
        assert!(mixed[n + 2].as_bundle().is_some());
        assert_eq!(text_lines(&mixed[n + 3..]), ["tail"]);

        // Fixture K: wide custom (500px) and bundle (96px) excluded from tooltipTextWidth.
        config.max_width.store(-1, Ordering::Relaxed);
        config.mode.store(2, Ordering::Relaxed);
        let excluded = game
            .gather_tooltip_components(
                &stick,
                vec![(sunrise_client::text::Text::literal("short"), u32::MAX)],
                10,
                (100.0, 240.0),
            )
            .unwrap().components;
        assert_eq!(excluded.len(), 3);
        assert_eq!(text_lines(&excluded[..1]), ["short"]);
        assert!(excluded[1].downcast_ref::<WideClient>().is_some());
        assert!(excluded[2].as_bundle().is_some());
    });
}

/// Verifies two-pass draw order (all `draw_text` before any `draw_image`), exact `(x, local_y, w, h)`
/// geometry (including `+2` after index 0 when `len > 1` and `-2` start when `len == 1`), and
/// retention of zero-width and zero-height components (audit [87]).
#[test]
#[ignore = "daily"]
fn two_pass_draw_order_geometry_and_zero_sized_components() {
    struct ZeroWidthData;
    impl TooltipComponentData for ZeroWidthData {}
    struct ZeroHeightData;
    impl TooltipComponentData for ZeroHeightData {}
    struct PanelData;
    impl TooltipComponentData for PanelData {}
    struct SingleData;
    impl TooltipComponentData for SingleData {}

    struct RecordingClient {
        tag: &'static str,
        w: i32,
        h: i32,
        log: Arc<Mutex<Vec<String>>>,
    }
    impl ClientTooltipComponent for RecordingClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            self.h
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            self.w
        }
        fn draw_text(&self, _: &dyn TooltipFont, x: i32, y: i32, _: &mut dyn DrawContext) {
            self.log
                .lock()
                .unwrap()
                .push(format!("{}:text({x},{y})", self.tag));
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
            self.log
                .lock()
                .unwrap()
                .push(format!("{}:image({x},{y},{width},{height})", self.tag));
        }
    }

    struct FactoryMod(Arc<Mutex<Vec<String>>>);
    #[mod_api::__subscriptions]
    impl Hooks for FactoryMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let log = self.0.clone();
            event
                .factories
                .register::<ZeroWidthData, RecordingClient>(move |_| RecordingClient {
                    tag: "zero_w",
                    w: 0,
                    h: 6,
                    log: log.clone(),
                });
            let log = self.0.clone();
            event
                .factories
                .register::<ZeroHeightData, RecordingClient>(move |_| RecordingClient {
                    tag: "zero_h",
                    w: 12,
                    h: 0,
                    log: log.clone(),
                });
            let log = self.0.clone();
            event
                .factories
                .register::<PanelData, RecordingClient>(move |_| RecordingClient {
                    tag: "panel",
                    w: 80,
                    h: 16,
                    log: log.clone(),
                });
            let log = self.0.clone();
            event
                .factories
                .register::<SingleData, RecordingClient>(move |_| RecordingClient {
                    tag: "single",
                    w: 40,
                    h: 16,
                    log: log.clone(),
                });
        }
    }

    let log = Arc::new(Mutex::new(Vec::new()));
    let mods: Mods = vec![ModEntry::new(
        "order_mod",
        Box::new(FactoryMod(log.clone())),
    )]
    .into();

    with_mod_factories(mods, |game| {
        let components = vec![
            PreparedComponent::Text(sunrise_client::text::Text::literal("A"), u32::MAX),
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(ZeroWidthData)).unwrap()
                .unwrap(),
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(ZeroHeightData)).unwrap()
                .unwrap(),
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(PanelData)).unwrap()
                .unwrap(),
        ];
        let mut batch = HudBatch::scratch([320, 240]);
        let rect = game.draw_prepared_tooltip(&mut batch, None, &components, [10, 20], DEFAULT_FONT_ID.clone(), true);
        // x = 10 + 12 = 22, y = 20 - 12 = 8, w = 80, h = 0 + 10 + 6 + 0 + 16 = 32.
        assert_eq!(rect, Some([22.0, 8.0, 80.0, 32.0]));
        assert_eq!(
            std::mem::take(&mut *log.lock().unwrap()),
            [
                "zero_w:text(22,20)",
                "zero_h:text(22,26)",
                "panel:text(22,26)",
                "zero_w:image(22,20,80,32)",
                "zero_h:image(22,26,80,32)",
                "panel:image(22,26,80,32)",
            ]
        );

        // Single-component tooltip: height starts at -2 (16 - 2 = 14) with no +2 gap.
        let single = vec![
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(SingleData)).unwrap()
                .unwrap(),
        ];
        let mut single_batch = HudBatch::scratch([320, 240]);
        let single_rect =
            game.draw_prepared_tooltip(&mut single_batch, None, &single, [10, 20], DEFAULT_FONT_ID.clone(), true);
        assert_eq!(single_rect, Some([22.0, 8.0, 40.0, 14.0]));
        assert_eq!(
            std::mem::take(&mut *log.lock().unwrap()),
            ["single:text(22,8)", "single:image(22,8,40,14)"]
        );
    });
}

/// Verifies `render_tooltip_pre`: inspects prepared components (`as_text`, `as_bundle`,
/// `downcast_ref`), checks `positioner`, moves `place`, replaces `font` without rewrapping, and
/// cancels drawing while keeping custom draws submitted in `Pre`.
#[test]
#[ignore = "daily"]
fn render_tooltip_pre_observes_components_replaces_font_without_rewrap_and_cancels() {
    struct FontAwareData;
    impl TooltipComponentData for FontAwareData {}

    struct FontState {
        fonts_seen: Mutex<Vec<String>>,
        draws: AtomicUsize,
        pre_mode: AtomicU8,
        pre_observed_texts: AtomicUsize,
        pre_saw_bundle: AtomicBool,
        pre_saw_custom: AtomicBool,
        bundle_stack: Mutex<Option<mod_api::ItemStack>>,
    }

    struct FontAwareClient(Arc<FontState>);
    impl ClientTooltipComponent for FontAwareClient {
        fn height(&self, font: &dyn TooltipFont) -> i32 {
            self.0
                .fonts_seen
                .lock()
                .unwrap()
                .push(format!("h:{}", font.id()));
            12
        }
        fn width(&self, font: &dyn TooltipFont) -> i32 {
            self.0
                .fonts_seen
                .lock()
                .unwrap()
                .push(format!("w:{}", font.id()));
            if font.id().path() == "alt" { 130 } else { 30 }
        }
        fn draw_text(&self, font: &dyn TooltipFont, _: i32, _: i32, _: &mut dyn DrawContext) {
            self.0
                .fonts_seen
                .lock()
                .unwrap()
                .push(format!("t:{}", font.id()));
        }
        fn draw_image(
            &self,
            font: &dyn TooltipFont,
            x: i32,
            y: i32,
            _: i32,
            _: i32,
            draw: &mut dyn DrawContext,
        ) {
            self.0
                .fonts_seen
                .lock()
                .unwrap()
                .push(format!("i:{}", font.id()));
            self.0.draws.fetch_add(1, Ordering::Relaxed);
            draw.fill([x as f32, y as f32, 6.0, 6.0], 0xFF11_2233);
        }
    }

    struct PreMod(Arc<FontState>);
    #[mod_api::__subscriptions]
    impl Hooks for PreMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let state = self.0.clone();
            event
                .factories
                .register::<FontAwareData, FontAwareClient>(move |_| {
                    FontAwareClient(state.clone())
                });
        }
        fn render_tooltip_gather_components(
            &self,
            _: &dyn ClientHost,
            event: &mut RenderTooltipGatherComponentsEvent<'_>,
        ) -> Flow {
            event.max_width = 55;
            let bundle = self.0.bundle_stack.lock().unwrap().clone().unwrap();
            event
                .elements
                .push(TooltipElement::Component(TooltipComponent::bundle(bundle)));
            event
                .elements
                .push(TooltipElement::Component(TooltipComponent::custom(
                    FontAwareData,
                )));
            Flow::Continue
        }
        fn render_tooltip_pre(
            &self,
            _: &dyn ClientHost,
            event: &mut RenderTooltipPreEvent<'_>,
        ) -> Flow {
            assert_eq!(
                event
                    .positioner
                    .position_tooltip([320, 240], [10, 20], [50, 20]),
                [22, 8]
            );
            let texts = event
                .components
                .iter()
                .filter(|c| c.as_text().is_some())
                .count();
            let has_bundle = event.components.iter().any(|c| c.as_bundle().is_some());
            let has_custom = event
                .components
                .iter()
                .any(|c| c.downcast_ref::<FontAwareClient>().is_some());
            self.0.pre_observed_texts.store(texts, Ordering::Relaxed);
            self.0.pre_saw_bundle.store(has_bundle, Ordering::Relaxed);
            self.0.pre_saw_custom.store(has_custom, Ordering::Relaxed);

            match self.0.pre_mode.load(Ordering::Relaxed) {
                1 => {
                    event.place.x = 40;
                    event.place.y = 50;
                    *event.font = Identifier::parse("minecraft:alt").unwrap();
                    Flow::Continue
                }
                2 => {
                    event.draw.fill([5.0, 5.0, 10.0, 10.0], 0xFFAA_BBCC);
                    Flow::Cancel
                }
                _ => Flow::Continue,
            }
        }
    }

    let state = Arc::new(FontState {
        fonts_seen: Mutex::new(Vec::new()),
        draws: AtomicUsize::new(0),
        pre_mode: AtomicU8::new(1),
        pre_observed_texts: AtomicUsize::new(0),
        pre_saw_bundle: AtomicBool::new(false),
        pre_saw_custom: AtomicBool::new(false),
        bundle_stack: Mutex::new(None),
    });
    let mods: Mods = vec![ModEntry::new("pre_mod", Box::new(PreMod(state.clone())))].into();

    with_mod_factories(mods, |game| {
        let stick = ItemStack::new("minecraft:stick", 1);
        *state.bundle_stack.lock().unwrap() = Some(populated_bundle().try_into_api().unwrap());
        let line = sunrise_client::text::Text::literal("one two three four five six");
        let expected_wrapped = game.gui.atlas.font.split_formatted(&line, 55.0);
        let wrapped_count = expected_wrapped.len();
        assert!(wrapped_count > 1);

        let prepared = game
            .gather_tooltip_components(&stick, vec![(line, u32::MAX)], 10, (320.0, 240.0))
            .unwrap().components;
        assert_eq!(prepared.len(), wrapped_count + 2);

        // Mode 1: move place and switch font to minecraft:alt without rewrapping.
        let mut batch = HudBatch::scratch([320, 240]);
        let rect =
            game.draw_prepared_tooltip(&mut batch, Some(&stick), &prepared, [10, 20], DEFAULT_FONT_ID.clone(), true);
        let expected_h = (wrapped_count as i32) * 10 + 45 + 12;
        assert_eq!(rect, Some([52.0, 38.0, 130.0, expected_h as f32]));
        assert_eq!(
            state.pre_observed_texts.load(Ordering::Relaxed),
            wrapped_count
        );
        assert!(state.pre_saw_bundle.load(Ordering::Relaxed));
        assert!(state.pre_saw_custom.load(Ordering::Relaxed));
        assert_eq!(
            std::mem::take(&mut *state.fonts_seen.lock().unwrap()),
            [
                "w:minecraft:alt",
                "h:minecraft:alt",
                "t:minecraft:alt",
                "i:minecraft:alt",
            ]
        );
        assert_eq!(state.draws.load(Ordering::Relaxed), 1);
        assert!(
            batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0x11, 0x22, 0x33, 0xFF))
        );

        // Mode 2: cancel in Pre skips tooltip background and components, keeping Pre's custom draw.
        state.pre_mode.store(2, Ordering::Relaxed);
        let mut cancelled_batch = HudBatch::scratch([320, 240]);
        let cancelled = game.draw_prepared_tooltip(
            &mut cancelled_batch,
            Some(&stick),
            &prepared,
            [10, 20], DEFAULT_FONT_ID.clone(),
            true,
        );
        assert_eq!(cancelled, None);
        assert_eq!(state.draws.load(Ordering::Relaxed), 1);
        assert!(state.fonts_seen.lock().unwrap().is_empty());
        assert!(
            cancelled_batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0xAA, 0xBB, 0xCC, 0xFF)),
            "custom Pre draw must remain after cancel"
        );
        assert!(
            !cancelled_batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0x11, 0x22, 0x33, 0xFF)),
            "cancelled tooltip must not draw custom component"
        );
        assert!(
            !cancelled_batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0x10, 0x00, 0x10, 0xF0)),
            "cancelled tooltip must not draw vanilla background"
        );
    });
}

/// Verifies `render_tooltip_texture` still runs for tooltips with custom components and
/// `ItemResource::tooltip` still returns only `TooltipElement::Text` lines.
#[test]
#[ignore = "daily"]
fn tooltip_texture_and_item_resource_tooltip_with_custom_components() {
    struct BadgeData;
    impl TooltipComponentData for BadgeData {}

    struct BadgeClient;
    impl ClientTooltipComponent for BadgeClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            12
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            24
        }
        fn draw_image(
            &self,
            _: &dyn TooltipFont,
            x: i32,
            y: i32,
            _: i32,
            _: i32,
            draw: &mut dyn DrawContext,
        ) {
            draw.fill([x as f32, y as f32, 8.0, 8.0], 0xFF33_4455);
        }
    }

    #[derive(Clone)]
    struct BadgePart;
    #[mod_api::sunrise_part]
    impl ItemPart for BadgePart {
        fn tooltip_image(
            &self,
            _: &dyn ClientHost,
            _: &mut TooltipImageEvent<'_>,
        ) -> Option<TooltipComponent> {
            Some(TooltipComponent::custom(BadgeData))
        }
    }

    struct TextureMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for TextureMod {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let stick = r.item_key("minecraft:stick");
            r.item_part(stick, BadgePart);
        }
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            event
                .factories
                .register::<BadgeData, BadgeClient>(|_| BadgeClient);
        }
        fn render_tooltip_texture(
            &self,
            _: &dyn ClientHost,
            event: &mut RenderTooltipTextureEvent<'_>,
        ) {
            assert!(event.stack.is_some());
            self.0.fetch_add(1, Ordering::Relaxed);
            event.background = 0xFF22_6699;
        }
    }

    let texture_calls = Arc::new(AtomicUsize::new(0));
    let mods: Mods = vec![ModEntry::new(
        "texture_mod",
        Box::new(TextureMod(texture_calls.clone())),
    )]
    .into();

    with_mod_factories(mods, |game| {
        let stick = ItemStack::new("minecraft:stick", 1);
        let mut batch = HudBatch::scratch([320, 240]);
        game.draw_stack_tooltip(&mut batch, &stick, 10.0, 20.0);
        assert_eq!(texture_calls.load(Ordering::Relaxed), 1);
        assert!(
            batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0x22, 0x66, 0x99, 0xFF)),
            "render_tooltip_texture background must apply to custom-component tooltips"
        );
        assert!(
            batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0x33, 0x44, 0x55, 0xFF)),
            "custom component must draw alongside the textured box"
        );

        // ItemResource::tooltip stays line-only (TooltipElement::Text) for both custom and bundle items.
        for stack in [stick, populated_bundle()] {
            let resource = ItemResource::of(&crate::game::api_values::test_snapshot(&stack));
            let lines = resource.tooltip(game, false, false).unwrap();
            assert!(!lines.is_empty());
            assert!(
                lines
                    .iter()
                    .all(|element| matches!(element, TooltipElement::Text(..))),
                "ItemResource::tooltip must return only TooltipElement::Text: {lines:?}"
            );
        }
    });
}

/// Verifies `ClientTooltipComponent::show_tooltip_with_item_in_hand()` controls whether a hovered
/// slot tooltip stays visible while `game.carried` holds an item stack.
#[test]
#[ignore = "daily"]
fn show_tooltip_with_item_in_hand_controls_carried_visibility() {
    struct AlwaysShowData;
    impl TooltipComponentData for AlwaysShowData {}
    struct HideWithHandData;
    impl TooltipComponentData for HideWithHandData {}

    struct AlwaysShowClient;
    impl ClientTooltipComponent for AlwaysShowClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            20
        }
        fn show_tooltip_with_item_in_hand(&self) -> bool {
            true
        }
    }

    struct HideWithHandClient;
    impl ClientTooltipComponent for HideWithHandClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            20
        }
        fn show_tooltip_with_item_in_hand(&self) -> bool {
            false
        }
    }

    #[derive(Clone)]
    struct AlwaysShowPart;
    #[mod_api::sunrise_part]
    impl ItemPart for AlwaysShowPart {
        fn tooltip_image(
            &self,
            _: &dyn ClientHost,
            _: &mut TooltipImageEvent<'_>,
        ) -> Option<TooltipComponent> {
            Some(TooltipComponent::custom(AlwaysShowData))
        }
    }

    #[derive(Clone)]
    struct HideWithHandPart;
    #[mod_api::sunrise_part]
    impl ItemPart for HideWithHandPart {
        fn tooltip_image(
            &self,
            _: &dyn ClientHost,
            _: &mut TooltipImageEvent<'_>,
        ) -> Option<TooltipComponent> {
            Some(TooltipComponent::custom(HideWithHandData))
        }
    }

    struct HandMod;
    #[mod_api::__subscriptions]
    impl Hooks for HandMod {
        fn register(&self, r: &mut dyn Registrar, _: &RegistrationContext) {
            let stick = r.item_key("minecraft:stick");
            r.item_part(stick, AlwaysShowPart);
            let apple = r.item_key("minecraft:apple");
            r.item_part(apple, HideWithHandPart);
        }
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            event
                .factories
                .register::<AlwaysShowData, AlwaysShowClient>(|_| AlwaysShowClient);
            event
                .factories
                .register::<HideWithHandData, HideWithHandClient>(|_| HideWithHandClient);
        }
    }

    let mods: Mods = vec![ModEntry::new("hand_mod", Box::new(HandMod))].into();
    with_mod_factories(mods, |game| {
        let stick = ItemStack::new("minecraft:stick", 1);
        let apple = ItemStack::new("minecraft:apple", 1);
        game.set_screen(Screen::Inventory);
        game.inventory[9] = Some(stick.clone());
        game.inventory[10] = Some(apple.clone());

        game.carried = None;
        assert!(game.tooltip_with_carried(&stick));
        assert!(game.tooltip_with_carried(&apple));

        game.carried = Some(ItemStack::new("minecraft:stone", 1));
        assert!(game.tooltip_with_carried(&stick));
        assert!(!game.tooltip_with_carried(&apple));

        let mut batch = HudBatch::scratch([512, 512]);
        let (ox, oy) = game.inventory_origin(&batch);

        let (sx, sy) = game.slot_position(9);
        game.cursor = (ox + sx + 8.0, oy + sy + 8.0);
        assert_eq!(game.slot_at(&batch, game.cursor.0, game.cursor.1), Some(9));
        game.draw_slot_tooltip(&mut batch);
        assert!(
            !batch.vertices.is_empty(),
            "show_tooltip_with_item_in_hand = true keeps slot tooltip visible while carrying"
        );

        batch.vertices.clear();
        let (ax, ay) = game.slot_position(10);
        game.cursor = (ox + ax + 8.0, oy + ay + 8.0);
        assert_eq!(game.slot_at(&batch, game.cursor.0, game.cursor.1), Some(10));
        game.draw_slot_tooltip(&mut batch);
        assert!(
            batch.vertices.is_empty(),
            "show_tooltip_with_item_in_hand = false hides slot tooltip while carrying"
        );
    });
}

/// Audit [88]: verifies no-listener fast path skips stack API conversion without cancelling,
/// conversion error in `render_tooltip_pre` skips only that hook without cancelling, and an
/// invalid individual `TooltipElement::Text` (> 512-node depth) is skipped individually while
/// valid sibling lines and components still render.
#[test]
#[ignore = "daily"]
fn audit_88_no_listener_fast_path_and_invalid_text_skipped_individually() {
    // Part 1: no listeners at all — an unconvertible stack still gathers and renders its lines.
    let empty_mods: Mods = Vec::<ModEntry>::new().into();
    with_mod_factories(empty_mods, |game| {
        let unconvertible = ItemStack::new("missing:unregistered_item", 1);
        assert!(unconvertible.try_into_api().is_err());
        let lines = vec![(sunrise_client::text::Text::literal("Plain"), u32::MAX)];
        let gathered = game
            .gather_tooltip_components(&unconvertible, lines.clone(), 10, (320.0, 240.0))
            .expect("no-listener fast path must not cancel on unconvertible stack").components;
        assert_eq!(text_lines(&gathered), ["Plain"]);

        let mut batch = HudBatch::scratch([320, 240]);
        let pre = game.tooltip_pre_components(
            &mut batch,
            Some(&unconvertible),
            &gathered,
            [10, 20],
            (320.0, 240.0), DEFAULT_FONT_ID.clone(),
        );
        assert_eq!(pre, Some(([10, 20], DEFAULT_FONT_ID.clone())));
        game.draw_stack_tooltip_lines(&mut batch, &unconvertible, lines, 10.0, 20.0);
        assert!(!batch.vertices.is_empty());
    });

    // Part 2: listeners active — unconvertible stack skips Pre without cancelling, and a
    // >512-deep TooltipElement::Text is dropped individually while valid siblings still render.
    struct SiblingData;
    impl TooltipComponentData for SiblingData {}

    struct SiblingClient;
    impl ClientTooltipComponent for SiblingClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            24
        }
        fn draw_image(
            &self,
            _: &dyn TooltipFont,
            x: i32,
            y: i32,
            _: i32,
            _: i32,
            draw: &mut dyn DrawContext,
        ) {
            draw.fill([x as f32, y as f32, 6.0, 6.0], 0xFF44_88CC);
        }
    }

    struct InvalidTextMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for InvalidTextMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            event
                .factories
                .register::<SiblingData, SiblingClient>(|_| SiblingClient);
        }
        fn render_tooltip_gather_components(
            &self,
            _: &dyn ClientHost,
            event: &mut RenderTooltipGatherComponentsEvent<'_>,
        ) -> Flow {
            let mut too_deep = mod_api::Text::literal("deep");
            for _ in 0..mod_api::text::MAX_TEXT_DEPTH {
                let mut parent = mod_api::Text::literal("");
                parent.children.push(too_deep);
                too_deep = parent;
            }
            *event.elements = vec![
                TooltipElement::Text(mod_api::Text::literal("Before"), u32::MAX),
                TooltipElement::Text(too_deep, u32::MAX),
                TooltipElement::Component(TooltipComponent::custom(SiblingData)),
                TooltipElement::Text(mod_api::Text::literal("After"), u32::MAX),
            ];
            Flow::Continue
        }
        fn render_tooltip_pre(
            &self,
            _: &dyn ClientHost,
            _: &mut RenderTooltipPreEvent<'_>,
        ) -> Flow {
            self.0.fetch_add(1, Ordering::Relaxed);
            Flow::Continue
        }
    }

    let pre_calls = Arc::new(AtomicUsize::new(0));
    let mods: Mods = vec![ModEntry::new(
        "invalid_text_mod",
        Box::new(InvalidTextMod(pre_calls.clone())),
    )]
    .into();

    with_mod_factories(mods, |game| {
        let unconvertible = ItemStack::new("missing:unregistered_item", 1);
        let one_line = vec![PreparedComponent::Text(sunrise_client::text::Text::literal("Keep"),
            u32::MAX,
        )];
        let mut pre_batch = HudBatch::scratch([320, 240]);
        let pre = game.tooltip_pre_components(
            &mut pre_batch,
            Some(&unconvertible),
            &one_line,
            [10, 20],
            (320.0, 240.0), DEFAULT_FONT_ID.clone(),
        );
        assert_eq!(pre, Some(([10, 20], DEFAULT_FONT_ID.clone())));
        assert_eq!(
            pre_calls.load(Ordering::Relaxed),
            0,
            "stack conversion failure skips Pre without cancelling"
        );

        let stick = ItemStack::new("minecraft:stick", 1);
        let gathered = game
            .gather_tooltip_components(
                &stick,
                vec![(sunrise_client::text::Text::literal("Seed"), u32::MAX)],
                10,
                (320.0, 240.0),
            )
            .expect("invalid individual text element must not cancel tooltip").components;
        assert_eq!(gathered.len(), 3);
        assert_eq!(text_lines(&gathered), ["Before", "After"]);
        assert!(gathered[1].downcast_ref::<SiblingClient>().is_some());

        let mut draw_batch = HudBatch::scratch([320, 240]);
        assert!(
            game.draw_prepared_tooltip(
                &mut draw_batch,
                Some(&stick),
                &gathered,
                [10, 20], DEFAULT_FONT_ID.clone(),
                true
            )
            .is_some()
        );
        assert_eq!(pre_calls.load(Ordering::Relaxed), 1);
        assert!(
            draw_batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0x44, 0x88, 0xCC, 0xFF)),
            "valid sibling custom component must still render"
        );
    });
}

/// Verifies panic containment across factory creation, `width`, `height`,
/// `show_tooltip_with_item_in_hand`, `draw_text`, and `draw_image`: partial `HudBatch` vertices
/// from panicking draw passes are rolled back via `paint_checkpoint().discard(batch)`, each
/// failing factory registration is disabled for subsequent tooltips, and sibling components still
/// draw.
#[test]
#[ignore = "daily"]
fn panic_containment_rolls_back_partial_paint_and_disables_only_failing_factory() {
    struct CreatePanicData;
    impl TooltipComponentData for CreatePanicData {}
    struct WidthPanicData;
    impl TooltipComponentData for WidthPanicData {}
    struct HeightPanicData;
    impl TooltipComponentData for HeightPanicData {}
    struct HandPanicData;
    impl TooltipComponentData for HandPanicData {}
    struct DrawTextPanicData;
    impl TooltipComponentData for DrawTextPanicData {}
    struct DrawImagePanicData;
    impl TooltipComponentData for DrawImagePanicData {}
    struct SiblingData;
    impl TooltipComponentData for SiblingData {}

    struct WidthPanicClient(Arc<AtomicUsize>);
    impl ClientTooltipComponent for WidthPanicClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("tooltip width panic");
        }
    }

    struct HeightPanicClient(Arc<AtomicUsize>);
    impl ClientTooltipComponent for HeightPanicClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("tooltip height panic");
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
    }

    struct HandPanicClient(Arc<AtomicUsize>);
    impl ClientTooltipComponent for HandPanicClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn show_tooltip_with_item_in_hand(&self) -> bool {
            self.0.fetch_add(1, Ordering::Relaxed);
            panic!("tooltip hand panic");
        }
    }

    struct DrawTextPanicClient(Arc<AtomicUsize>);
    impl ClientTooltipComponent for DrawTextPanicClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            20
        }
        fn draw_text(&self, _: &dyn TooltipFont, x: i32, y: i32, draw: &mut dyn DrawContext) {
            self.0.fetch_add(1, Ordering::Relaxed);
            draw.fill([x as f32, y as f32, 8.0, 8.0], 0xFFDE_AD01);
            panic!("tooltip draw_text panic");
        }
    }

    struct DrawImagePanicClient(Arc<AtomicUsize>);
    impl ClientTooltipComponent for DrawImagePanicClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            20
        }
        fn draw_image(
            &self,
            _: &dyn TooltipFont,
            x: i32,
            y: i32,
            _: i32,
            _: i32,
            draw: &mut dyn DrawContext,
        ) {
            self.0.fetch_add(1, Ordering::Relaxed);
            draw.fill([x as f32, y as f32, 8.0, 8.0], 0xFFDE_AD02);
            panic!("tooltip draw_image panic");
        }
    }

    struct SiblingClient(Arc<AtomicUsize>);
    impl ClientTooltipComponent for SiblingClient {
        fn height(&self, _: &dyn TooltipFont) -> i32 {
            10
        }
        fn width(&self, _: &dyn TooltipFont) -> i32 {
            30
        }
        fn draw_image(
            &self,
            _: &dyn TooltipFont,
            x: i32,
            y: i32,
            _: i32,
            _: i32,
            draw: &mut dyn DrawContext,
        ) {
            self.0.fetch_add(1, Ordering::Relaxed);
            draw.fill([x as f32, y as f32, 8.0, 8.0], 0xFF12_EF34);
        }
    }

    struct CreatePanicMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for CreatePanicMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let calls = self.0.clone();
            event
                .factories
                .register::<CreatePanicData, SiblingClient>(move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    panic!("tooltip factory panic");
                });
        }
    }

    struct WidthPanicMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for WidthPanicMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let calls = self.0.clone();
            event
                .factories
                .register::<WidthPanicData, WidthPanicClient>(move |_| {
                    WidthPanicClient(calls.clone())
                });
        }
    }

    struct HeightPanicMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for HeightPanicMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let calls = self.0.clone();
            event
                .factories
                .register::<HeightPanicData, HeightPanicClient>(move |_| {
                    HeightPanicClient(calls.clone())
                });
        }
    }

    struct HandPanicMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for HandPanicMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let calls = self.0.clone();
            event
                .factories
                .register::<HandPanicData, HandPanicClient>(move |_| {
                    HandPanicClient(calls.clone())
                });
        }
    }

    struct DrawTextPanicMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for DrawTextPanicMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let calls = self.0.clone();
            event
                .factories
                .register::<DrawTextPanicData, DrawTextPanicClient>(move |_| {
                    DrawTextPanicClient(calls.clone())
                });
        }
    }

    struct DrawImagePanicMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for DrawImagePanicMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let calls = self.0.clone();
            event
                .factories
                .register::<DrawImagePanicData, DrawImagePanicClient>(move |_| {
                    DrawImagePanicClient(calls.clone())
                });
        }
    }

    struct SiblingMod(Arc<AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for SiblingMod {
        fn register_client_tooltip_component_factories(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterClientTooltipComponentFactoriesEvent<'_>,
        ) {
            let calls = self.0.clone();
            event
                .factories
                .register::<SiblingData, SiblingClient>(move |_| SiblingClient(calls.clone()));
        }
    }

    let create_calls = Arc::new(AtomicUsize::new(0));
    let width_calls = Arc::new(AtomicUsize::new(0));
    let height_calls = Arc::new(AtomicUsize::new(0));
    let hand_calls = Arc::new(AtomicUsize::new(0));
    let text_calls = Arc::new(AtomicUsize::new(0));
    let image_calls = Arc::new(AtomicUsize::new(0));
    let sibling_calls = Arc::new(AtomicUsize::new(0));

    let mods: Mods = vec![
        ModEntry::new(
            "panic_create",
            Box::new(CreatePanicMod(create_calls.clone())),
        ),
        ModEntry::new("panic_width", Box::new(WidthPanicMod(width_calls.clone()))),
        ModEntry::new(
            "panic_height",
            Box::new(HeightPanicMod(height_calls.clone())),
        ),
        ModEntry::new("panic_hand", Box::new(HandPanicMod(hand_calls.clone()))),
        ModEntry::new(
            "panic_draw_text",
            Box::new(DrawTextPanicMod(text_calls.clone())),
        ),
        ModEntry::new(
            "panic_draw_image",
            Box::new(DrawImagePanicMod(image_calls.clone())),
        ),
        ModEntry::new(
            "healthy_sibling",
            Box::new(SiblingMod(sibling_calls.clone())),
        ),
    ]
    .into();

    with_mod_factories(mods, |game| {
        let font = crate::game::host::draw::EngineTooltipFont {
            font: &game.gui.atlas.font,
            id: &DEFAULT_FONT_ID,
        };

        // 1. Factory creation panic: caught and disabled on subsequent tooltips.
        assert!(
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(CreatePanicData)).unwrap()
                .is_none()
        );
        assert_eq!(create_calls.load(Ordering::Relaxed), 1);
        assert!(
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(CreatePanicData)).unwrap()
                .is_none()
        );
        assert_eq!(create_calls.load(Ordering::Relaxed), 1);

        // 2. width panic: caught, returns 0, disables factory for subsequent tooltips.
        let width_comp = sunrise_client::tooltip_components::create(&TooltipComponent::custom(WidthPanicData)).unwrap()
            .unwrap();
        assert_eq!(width_comp.width(&font), 0);
        assert_eq!(width_calls.load(Ordering::Relaxed), 1);
        assert!(
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(WidthPanicData)).unwrap()
                .is_none()
        );

        // 3. height panic: caught, returns 0, disables factory for subsequent tooltips.
        let height_comp = sunrise_client::tooltip_components::create(&TooltipComponent::custom(HeightPanicData)).unwrap()
            .unwrap();
        assert_eq!(height_comp.height(&font), 0);
        assert_eq!(height_calls.load(Ordering::Relaxed), 1);
        assert!(
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(HeightPanicData)).unwrap()
                .is_none()
        );

        // 4. show_tooltip_with_item_in_hand panic: caught, returns false, disables factory.
        let hand_comp = sunrise_client::tooltip_components::create(&TooltipComponent::custom(HandPanicData)).unwrap()
            .unwrap();
        assert!(!hand_comp.show_tooltip_with_item_in_hand());
        assert_eq!(hand_calls.load(Ordering::Relaxed), 1);
        assert!(
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(HandPanicData)).unwrap()
                .is_none()
        );

        // 5 & 6. draw_text and draw_image panics: partial HudBatch vertices rolled back via
        // paint_checkpoint().discard(batch), failing registrations disabled, sibling still draws.
        let components = vec![
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(DrawTextPanicData)).unwrap()
                .unwrap(),
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(DrawImagePanicData)).unwrap()
                .unwrap(),
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(SiblingData)).unwrap()
                .unwrap(),
        ];
        let mut batch = HudBatch::scratch([320, 240]);
        assert!(
            game.draw_prepared_tooltip(&mut batch, None, &components, [10, 20], DEFAULT_FONT_ID.clone(), true)
                .is_some()
        );
        assert_eq!(text_calls.load(Ordering::Relaxed), 1);
        assert_eq!(image_calls.load(Ordering::Relaxed), 1);
        assert_eq!(sibling_calls.load(Ordering::Relaxed), 1);
        assert!(
            !batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0xDE, 0xAD, 0x01, 0xFF)),
            "partial draw_text paint before panic must be rolled back"
        );
        assert!(
            !batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0xDE, 0xAD, 0x02, 0xFF)),
            "partial draw_image paint before panic must be rolled back"
        );
        assert!(
            batch
                .vertices
                .iter()
                .any(|v| v.color == rgba(0x12, 0xEF, 0x34, 0xFF)),
            "healthy sibling component must still draw"
        );

        // Subsequent tooltips skip disabled factories and never re-run panicking callbacks, while
        // the healthy sibling keeps preparing and drawing normally.
        assert!(
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(DrawTextPanicData)).unwrap()
                .is_none()
        );
        assert!(
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(DrawImagePanicData)).unwrap()
                .is_none()
        );
        let mut second_batch = HudBatch::scratch([320, 240]);
        assert!(
            game.draw_prepared_tooltip(&mut second_batch, None, &components, [10, 20], DEFAULT_FONT_ID.clone(), true)
                .is_some()
        );
        assert_eq!(text_calls.load(Ordering::Relaxed), 1);
        assert_eq!(image_calls.load(Ordering::Relaxed), 1);
        assert_eq!(sibling_calls.load(Ordering::Relaxed), 2);
        assert!(
            sunrise_client::tooltip_components::create(&TooltipComponent::custom(SiblingData)).unwrap()
                .is_some()
        );
    });
}
