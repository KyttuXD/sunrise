//! A live isolated cancel dispatch accepts cancellation on return and never clears it.

#[test]
fn a_listener_receiving_a_cancellation_cannot_clear_it() {
    struct Cancel;
    #[mod_api::__subscriptions]
    impl Hooks for Cancel {
        fn living_travel(&self, _: &dyn LoopContext, _: &mut LivingTravelEvent<'_>) -> Flow {
            Flow::Cancel
        }
    }

    struct Clear(Arc<Mutex<Vec<bool>>>);
    #[mod_api::__subscriptions]
    impl Hooks for Clear {
        #[listener(receive_cancelled)]
        fn living_travel(&self, _: &dyn LoopContext, event: &mut LivingTravelEvent<'_>) -> Flow {
            self.0.lock().unwrap().push(event.cancelled);
            event.cancelled = false;
            Flow::Continue
        }
    }

    struct Normal(Arc<Mutex<Vec<bool>>>);
    #[mod_api::__subscriptions]
    impl Hooks for Normal {
        fn living_travel(&self, _: &dyn LoopContext, event: &mut LivingTravelEvent<'_>) -> Flow {
            self.0.lock().unwrap().push(event.cancelled);
            Flow::Continue
        }
    }

    let heard = Arc::new(Mutex::new(Vec::new()));
    let skipped = Arc::new(Mutex::new(Vec::new()));
    install(
        vec![
            ModEntry::new("first", Box::new(Cancel)),
            ModEntry::new("clear", Box::new(Clear(Arc::clone(&heard)))),
            ModEntry::new("normal", Box::new(Normal(Arc::clone(&skipped)))),
        ]
        .into(),
    );
    let mut canceled = false;
    mod_api::dispatch_living_travel(
        &Loop("server"),
        7,
        "minecraft:zombie",
        TravelMode::Air,
        TravelMotion::default(),
        &mut canceled,
    );
    assert!(canceled);
    assert_eq!(*heard.lock().unwrap(), [true]);
    assert!(skipped.lock().unwrap().is_empty());
}

mod root {
    mod callback_owner {
        pub(super) fn contain_unwind<T>(call: impl FnOnce() -> T) -> Result<T, String> {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(call))
                .map_err(|_| "fixture callback panicked".into())
        }
    }
}
