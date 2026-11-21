mod stubs {
    #[derive(Clone, Copy)]
    pub(super) struct CommandTreeFilter<'a>(pub(super) &'a [&'a str]);

    pub(super) struct CommandTreeSelection;
}

mod root {
    mod callback_owner {
        // The real runtime also reports a panic to the error-toast queue. Fixtures exercise
        // the dispatch's answer and poison policy, with no game UI.
        pub(super) fn contain_listener<T>(
            _: &'static str,
            _: &'static str,
            invoke: impl FnOnce() -> T,
        ) -> Result<T, ()> {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(invoke)).map_err(|payload| {
                if let Err(payload) =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)))
                {
                    std::mem::forget(payload);
                }
            })
        }
    }
}
