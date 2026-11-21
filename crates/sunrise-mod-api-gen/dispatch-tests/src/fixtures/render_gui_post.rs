mod stubs {
    pub(super) struct GuiLayerBounds {
        pub layer: &'static str,
        pub rect: [f32; 4],
    }
}

#[test]
fn post_hud_dispatch_shares_geometry_without_reordering_draws() {
    struct Overlay(u32);
    #[mod_api::__subscriptions]
    impl Hooks for Overlay {
        fn render_gui_post(&self, _: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
            assert_eq!(event.layers[0].layer, "minecraft:effects");
            assert_eq!(event.layers[0].rect, [295.0, 1.0, 24.0, 24.0]);
            event.draw.mark(self.0);
        }
    }
    install(
        vec![
            ModEntry::new("first", Box::new(Overlay(1))),
            ModEntry::new("second", Box::new(Overlay(2))),
        ]
        .into(),
    );
    let layers = [GuiLayerBounds {
        layer: "minecraft:effects",
        rect: [295.0, 1.0, 24.0, 24.0],
    }];
    let mut batch = HudBatch(vec![99]);
    dispatch_render_gui_post(&TestHost, &mut batch, &layers);
    assert_eq!(batch.0, [99, 1, 2]);
}
