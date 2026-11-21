//! Append native geometry to the section's own terrain buffers, after its blocks.
use super::{Mesher, SectionMesh, TerrainVertex, Tints, mod_tint::Region};
use crate::connected::TileLayer;
use mod_api::__private::game::CallbackOwner;
use mod_api::{
    BlockAndTintGetter, SectionGeometry, SectionGeometryContext, SectionQuad, SectionQuadError,
};
use std::{
    cell::RefCell,
    sync::{Arc, OnceLock},
};

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
    origin: [i32; 3],
    mod_id: &'static str,
}

impl SectionGeometryContext for Collector<'_, '_> {
    fn level(&self) -> &dyn BlockAndTintGetter {
        self.level
    }

    fn add(&mut self, sprite: &str, quad: SectionQuad) -> Result<(), SectionQuadError> {
        let error = if !quad.positions.iter().flatten().all(|v| v.is_finite()) {
            Some(SectionQuadError::NonFinitePosition)
        } else if !quad.uvs.iter().flatten().all(|v| v.is_finite()) {
            Some(SectionQuadError::NonFiniteUv)
        } else {
            None
        };
        if let Some(error) = error {
            log::warn!(
                "mod {}: section geometry at {:?} refused a quad: {error}",
                self.mod_id,
                self.origin,
            );
            return Err(error);
        }

        let sprites = &self.mesher.render.sprite_layers;
        let layer = sprites
            .get(sprite)
            .copied()
            .unwrap_or_else(|| sprites["minecraft:missingno"]) as u16;
        let [a, r, g, b] = quad.color.to_be_bytes().map(u32::from);
        let color = r | g << 8 | b << 16 | a << 24;
        let [sky, block] = quad.light.map(|l| u16::from(l.min(15)) * 16);
        let light = sky | block << 8;
        let vertices = std::array::from_fn(|i| {
            TerrainVertex::new(quad.positions[i], quad.uvs[i], 0, color, light)
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
        Ok(())
    }
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
    for renderer in mesher.tables.section_geometry.iter() {
        if !renderer.owner.is_enabled() {
            continue;
        }
        let mut collector = Collector {
            level: &region,
            mesher,
            out,
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
