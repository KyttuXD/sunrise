//! Native geometry appended to a chunk section's mesh by `add_section_geometry`.
use super::BlockAndTintGetter;
use std::sync::Arc;

/// A mod's extra section geometry: NeoForge's `AdditionalSectionRenderer`.
pub trait SectionGeometry: Send + Sync + 'static {
    /// Adds quads through `context`, relative to the section's lowest block, `origin`.
    /// Called on the meshing workers on each rebuild, including all-air sections. Callbacks
    /// may run concurrently. A panic disables this callback for the session and reports the mod;
    /// completed submissions stay in the current mesh.
    fn add(&self, origin: [i32; 3], context: &mut dyn SectionGeometryContext);
}

/// The section collector, corresponding to NeoForge's `SectionRenderingContext`.
pub trait SectionGeometryContext {
    /// Reads the section's chunk and its eight neighbours.
    fn level(&self) -> &dyn BlockAndTintGetter;
    /// Appends one quad textured with the named block atlas sprite (e.g. `minecraft:block/stone`).
    /// An absent sprite uses the atlas's missing sprite. A refusal is reported and skips only
    /// this quad: earlier submissions remain, and later calls can still succeed.
    fn add(&mut self, sprite: &str, quad: SectionQuad) -> Result<(), SectionQuadError>;
}

/// Why a section's collector refused one submitted quad.
// no wire: a local section mesher's submission result, not serialized.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionQuadError {
    /// At least one position coordinate is NaN or infinite.
    NonFinitePosition,
    /// At least one UV coordinate is NaN or infinite.
    NonFiniteUv,
    /// The render layer's vertex buffer cannot take another full quad (vanilla's
    /// `BufferBuilder.beginVertex` limit).
    VertexCount,
    /// The block atlas has no initialized missing sprite.
    BlockAtlasUnavailable,
}

impl std::fmt::Display for SectionQuadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonFinitePosition => "non-finite position",
            Self::NonFiniteUv => "non-finite UV",
            Self::VertexCount => "vertex count exceeds vanilla BufferBuilder's limit",
            Self::BlockAtlasUnavailable => "block atlas has no missing sprite",
        })
    }
}

impl std::error::Error for SectionQuadError {}

/// One quad a [`SectionGeometry`] adds. Its corners go counter-clockwise when seen from the
/// side it faces; a solid quad is back-face culled, like a block's faces.
// no wire: quads go directly from a native callback into the local section mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
// exhaustive: mods build quads with struct literals
pub struct SectionQuad {
    /// Corners in blocks relative to the section's lowest corner. Finite positions may extend
    /// beyond the section, but the quad is still rebuilt and unloaded with this section.
    pub positions: [[f32; 3]; 4],
    /// Finite coordinates relative to the sprite. Zero and one mark its opposite edges;
    /// coordinates are not restricted to that interval or rounded to model UV steps.
    pub uvs: [[f32; 2]; 4],
    /// ARGB the sprite's texels are multiplied by; `0xFFFFFFFF` leaves them as they are.
    pub color: u32,
    /// Sky and block light, 0 to 15 each (`BlockAndTintGetter::brightness` reads the world's).
    pub light: [u8; 2],
    /// Drawn in the translucent pass (sorted, blended) rather than the solid one (alpha cutout).
    pub translucent: bool,
}

/// The renderer registry `add_section_geometry` hands out.
pub trait SectionGeometryRenderers {
    /// `addRenderer(renderer)`: `renderer` adds its quads to every section the client meshes, in
    /// registration order after the section's own blocks.
    fn register(&mut self, renderer: Arc<dyn SectionGeometry>);
}

/// Engine-owned registrations, in dispatch order.
#[derive(Default)]
// exhaustive: its field is private; the game builds it
// no wire: process-local renderer storage; renderers run on the meshing workers, not serialized.
pub struct SectionGeometryRegistry {
    renderers: Vec<(&'static str, Arc<dyn SectionGeometry>)>,
}

impl SectionGeometryRenderers for SectionGeometryRegistry {
    fn register(&mut self, renderer: Arc<dyn SectionGeometry>) {
        // The private registry is provided only by the generated registration dispatch.
        let owner = crate::content::crash::current_native_mod()
            .expect("section renderer registration has a dispatch owner");
        self.renderers.push((owner, renderer));
    }
}

impl SectionGeometryRegistry {
    /// The callbacks and their captured owners, in registration order.
    pub fn into_renderers(self) -> Vec<(&'static str, Arc<dyn SectionGeometry>)> {
        self.renderers
    }
}
