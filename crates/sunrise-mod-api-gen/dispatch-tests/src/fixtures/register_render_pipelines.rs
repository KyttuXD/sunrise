mod api {
    #[derive(Clone)]
    pub struct RenderPipeline;
}
mod crate_root {
    // Dispatch fixtures isolate ordering; API tests cover pipeline validation.
    mod client {
        pub(crate) mod camera {
            pub(crate) fn refused_angles_output(
                _: &f32,
                _: &f32,
                _: &f32,
            ) -> Result<(), &'static str> {
                Ok(())
            }

            pub(crate) fn refused_position_output(_: &[f64; 3]) -> Result<(), &'static str> {
                Ok(())
            }
        }

        pub(crate) mod render_pipeline {
            pub(crate) fn validate_render_pipelines<T>(_: &str, _: &[T], _: &mut Vec<T>) {}
        }
    }
}
