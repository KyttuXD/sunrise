mod api {
    pub struct Pose;
}

#[test]
fn entity_size_collects_dimension_edits_in_listener_order() {
    struct Width;
    #[mod_api::__subscriptions]
    impl Hooks for Width {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            assert_eq!(event.old_size, [0.6, 1.8, 1.62]);
            event.size[0] = 2.0;
            event.size[1] = 3.0;
        }
    }
    struct Eyes;
    #[mod_api::__subscriptions]
    impl Hooks for Eyes {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            assert_eq!(*event.size, [2.0, 3.0, 1.27]);
            event.size[2] = 2.5;
        }
    }
    install(
        vec![
            ModEntry::new("width", Box::new(Width)),
            ModEntry::new("eyes", Box::new(Eyes)),
        ]
        .into(),
    );
    let mut size = [0.6, 1.5, 1.27];
    dispatch_entity_size(
        &TestHost,
        EntityRef,
        generated::mod_api::Pose,
        [0.6, 1.8, 1.62],
        &mut size,
    );
    assert_eq!(size, [2.0, 3.0, 2.5]);
}

#[test]
fn entity_size_without_listeners_preserves_float_bits() {
    install(Arc::from([]));
    let mut size = [0.6_f32 * 0.1, 1.5 * 0.1, 1.27 * 0.1];
    let expected = size.map(f32::to_bits);
    dispatch_entity_size(
        &TestHost,
        EntityRef,
        generated::mod_api::Pose,
        [0.6, 1.8, 1.62],
        &mut size,
    );
    assert_eq!(size.map(f32::to_bits), expected);
}

#[test]
fn an_invalid_size_or_panic_never_reaches_the_next_listener() {
    struct Wide;
    #[mod_api::__subscriptions]
    impl Hooks for Wide {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            event.size[0] = 96.0;
        }
    }
    struct NoHeight;
    #[mod_api::__subscriptions]
    impl Hooks for NoHeight {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            event.size[1] = f32::NAN;
        }
    }
    struct Panics;
    #[mod_api::__subscriptions]
    impl Hooks for Panics {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            event.size[1] = 777.0;
            panic!("size callback panicked after editing its proposal");
        }
    }
    struct Final;
    #[mod_api::__subscriptions]
    impl Hooks for Final {
        fn entity_size(&self, _: &dyn Host, event: &mut EntitySizeEvent<'_>) {
            if event.size[1] == 1.5 {
                event.size[2] = 1.0;
            }
        }
    }
    install(
        vec![
            ModEntry::new("wide", Box::new(Wide)),
            ModEntry::new("no_height", Box::new(NoHeight)),
            ModEntry::new("panicking_size", Box::new(Panics)),
            ModEntry::new("final_size", Box::new(Final)),
        ]
        .into(),
    );
    for _ in 0..2 {
        let mut size = [0.6, 1.5, 1.27];
        dispatch_entity_size(
            &TestHost,
            EntityRef,
            generated::mod_api::Pose,
            size,
            &mut size,
        );
        assert_eq!(size, [96.0, 1.5, 1.0]);
        assert!(runtime::poisoned(HookId::EntitySize, "panicking_size"));
        assert!(!runtime::poisoned(HookId::EntitySize, "no_height"));
    }
}
