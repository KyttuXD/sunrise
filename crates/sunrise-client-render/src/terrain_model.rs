//! Native terrain-sheet models with explicit material and UV policy.
use crate::{gfx, mesher::Vertex, models::catalog::BakedFace};
use glam::{Mat3, Mat4, Vec3};
use sunrise_assets::bake::RenderType;
fn same_sprite(left: &str, right: &str) -> bool {
    left.strip_prefix("minecraft:").unwrap_or(left)
        == right.strip_prefix("minecraft:").unwrap_or(right)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeModelMaterial {
    /// Use each baked quad's material: solid and cutout ones the terrain's, a translucent one the
    /// sorted translucent material (vanilla's `translucent_moving_block`). Quads with an unknown or
    /// tripwire render type are skipped individually. A `color` alpha below 255 puts every quad in
    /// the translucent material, whatever the policy.
    #[default]
    Model,
    Solid,
    Cutout,
    CutoutMipped,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeModelShade {
    /// Respect the baked quad's directional-shading flag.
    #[default]
    Model,
    Off,
    /// The level's two entity light directions, even at full block/sky light.
    Entity,
}

/// Transform UVs only on the explicitly named original sprite, in unit-sprite coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeModelUvShift<'a> {
    pub source_sprite: &'a str,
    pub target_sprite: &'a str,
    pub scale: [f32; 2],
    pub offset: [f32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NativeModelRenderOptions<'a> {
    pub material: NativeModelMaterial,
    pub shade: NativeModelShade,
    pub uv_shift: Option<NativeModelUvShift<'a>>,
    /// Optional compatibility policy applied toward each face's UV bounds center before shifting.
    /// Zero retains the baked UVs. For an older JSON FaceBakery use its actual atlas shrink ratio;
    /// OBJ loaders that do not shrink UVs must keep zero.
    pub uv_shrink_ratio: f32,
    /// RGB multiplier on quads with a tint index; other quads keep their baked color.
    pub tint: u32,
    /// `0xAARRGGBB` multiplied into every quad's vertex colour after the baked colour and the
    /// tint, alpha included. `u32::MAX` multiplies nothing.
    pub color: u32,
    /// The alpha (`0..=255`) multiplied into quads with a tint index, whose `tint` has none of its own.
    pub tint_alpha: u8,
    /// Faces left out, one bit per [`sunrise_assets::model::Direction`] (down, up, north, south,
    /// west, east): a quad whose `cull` side is set is skipped.
    pub hidden_faces: u8,
}
impl Default for NativeModelRenderOptions<'_> {
    fn default() -> Self {
        Self {
            material: NativeModelMaterial::Model,
            shade: NativeModelShade::Model,
            uv_shift: None,
            uv_shrink_ratio: 0.0,
            tint: 0xff_ffff,
            color: u32::MAX,
            tint_alpha: 255,
            hidden_faces: 0,
        }
    }
}

/// How a quad is drawn: a terrain model material's overlay, or the translucent one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    Terrain(u32),
    Translucent,
}

fn route(face: &BakedFace, policy: NativeModelMaterial, faded: bool) -> Option<Route> {
    use NativeModelMaterial as M;
    let route = match policy {
        M::Solid => Route::Terrain(gfx::TERRAIN_MODEL_SOLID_OVERLAY),
        M::Cutout => Route::Terrain(gfx::TERRAIN_MODEL_CUTOUT_OVERLAY),
        M::CutoutMipped => Route::Terrain(gfx::TERRAIN_MODEL_CUTOUT_MIPPED_OVERLAY),
        M::Model => match face.quad.render_type.unwrap_or(RenderType::Solid) {
            // A solid quad whose material is `forceTranslucent` (`Material.Baked`: the sprite is
            // drawn as translucent whatever its texels say) is a translucent quad.
            RenderType::Solid if face.quad.material.force_translucent => Route::Translucent,
            RenderType::Solid => Route::Terrain(gfx::TERRAIN_MODEL_SOLID_OVERLAY),
            RenderType::Translucent => Route::Translucent,
            RenderType::Cutout => Route::Terrain(gfx::TERRAIN_MODEL_CUTOUT_OVERLAY),
            RenderType::CutoutMipped => Route::Terrain(gfx::TERRAIN_MODEL_CUTOUT_MIPPED_OVERLAY),
            _ => return None,
        },
    };
    // A colour with alpha below opaque draws a terrain route's quad translucent too; a
    // refused render type was refused above.
    Some(match route {
        Route::Terrain(_) if faded => Route::Translucent,
        route => route,
    })
}

