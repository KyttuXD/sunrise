mod crate_root {
    mod callback_owner {
        pub(crate) fn contain_unwind<T>(call: impl FnOnce() -> T) -> Result<T, String> {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(call))
                .map_err(|_| "fixture callback panic".to_owned())
        }

        pub(crate) fn report_hook_panic(_: &str, _: &str, _: String) {}
    }
}

#[test]
fn an_answer_names_the_mod_that_gave_it() {
    struct Passer;
    #[mod_api::__subscriptions]
    impl Hooks for Passer {
        fn screen_opening(
            &self,
            _: &dyn ClientHost,
            _: &mut ScreenOpeningEvent<'_>,
        ) -> Option<ScreenOpening> {
            None
        }
    }
    struct Opener;
    #[mod_api::__subscriptions]
    impl Hooks for Opener {
        fn screen_opening(
            &self,
            _: &dyn ClientHost,
            _: &mut ScreenOpeningEvent<'_>,
        ) -> Option<ScreenOpening> {
            Some(ScreenOpening::ModScreen)
        }
    }
    let mods: Vec<ModEntry> = vec![
        ModEntry::new("passer", Box::new(Passer)),
        ModEntry::new("opener", Box::new(Opener)),
        ModEntry::new("late", Box::new(Opener)),
    ];
    install(mods.into());
    let view = ScreenView(&());
    let mut outcome = None;
    dispatch_screen_opening(&TestHost, &view, &view, &mut outcome);
    assert_eq!(
        outcome,
        Some(Answered {
            answer: ScreenOpening::ModScreen,
            mod_id: "opener"
        })
    );
}

#[test]
fn a_panicking_screen_opener_does_not_hide_the_next_answer() {
    struct Panics(Arc<std::sync::atomic::AtomicUsize>);
    #[mod_api::__subscriptions]
    impl Hooks for Panics {
        fn screen_opening(
            &self,
            _: &dyn ClientHost,
            _: &mut ScreenOpeningEvent<'_>,
        ) -> Option<ScreenOpening> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            panic!("opening callback failed");
        }
    }

    struct Opens;
    #[mod_api::__subscriptions]
    impl Hooks for Opens {
        fn screen_opening(
            &self,
            _: &dyn ClientHost,
            _: &mut ScreenOpeningEvent<'_>,
        ) -> Option<ScreenOpening> {
            Some(ScreenOpening::ModScreen)
        }
    }

    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    install(
        vec![
            ModEntry::new("panics", Box::new(Panics(calls.clone()))),
            ModEntry::new("opens", Box::new(Opens)),
        ]
        .into(),
    );
    let view = ScreenView(&());
    for _ in 0..2 {
        let mut outcome = None;
        dispatch_screen_opening(&TestHost, &view, &view, &mut outcome);
        assert_eq!(
            outcome,
            Some(Answered {
                answer: ScreenOpening::ModScreen,
                mod_id: "opens",
            })
        );
    }
    assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
}
