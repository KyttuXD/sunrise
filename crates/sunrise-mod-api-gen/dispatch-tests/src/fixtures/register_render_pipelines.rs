mod api {
    #[derive(Clone)]
    pub struct RenderPipeline;
}
mod crate_root {
    // Dispatch fixtures isolate ordering; API tests cover pipeline validation.
    mod client {
        pub(crate) mod camera {
            pub(crate) fn validate_angles(_: &[f32; 3], _: &[f32; 3]) -> Result<(), &'static str> {
                Ok(())
            }
            pub(crate) fn validate_position(
                _: &[f64; 3],
                _: &[f64; 3],
            ) -> Result<(), &'static str> {
                Ok(())
            }
            pub(crate) fn validate_transform(
                _: &[f32; 16],
                _: &[f32; 16],
            ) -> Result<(), &'static str> {
                Ok(())
            }
        }
        pub(crate) mod render_pipeline {
            pub(crate) fn validate_render_pipelines<T>(_: &str, _: &[T], _: &mut Vec<T>) {}
        }
    }
}