/// `color` (ABGR, as vertices hold it) times the `0xAARRGGBB` draw colour `draw`, channel by
/// channel.
pub fn multiply_draw_color(color: u32, draw: u32) -> u32 {
    crate::hud_layout::multiply(color, crate::hud_layout::abgr(draw))
}

/// `PoseStack.Pose` carries a separate normal matrix. Rebuilding it from positions loses
/// information when, for example, a uniform zero scale leaves the normal matrix unchanged.
#[derive(Clone, Copy)]
pub struct NormalPose {
    matrix: Mat3,
    trusted: bool,
}

impl NormalPose {
    pub const IDENTITY: Self = Self {
        matrix: Mat3::IDENTITY,
        trusted: true,
    };

    /// An arbitrary matrix supplied as a whole, like `PoseStack.Pose.mulPose`.
    pub fn from_affine(pose: Mat4) -> Self {
        let basis = Mat3::from_mat4(pose);
        if basis == Mat3::IDENTITY {
            return Self::IDENTITY;
        }
        let det = basis.determinant();
        Self {
            matrix: if det.is_finite() && det != 0.0 {
                basis.inverse().transpose()
            } else {
                Mat3::ZERO
            },
            trusted: false,
        }
    }

    pub fn rotate(&mut self, by: glam::Quat) {
        self.matrix *= Mat3::from_quat(by);
    }

    pub fn scale(&mut self, by: Vec3) {
        let abs = by.abs();
        if abs.x == abs.y && abs.y == abs.z {
            if by.min_element() < 0.0 {
                self.matrix *= Mat3::from_diagonal(by.signum());
            }
        } else {
            self.matrix *= Mat3::from_diagonal(by.recip());
            self.trusted = false;
        }
    }

    pub fn transform(self, normal: Vec3) -> Option<Vec3> {
        let transformed = self.matrix * normal;
        if self.trusted {
            (transformed.is_finite() && transformed != Vec3::ZERO).then_some(transformed)
        } else {
            transformed.try_normalize()
        }
    }
}

