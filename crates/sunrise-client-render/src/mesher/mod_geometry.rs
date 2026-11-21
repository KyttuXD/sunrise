//! Append native geometry to the section's own terrain buffers, after its blocks.
use super::{Mesher, OPAQUE_LAYER, SectionMesh, TerrainVertex, Tints, mod_tint::Region};
use crate::connected::TileLayer;
use mod_api::__private::game::CallbackOwner;
use mod_api::{
    BlockAndTintGetter, SectionGeometry, SectionGeometryContext, SectionQuad, SectionQuadError,
};
use std::{
    cell::RefCell,
    sync::{Arc, OnceLock},
};
use sunrise_assets::item_bake::MISSING_SPRITE;

/// Vanilla `BufferBuilder.beginVertex` stops after this many vertices in one layer.
const MAX_LAYER_VERTICES: usize = 16_777_215;

/// One retained callback with its registering mod's session-scoped panic policy.
pub(super) struct Renderer {
    owner: CallbackOwner,
    renderer: Arc<dyn SectionGeometry>,
}

impl Renderer {
    pub(super) fn new(mod_id: &'static str, renderer: Arc<dyn SectionGeometry>) -> Self {
        Self {
            owner: CallbackOwner::for_mod(mod_id),
            renderer,
        }
    }
}

/// The renderers, in registration order: set once at start, read by every table build.
static REGISTERED: OnceLock<Arc<[Renderer]>> = OnceLock::new();

/// Sets the native mods' section geometry renderers before the first resource load. False when
/// already set.
pub fn register_section_geometry(renderers: Vec<(&'static str, Arc<dyn SectionGeometry>)>) -> bool {
    REGISTERED
        .set(
            renderers
                .into_iter()
                .map(|(id, renderer)| Renderer::new(id, renderer))
                .collect(),
        )
        .is_ok()
}

/// Whether any mod renderer is registered, so the game prepares the block values its `level`
/// reads.
pub fn has_section_geometry() -> bool {
    REGISTERED.get().is_some_and(|r| !r.is_empty())
}

/// What [`register_section_geometry`] set; none before.
pub(super) fn registered() -> Arc<[Renderer]> {
    REGISTERED.get().cloned().unwrap_or_else(|| Arc::new([]))
}

/// One section's checked submission boundary. Each successful call appends a complete quad;
/// there is no batch to discard when a later call fails.
struct Collector<'borrow, 'assets> {
    level: &'borrow dyn BlockAndTintGetter,
    mesher: &'borrow Mesher<'assets>,
    out: &'borrow mut SectionMesh,
    cutout_vertices: &'borrow mut usize,
    origin: [i32; 3],
    mod_id: &'static str,
}

impl SectionGeometryContext for Collector<'_, '_> {
    fn level(&self) -> &dyn BlockAndTintGetter {
        self.level
    }

    fn add(&mut self, sprite: &str, quad: SectionQuad) -> Result<(), SectionQuadError> {
        let vertices = if quad.translucent {
            self.out.translucent.len()
        } else {
            *self.cutout_vertices
        };
        let layer = check_quad(&quad, vertices).and_then(|()| {
            let sprites = &self.mesher.render.sprite_layers;
            sprites
                .get(sprite)
                .or_else(|| sprites.get(MISSING_SPRITE))
                .copied()
                .ok_or(SectionQuadError::BlockAtlasUnavailable)
        });
        let layer = match layer {
            Ok(layer) => layer as u16, // MeshTables validates the atlas layer count at load.
            Err(error) => {
                log::warn!(
                    "mod {}: section geometry at {:?} refused a quad: {error}",
                    self.mod_id,
                    self.origin,
                );
                return Err(error);
            }
        };
        let [a, r, g, b] = quad.color.to_be_bytes().map(u32::from);
        let color = r | g << 8 | b << 16 | a << 24;
        // Keep the terrain vertex's existing encoding of the API's 0–15 light levels.
        let [sky, block] = quad.light.map(|l| u16::from(l.min(15)) * 16);
        let light = sky | block << 8;
        // BufferBuilder writes the supplied position and UV floats, not the prepacked UVs
        // used for baked models. The mesh codec spills any quad it cannot reproduce exactly.
        let vertices = std::array::from_fn(|i| TerrainVertex {
            pos: quad.positions[i],
            uv: quad.uvs[i],
            layer,
            color,
            light,
        });
        self.mesher.quad_as(
            self.out,
            false,
            TileLayer::Layer {
                layer,
                translucent: quad.translucent,
                opaque: false,
            },
            vertices,
            false,
            Some(quad.color & 0xFF_FFFF),
        );
        if !quad.translucent {
            *self.cutout_vertices += vertices.len();
        }
        Ok(())
    }
}

fn check_quad(quad: &SectionQuad, vertices: usize) -> Result<(), SectionQuadError> {
    if !quad.positions.iter().flatten().all(|v| v.is_finite()) {
        return Err(SectionQuadError::NonFinitePosition);
    }
    if !quad.uvs.iter().flatten().all(|v| v.is_finite()) {
        return Err(SectionQuadError::NonFiniteUv);
    }
    if vertices > MAX_LAYER_VERTICES - quad.positions.len() {
        return Err(SectionQuadError::VertexCount);
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn one_more_quad_requires_four_available_vertices_in_its_layer() {
    let quad = SectionQuad {
        positions: [[0.0; 3]; 4],
        uvs: [[0.0; 2]; 4],
        color: 0xFFFF_FFFF,
        light: [15, 0],
        translucent: false,
    };
    assert_eq!(check_quad(&quad, 16_777_208), Ok(()));
    assert_eq!(
        check_quad(&quad, 16_777_212),
        Err(SectionQuadError::VertexCount)
    );
}

/// Ask the registered renderers to add to this section in order. A panic disables only its
/// callback. All previously completed submissions stay in the section, even across a panic.
pub(super) fn add(mesher: &Mesher<'_>, tints: &mut Tints<'_>, out: &mut SectionMesh) {
    let [cx, cz, base_y] = tints.origin;
    let origin = [cx * 16, base_y, cz * 16];
    let region = Region {
        tables: mesher.tables,
        tints: RefCell::new(tints),
        nether: mesher.nether_shading,
    };
    // The engine stores solid terrain and cutout in the same mesh vector. Only cutout shares
    // the layer that non-translucent submissions append to.
    let mut cutout_vertices = out
        .solid
        .iter()
        .step_by(4)
        .filter(|v| v.layer & OPAQUE_LAYER == 0)
        .count()
        * 4;
    for renderer in mesher.tables.section_geometry.iter() {
        if !renderer.owner.is_enabled() {
            continue;
        }
        let mut collector = Collector {
            level: &region,
            mesher,
            out,
            cutout_vertices: &mut cutout_vertices,
            origin,
            mod_id: renderer.owner.id(),
        };
        let _ = renderer
            .owner
            .call("section geometry", "add_section_geometry", || {
                renderer.renderer.add(origin, &mut collector);
            });
    }
}
