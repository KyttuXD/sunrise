mod root {
    // The fixture dispatches against stand-ins for the game vocabulary.
    mod client {
        pub(super) mod camera {
            pub(in crate::tests) fn validate_turn_delta(
                yaw: &f64,
                pitch: &f64,
            ) -> Result<(), &'static str> {
                [*yaw as f32, *pitch as f32]
                    .into_iter()
                    .all(f32::is_finite)
                    .then_some(())
                    .ok_or("non-finite delta")
            }
        }
    }
}