/// Forward each face's baked sprite layer and submit valid quads independently. Only explicit UV
/// shifts look up a sprite name, in the blocks atlas, where a missing target uses the atlas's
/// missing sprite (`TextureAtlas.getSprite`). Returning `false` from `emit` stops submission.
pub fn draw_model(
    faces: &[BakedFace],
    pose: Mat4,
    normal_pose: NormalPose,
    light: u32,
    options: NativeModelRenderOptions<'_>,
    sprite: impl Fn(&str) -> Option<u32>,
    mut emit: impl FnMut([Vertex; 4]) -> bool,
) {
    if !pose.is_finite() {
        log::warn!("Terrain model: the draw pose is not representable");
        return;
    }
    let mut invalid = 0usize;
    let shift = options.uv_shift.map(|shift| {
        let target = sprite(shift.target_sprite).or_else(|| sprite("minecraft:missingno"));
        (target, shift)
    });
    let faded = options.color >> 24 != 0xff;
    for face in faces {
        if face
            .quad
            .cull
            .is_some_and(|side| options.hidden_faces >> side as u8 & 1 != 0)
        {
            continue;
        }
        let quad = &face.quad;
        let Some(route) = route(face, options.material, faded) else {
            invalid += 1;
            continue;
        };
        let mut uv = quad.uv;
        if options.uv_shrink_ratio != 0.0 {
            let center = std::array::from_fn::<_, 2, _>(|i| {
                let min = uv.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min);
                let max = uv.iter().map(|p| p[i]).fold(f32::NEG_INFINITY, f32::max);
                (min + min + max + max) / 4.0
            });
            for p in &mut uv {
                *p = std::array::from_fn(|i| p[i] + options.uv_shrink_ratio * (center[i] - p[i]));
            }
        }
        let shifted = shift.filter(|(_, s)| same_sprite(&quad.material.sprite, s.source_sprite));
        let layer = if let Some((target, shift)) = shifted {
            let Some(target) = target else {
                invalid += 1;
                continue;
            };
            for p in &mut uv {
                *p = std::array::from_fn(|i| p[i] * shift.scale[i] + shift.offset[i]);
            }
            target
        } else {
            face.layer
        };
        // The terrain-model vertex packs the whole texture-array layer into the upper 16 bits.
        if u16::try_from(layer).is_err() || uv.iter().flatten().any(|v| !v.is_finite()) {
            invalid += 1;
            continue;
        }
        let positions = quad
            .positions
            .map(|p| pose.transform_point3(Vec3::from_array(p)));
        if positions.iter().any(|p| !p.is_finite()) {
            invalid += 1;
            continue;
        }
        let shaded = match options.shade {
            NativeModelShade::Model => quad.shade,
            NativeModelShade::Off => false,
            NativeModelShade::Entity => true,
        };
        let normal = if shaded && matches!(route, Route::Terrain(_)) {
            let p = quad.positions.map(Vec3::from_array);
            let Some(n) = (p[1] - p[0]).cross(p[2] - p[0]).try_normalize() else {
                invalid += 1;
                continue;
            };
            // Local normals use the baked signed-byte precision, but the pose's result is float.
            let n = n.map(|v| ((v * 127.0) as i8) as f32 / 127.0);
            let Some(n) = normal_pose.transform(n) else {
                invalid += 1;
                continue;
            };
            n
        } else {
            Vec3::ZERO
        };
        let normal_bits = normal.z.to_ne_bytes().map(|b| b as i8);
        let argb = quad.color.map_or(u32::MAX, |v| v as u32);
        let mut color = (argb & 0xff00_ff00) | (argb >> 16 & 0xff) | (argb & 0xff) << 16;
        if quad.tint_index >= 0 {
            let tint = options.tint;
            let rgb = (tint >> 16 & 0xff) | (tint & 0xff00) | (tint & 0xff) << 16;
            color = (((color >> 24) * u32::from(options.tint_alpha) / 255) << 24)
                | ((color & 0xff) * (rgb & 0xff) / 255)
                | (((color >> 8 & 0xff) * (rgb >> 8 & 0xff) / 255) << 8)
                | (((color >> 16 & 0xff) * (rgb >> 16 & 0xff) / 255) << 16);
        }
        if options.color != u32::MAX {
            color = multiply_draw_color(color, options.color);
        }
        let light = (light & 0xff).max(u32::from(quad.light[1]) * 16)
            | ((light >> 8 & 0xff).max(u32::from(quad.light[0].max(quad.light_emission)) * 16)
                << 8);
        let quad_vertices = match route {
            Route::Terrain(overlay) => std::array::from_fn(|i| Vertex {
                pos: positions[i].to_array(),
                uv: uv[i],
                layer: overlay | layer << 16,
                color,
                light,
                normal: normal_bits,
                glint_uv: [normal.x, normal.y],
            }),
            Route::Translucent => {
                // No normal lighting: the directional shade is in the colour, as a block quad's is.
                let color = if shaded {
                    let shade = (crate::models::cube::shade(face) * 255.0) as u32;
                    crate::hud_layout::multiply(color, 0xFF00_0000 | (shade * 0x01_0101))
                } else {
                    color
                };
                std::array::from_fn(|i| Vertex {
                    pos: positions[i].to_array(),
                    uv: uv[i],
                    layer: gfx::TERRAIN_MODEL_TRANSLUCENT_OVERLAY | layer << 16,
                    color,
                    light,
                    normal: [0; 4],
                    glint_uv: [0.0; 2],
                })
            }
        };
        if !emit(quad_vertices) {
            break;
        }
    }
    if invalid != 0 {
        log::warn!(
            "Terrain model: skipped {invalid} quads with invalid material, position, UV or normal"
        );
    }
}

#[cfg(test)]
#[path = "terrain_model_tests.rs"]
mod tests;
