//! Section geometry native mods register (`add_section_geometry`, NeoForge's
//! `AddSectionGeometryEvent` / `SectionCompiler` `ClientHooks.addAdditionalGeometry`): each
//! renderer is asked on the meshing workers every time a section meshes, and its quads go into
//! that section's own mesh through a checked collector, so they follow its rebuilds and unload
//! with it.
use super::{Mesher, OPAQUE_LAYER, SectionMesh, TerrainVertex, Tints, mod_tint::Region, packed};
use mod_api::{
    __private::game::CallbackOwner, BlockAndTintGetter, SectionGeometry, SectionGeometryContext,
    SectionQuad,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

/// Vanilla `BufferBuilder.MAX_VERTEX_COUNT`: the most vertices one render pass buffer can hold.
pub(super) const MAX_LAYER_VERTICES: usize = 16_777_215;

/// Vanilla `MissingTextureAtlasSprite.getLocation()`: fallback sprite when `TextureAtlas.getSprite`
/// is asked for an unknown sprite name.
pub(super) const MISSING_SPRITE: &str = "minecraft:missingno";

/// One mod renderer and its callback owner (enablement, attribution, and panic disablement).
pub(super) struct Renderer {
    owner: CallbackOwner,
    renderer: Arc<dyn SectionGeometry>,
}

impl Renderer {
    pub(super) fn new(owner: Option<&'static str>, renderer: Arc<dyn SectionGeometry>) -> Self {
        Self {
            owner: CallbackOwner::for_mod(owner.unwrap_or("unknown")),
            renderer,
        }
    }

    /// Whether this renderer is still enabled for this session.
    #[cfg(test)]
    pub(super) fn enabled(&self) -> bool {
        self.owner.is_enabled()
    }
}

/// The renderers, in registration order: set once at start, read by every table build.
static REGISTERED: OnceLock<Arc<[Renderer]>> = OnceLock::new();

/// Sets the native mods' section geometry renderers, once, before the resources first load; each
/// later [`super::MeshTables::new`] hands them to the meshing workers. False when already set.
pub fn register_section_geometry(
    renderers: Vec<(Option<&'static str>, Arc<dyn SectionGeometry>)>,
) -> bool {
    REGISTERED
        .set(
            renderers
                .into_iter()
                .map(|(owner, renderer)| Renderer::new(owner, renderer))
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

/// Bounds an untrusted sprite name in diagnostics without limiting valid atlas identifiers.
fn bound_sprite_name(sprite: &str) -> String {
    let end = sprite.floor_char_boundary(200);
    let mut out = String::with_capacity(end + 3);
    for ch in sprite[..end].chars() {
        if ch.is_control() {
            out.push('?');
        } else {
            out.push(ch);
        }
    }
    if end < sprite.len() {
        out.push('…');
    }
    out
}

/// Checks one quad against vanilla vertex/sprite rules and returns its block atlas layer.
///
/// Matches vanilla `BufferBuilder` and `TextureAtlas.getSprite`:
/// - any finite corner positions and UVs are accepted;
/// - unknown sprites fall back to [`MISSING_SPRITE`] (`minecraft:missingno`);
/// - each render pass is capped at [`MAX_LAYER_VERTICES`] (`BufferBuilder.MAX_VERTEX_COUNT`).
pub(super) fn check_quad(
    sprites: &BTreeMap<String, u32>,
    sprite: &str,
    quad: &SectionQuad,
    current_vertices: usize,
) -> Result<u16, String> {
    if !quad.positions.iter().flatten().all(|c| c.is_finite()) {
        return Err(format!("non-finite corner position: {:?}", quad.positions));
    }
    if !quad.uvs.iter().flatten().all(|c| c.is_finite()) {
        return Err(format!("non-finite UV coordinate: {:?}", quad.uvs));
    }
    if current_vertices.saturating_add(4) > MAX_LAYER_VERTICES {
        return Err(format!(
            "render pass has {current_vertices} vertices, exceeding vanilla BufferBuilder.MAX_VERTEX_COUNT ({MAX_LAYER_VERTICES})"
        ));
    }
    let layer = sprites
        .get(sprite)
        .or_else(|| sprites.get(MISSING_SPRITE))
        .copied()
        .ok_or_else(|| {
            format!(
                "sprite `{}` is missing and block atlas lacks `{MISSING_SPRITE}`",
                bound_sprite_name(sprite)
            )
        })?;
    if layer >= u32::from(OPAQUE_LAYER) {
        return Err(format!(
            "sprite `{}` layer {layer} exceeds terrain layer limit",
            bound_sprite_name(sprite)
        ));
    }
    Ok(layer as u16)
}

/// Lengths of [`SectionMesh`] fields touched by [`emit`], for rolling back a panicked renderer's
/// partial contribution without disturbing section blocks or other renderers.
#[derive(Clone, Copy)]
struct Checkpoint {
    solid: usize,
    solid_tints: usize,
    translucent: usize,
    sprite_layers: usize,
}

impl Checkpoint {
    fn capture(out: &SectionMesh) -> Self {
        Self {
            solid: out.solid.len(),
            solid_tints: out.solid_tints.len(),
            translucent: out.translucent.len(),
            sprite_layers: out.sprite_layers.len(),
        }
    }

    fn rollback(self, out: &mut SectionMesh) {
        out.solid.truncate(self.solid);
        out.solid_tints.truncate(self.solid_tints);
        out.translucent.truncate(self.translucent);
        out.sprite_layers.truncate(self.sprite_layers);
    }
}

/// Checked section collector handed to one [`SectionGeometry`] invocation.
struct Collector<'a> {
    sprites: &'a BTreeMap<String, u32>,
    animated_layers: &'a [bool],
    level: &'a dyn BlockAndTintGetter,
    out: &'a mut SectionMesh,
    mod_id: &'static str,
    origin: [i32; 3],
}

impl SectionGeometryContext for Collector<'_> {
    fn level(&self) -> &dyn BlockAndTintGetter {
        self.level
    }

    fn add(&mut self, sprite: &str, quad: SectionQuad) -> bool {
        let current_vertices = if quad.translucent {
            self.out.translucent.len()
        } else {
            self.out.solid.len()
        };
        match check_quad(self.sprites, sprite, &quad, current_vertices) {
            Ok(layer) => {
                emit(self.animated_layers, &quad, layer, self.out);
                true
            }
            Err(why) => {
                log::warn!(
                    "Sunrise: mod {} section geometry quad at {:?} was rejected: {why}",
                    self.mod_id,
                    self.origin
                );
                false
            }
        }
    }
}

/// Runs `renderers` against `out` through the checked section collector.
pub(super) fn run_renderers(
    renderers: &[Renderer],
    sprites: &BTreeMap<String, u32>,
    animated_layers: &[bool],
    level: &dyn BlockAndTintGetter,
    origin: [i32; 3],
    out: &mut SectionMesh,
) {
    for renderer in renderers {
        if !renderer.owner.is_enabled() {
            continue;
        }
        let checkpoint = Checkpoint::capture(out);
        let called = renderer
            .owner
            .call("section geometry", "add_section_geometry", || {
                let mut collector = Collector {
                    sprites,
                    animated_layers,
                    level,
                    out,
                    mod_id: renderer.owner.id(),
                    origin,
                };
                renderer.renderer.add(origin, &mut collector);
            });
        if called.is_err() {
            checkpoint.rollback(out);
        }
    }
}

/// Asks every enabled renderer to add the quads of the section `tints` is set up for, appending
/// accepted quads directly into `out` after the section's own blocks. A rejected quad affects only
/// itself; a panicking renderer rolls back only its own quads for this section and is disabled for
/// the session.
pub(super) fn add(mesher: &Mesher<'_>, tints: &mut Tints<'_>, out: &mut SectionMesh) {
    let [cx, cz, base_y] = tints.origin;
    let origin = [cx * 16, base_y, cz * 16];
    let region = Region {
        tables: mesher.tables,
        tints: RefCell::new(tints),
        nether: mesher.nether_shading,
    };
    run_renderers(
        &mesher.tables.section_geometry,
        &mesher.render.sprite_layers,
        &mesher.tables.animated_layers,
        &region,
        origin,
        out,
    );
}

/// `quad` as four section vertices in its pass, drawn with atlas `layer`.
fn emit(animated_layers: &[bool], quad: &SectionQuad, layer: u16, out: &mut SectionMesh) {
    if animated_layers
        .get(usize::from(layer))
        .copied()
        .unwrap_or(false)
    {
        out.note_sprite_layer(layer);
    }
    let [a, r, g, b] = quad.color.to_be_bytes().map(u32::from);
    let color = r | g << 8 | b << 16 | a << 24;
    let [sky, block] = quad.light.map(|l| u16::from(l.min(15)) * 16);
    let light = sky | block << 8;
    let vertices = std::array::from_fn(|i| {
        TerrainVertex::new(quad.positions[i], quad.uvs[i], layer, color, light)
    });
    if quad.translucent {
        out.translucent.extend(vertices);
    } else {
        out.solid.extend(vertices);
        let idx = out.solid.len() / 4 - 1;
        out.solid_tints.resize(idx, packed::UNKNOWN_TINT);
        out.solid_tints.push(quad.color & 0xFF_FFFF);
    }
}

#[cfg(test)]
#[path = "mod_geometry_diagnostic_tests.rs"]
mod diagnostic_tests;
