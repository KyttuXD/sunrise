//! Extra geometry in chunk section meshes, as a client mod registers it in `add_section_geometry`
//! (NeoForge's `AddSectionGeometryEvent` and its `AdditionalSectionGeometryRenderer`).
//!
//! A mod's renderer is held by the game and asked on the meshing workers each time a section
//! meshes, like a block tint source: it gets no `Host` and keeps no mutable state, since two
//! workers can mesh neighbouring sections at once. The quads it adds are part of the section's
//! mesh, so they are rebuilt with it, never added twice, and gone when the section unloads.
use super::BlockAndTintGetter;
use std::sync::Arc;

/// A mod's extra section geometry: NeoForge's `AdditionalSectionGeometryRenderer`.
pub trait SectionGeometry: Send + Sync + 'static {
    /// `render(context)`: adds the quads of the section whose lowest corner is block `origin` (a
    /// multiple of 16 on each axis) through `context`. Asked on a meshing worker every time that
    /// section meshes, also when it holds only air; a renderer with nothing for the section adds
    /// nothing. Accepted quads stay in the section mesh even if another quad is refused (see
    /// [`SectionGeometryContext::add`]). A renderer that panics rolls back only its own quads for
    /// that section, reports an error naming the mod, and is disabled for the session.
    fn add(&self, origin: [i32; 3], context: &mut dyn SectionGeometryContext);
}

/// One section a [`SectionGeometry`] adds quads to.
///
/// Matches vanilla's `BufferBuilder` and `TextureAtlas.getSprite`: positions and UVs may be any
/// finite coordinates, an unknown sprite draws with `minecraft:missingno`, and each render pass
/// (solid or translucent) accepts up to vanilla's `BufferBuilder.MAX_VERTEX_COUNT` (16,777,215)
/// vertices across the section's blocks and renderers.
pub trait SectionGeometryContext {
    /// Reads the section's chunk and its eight neighbours.
    fn level(&self) -> &dyn BlockAndTintGetter;
    /// Adds `quad`, drawn with the block atlas sprite `sprite` (`minecraft:block/stone`, a mod's
    /// `mymod:block/glow`). Unknown sprites fall back to `minecraft:missingno`, matching
    /// `TextureAtlas.getSprite`. Returns `false` and logs a warning only for the rejected quad
    /// when a position or UV is non-finite or the target render pass is at vanilla's
    /// `BufferBuilder.MAX_VERTEX_COUNT` limit; earlier accepted quads stay and later valid quads
    /// still succeed.
    fn add(&mut self, sprite: &str, quad: SectionQuad) -> bool;
}

/// One quad a [`SectionGeometry`] adds. Its corners go counter-clockwise seen from the side it
/// faces; a solid quad is culled from behind, as a block's faces are, so a quad seen from both
/// sides is two quads.
#[derive(Clone, Copy, Debug, PartialEq)]
// exhaustive: mods build quads with struct literals
// no wire: in-process quad submitted to a section geometry collector during chunk meshing.
pub struct SectionQuad {
    /// The corners, in blocks from the section's lowest corner; any finite coordinates are
    /// accepted.
    pub positions: [[f32; 3]; 4],
    /// Each corner's texture coordinate within the sprite; any finite coordinates are accepted.
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

/// The game's [`SectionGeometryRenderers`]: the renderers and their registering mod ids, in
/// registration order.
#[derive(Default)]
// exhaustive: its field is private; the game builds it
// no wire: process-local renderer storage; renderers run on the meshing workers, not serialized.
pub struct SectionGeometryRegistry {
    renderers: Vec<(Option<&'static str>, Arc<dyn SectionGeometry>)>,
}

impl SectionGeometryRenderers for SectionGeometryRegistry {
    fn register(&mut self, renderer: Arc<dyn SectionGeometry>) {
        let owner = crate::content::crash::current_native_mod();
        self.renderers.push((owner, renderer));
    }
}

impl SectionGeometryRegistry {
    /// The renderers with the registering mod id, in registration order.
    pub fn into_renderers(self) -> Vec<(Option<&'static str>, Arc<dyn SectionGeometry>)> {
        self.renderers
    }
}
