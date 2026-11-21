mod section_geometry;
pub use section_geometry::{
    SectionGeometry, SectionGeometryContext, SectionGeometryRenderers, SectionQuad,
    SectionQuadError,
};
mod __private {
    mod game {
        pub use crate::section_geometry::SectionGeometryRegistry;
    }
}
