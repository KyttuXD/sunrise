mod api {
    #[derive(Debug, PartialEq, Eq)]
    pub struct RenderPipeline(pub &'static str);
}

#[test]
fn attributed_append_discards_a_panicking_listeners_tail_and_skips_it_next_time() {
    struct Listener(bool);
    #[mod_api::__subscriptions]
    impl Hooks for Listener {
        fn register_render_pipelines(
            &self,
            _: &RegistrationContext,
            event: &mut RegisterRenderPipelinesEvent<'_>,
        ) {
            event
                .pipelines
                .push(mod_api::mod_api::RenderPipeline(event.mod_id));
            assert!(!self.0, "fixture registration panic");
        }
    }

    install(
        vec![
            ModEntry::new("first", Box::new(Listener(false))),
            ModEntry::new("panicking", Box::new(Listener(true))),
            ModEntry::new("last", Box::new(Listener(false))),
        ]
        .into(),
    );
    ENTERED.with(|entered| entered.borrow_mut().clear());
    for _ in 0..2 {
        let mut pipelines = Vec::new();
        dispatch_register_render_pipelines(&RegistrationContext("bootstrap"), &mut pipelines);
        assert_eq!(
            pipelines,
            [
                mod_api::mod_api::RenderPipeline("first"),
                mod_api::mod_api::RenderPipeline("last"),
            ]
        );
    }
    ENTERED.with(|entered| {
        assert_eq!(
            entered
                .borrow()
                .iter()
                .map(|(index, _)| *index)
                .collect::<Vec<_>>(),
            [0, 1, 2, 0, 2],
        );
    });
}
