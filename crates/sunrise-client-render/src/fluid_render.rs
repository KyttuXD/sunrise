// Still-fluid tiling follows Catnip/Ponder 1.0.82 FluidRenderHelper.renderTiledFace.
// Copyright (c) 2022 The Create Team
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use glam::{DVec3, Mat4};
use sunrise_core::geometry::Direction;

/// Controls a still-fluid box.
#[derive(Clone, Copy, Debug)]
pub struct NativeFluidRenderOptions {
    pub render_bottom: bool,
    /// TextureAtlasSprite.uvShrinkRatio: 4 / the largest stitched atlas dimension.
    /// Explicit because Sunrise's block sprites occupy arrays rather than a stitched atlas.
    pub uv_shrink_ratio: f32,
}

/// A corner in final vertex coordinates, with UVs within the still sprite.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
}

/// Preserve sky light and increase block light to the fluid's luminosity (native light packing).
pub fn fluid_light(light: u32, luminosity: u8) -> u32 {
    (light & 0xf0) | (((light >> 8 & 0xf0).max(u32::from(luminosity.min(15)) * 16)) << 8)
}

/// Emit whole still-fluid tiles after the pose, keeping the UV fractions of the original grid.
/// Returning `false` from `emit` stops further emission (for example at a buffer limit).
pub fn fluid_box(
    bounds: [f32; 6],
    pose: Mat4,
    options: NativeFluidRenderOptions,
    mut emit: impl FnMut([FluidVertex; 4]) -> bool,
) {
    if !pose.is_finite()
        || !options.uv_shrink_ratio.is_finite()
        || bounds.iter().any(|x| !x.is_finite())
        || (0..3).any(|i| bounds[i] > bounds[i + 3])
    {
        return;
    }
    let pose = pose.as_dmat4();
    let [x0, y0, z0, x1, y1, z1] = bounds.map(f64::from);
    for (side, [a0, b0, a1, b1, plane], [a, b, depth]) in [
        (Direction::Down, [x0, z0, x1, z1, y0], [0, 2, 1]),
        (Direction::Up, [x0, z0, x1, z1, y1], [0, 2, 1]),
        (Direction::North, [x0, y0, x1, y1, z0], [0, 1, 2]),
        (Direction::South, [x0, y0, x1, y1, z1], [0, 1, 2]),
        (Direction::West, [z0, y0, z1, y1, x0], [2, 1, 0]),
        (Direction::East, [z0, y0, z1, y1, x1], [2, 1, 0]),
    ] {
        if a0 == a1 || b0 == b1 || (side == Direction::Down && !options.render_bottom) {
            continue;
        }
        let basis = |axis| pose.col(axis).truncate();
        let face = TiledFace {
            origin: basis(depth) * plane + pose.w_axis.truncate(),
            a: basis(a),
            b: basis(b),
            side,
            shrink: options.uv_shrink_ratio * 0.25,
        };
        let b_spans = TileSpan::of(b0, b1);
        for a_span in TileSpan::of(a0, a1).into_iter().flatten() {
            if !a_span.visit(&mut |a| {
                if !b_spans.iter().flatten().any(|&b| face.may_render(a, b)) {
                    return Visit::Skip;
                }
                if !a.single() {
                    return Visit::Split;
                }
                for b_span in b_spans.into_iter().flatten() {
                    if !b_span.visit(&mut |b| {
                        if !b.single() {
                            return if face.may_render(a, b) {
                                Visit::Split
                            } else {
                                Visit::Skip
                            };
                        }
                        let Some(quad) = face.quad(a, b) else {
                            return Visit::Skip;
                        };
                        if emit(quad) { Visit::Skip } else { Visit::Stop }
                    }) {
                        return Visit::Stop;
                    }
                }
                Visit::Skip
            }) {
                return;
            }
        }
    }
}

/// An ordered span of integer tile addresses, split at zero so the magnitude of every finite f32
/// coordinate fits in u128. Fractions are measured within the first and last tile of the span.
#[derive(Clone, Copy)]
struct TileSpan {
    lo: u128,
    hi: u128,
    start: f64,
    end: f64,
    negative: bool,
}

enum Visit {
    Skip,
    Split,
    Stop,
}

impl TileSpan {
    fn of(start: f64, end: f64) -> [Option<Self>; 2] {
        let span = |lo: f64, hi: f64, negative| Self {
            lo: lo.floor() as u128,
            hi: hi.ceil() as u128,
            start: lo.fract(),
            end: if hi.fract() == 0.0 { 1.0 } else { hi.fract() },
            negative,
        };
        [
            (start < 0.0).then(|| span(-end.min(0.0), -start, true)),
            (end > 0.0).then(|| span(start.max(0.0), end, false)),
        ]
    }

    fn single(self) -> bool {
        self.hi - self.lo == 1
    }

    fn visit(self, visit: &mut impl FnMut(Self) -> Visit) -> bool {
        match visit(self) {
            Visit::Skip => true,
            Visit::Stop => false,
            Visit::Split => {
                let mid = self.lo + (self.hi - self.lo) / 2;
                let lo = Self {
                    hi: mid,
                    end: 1.0,
                    ..self
                };
                let hi = Self {
                    lo: mid,
                    start: 0.0,
                    ..self
                };
                if self.negative {
                    hi.visit(visit) && lo.visit(visit)
                } else {
                    lo.visit(visit) && hi.visit(visit)
                }
            }
        }
    }

    /// High part of the world coordinate and its remaining tile-local offset. Keep the offset
    /// separate until after the large-coordinate terms of the affine transform (such as a
    /// translation bringing a distant box near the origin) cancel.
    fn endpoints(self) -> [[f64; 2]; 2] {
        let split = |i: u128, fraction: f64| {
            let shift = (u128::BITS - i.leading_zeros()).saturating_sub(f64::MANTISSA_DIGITS);
            let high = i >> shift << shift;
            let sign = if self.negative { -1.0 } else { 1.0 };
            [sign * high as f64, sign * ((i - high) as f64 + fraction)]
        };
        // Canonicalize the full upper edge to the neighboring tile's lower edge.
        let end = if self.end == 1.0 {
            split(self.hi, 0.0)
        } else {
            split(self.hi - 1, self.end)
        };
        let first = split(self.lo, self.start);
        if self.negative {
            [end, first]
        } else {
            [first, end]
        }
    }

    fn uv(self, reversed: bool) -> [f32; 2] {
        let [start, end] = if self.negative == reversed {
            [self.start, self.end]
        } else {
            [1.0 - self.end, 1.0 - self.start]
        };
        [start as f32, end as f32]
    }
}

struct TiledFace {
    origin: DVec3,
    a: DVec3,
    b: DVec3,
    side: Direction,
    shrink: f32,
}

impl TiledFace {
    fn corners(&self, a: TileSpan, b: TileSpan) -> [DVec3; 4] {
        let [a0, a1] = a.endpoints();
        let [b0, b1] = b.endpoints();
        let position = |[a, da]: [f64; 2], [b, db]: [f64; 2]| {
            let [a, b, o] = [self.a * a, self.b * b, self.origin];
            // Sum the large opposing terms first, before adding the tile-local offsets.
            DVec3::from_array(std::array::from_fn(|i| {
                let mut terms = [a[i], b[i], o[i]];
                terms.sort_unstable_by(|l, r| r.abs().total_cmp(&l.abs()));
                (terms[0] + terms[1]) + terms[2]
            })) + self.a * da
                + self.b * db
        };
        [
            position(a0, b0),
            position(a0, b1),
            position(a1, b0),
            position(a1, b1),
        ]
    }

    /// Each output coordinate is affine on the rectangle. Its extrema are at the corners:
    /// a span wholly outside the vertex format (or projected onto a line) needs no tiling.
    fn may_render(&self, a: TileSpan, b: TileSpan) -> bool {
        let corners = self.corners(a, b);
        let mut axis = DVec3::ONE;
        for i in 0..3 {
            let lo = corners.iter().map(|p| p[i]).fold(f64::INFINITY, f64::min) as f32;
            let hi = corners
                .iter()
                .map(|p| p[i])
                .fold(f64::NEG_INFINITY, f64::max) as f32;
            if lo == f32::INFINITY || hi == f32::NEG_INFINITY {
                return false;
            }
            if lo == hi {
                axis[i] = 0.0;
            }
        }
        (self.a * axis).cross(self.b * axis) != DVec3::ZERO
    }

    fn quad(&self, a: TileSpan, b: TileSpan) -> Option<[FluidVertex; 4]> {
        let pos = self.corners(a, b).map(|p| p.as_vec3());
        if pos.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let [u0, u1] = a.uv(matches!(self.side, Direction::North | Direction::East));
        let [v0, v1] = b.uv(self.side != Direction::Up);
        let order = match self.side {
            Direction::Up => [0, 1, 3, 2],
            Direction::North | Direction::East => [3, 2, 0, 1],
            _ => [1, 0, 2, 3],
        };
        let [p0, p1, p2, p3] = order.map(|i| pos[i].as_dvec3());
        let diagonal = p2 - p0;
        if diagonal.cross(p1 - p0) == DVec3::ZERO && diagonal.cross(p3 - p0) == DVec3::ZERO {
            return None;
        }
        let uv = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
        Some(std::array::from_fn(|i| FluidVertex {
            pos: pos[order[i]].to_array(),
            uv: uv[i].map(|v| v + self.shrink * (0.5 - v)),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_tiling_is_outward_and_does_not_inset_geometry() {
        let bounds = [-1.25, 0.125, -0.25, 1.5, 1.125, 0.75];
        let mut result = Vec::new();
        fluid_box(
            bounds,
            Mat4::IDENTITY,
            NativeFluidRenderOptions {
                render_bottom: false,
                uv_shrink_ratio: 4.0 / 2048.0,
            },
            |quad| {
                result.extend_from_slice(&quad);
                true
            },
        );
        assert_eq!(result.len(), (4 * 2 * 2 + 2 * 2 * 2 + 4 * 2) * 4);
        let up = std::iter::repeat_n(glam::Vec3::Y, 8);
        let north = std::iter::repeat_n(glam::Vec3::NEG_Z, 8);
        let south = std::iter::repeat_n(glam::Vec3::Z, 8);
        let west = std::iter::repeat_n(glam::Vec3::NEG_X, 4);
        let east = std::iter::repeat_n(glam::Vec3::X, 4);
        for (quad, normal) in result
            .as_chunks::<4>()
            .0
            .iter()
            .zip(up.chain(north).chain(south).chain(west).chain(east))
        {
            let [a, b, c, _] = quad.map(|v| glam::Vec3::from_array(v.pos));
            assert!((b - a).cross(c - a).dot(normal) > 0.0);
            assert!(
                quad.iter()
                    .all(|v| (0..3).all(|i| v.pos[i] >= bounds[i] && v.pos[i] <= bounds[i + 3]))
            );
        }
        assert!(
            result
                .iter()
                .any(|v| v.pos == [bounds[0], bounds[4], bounds[2]])
        );
    }

    #[test]
    fn distant_tiles_made_local_preserve_positions_and_uvs_on_both_axes() {
        let mut quads = Vec::new();
        fluid_box(
            [
                20_000_000.0,
                0.0,
                -20_000_002.0,
                20_000_008.0,
                1.0,
                -20_000_000.0,
            ],
            Mat4::from_translation(glam::Vec3::new(-20_000_000.0, 0.0, 20_000_002.0)),
            NativeFluidRenderOptions {
                render_bottom: false,
                uv_shrink_ratio: 0.0,
            },
            |quad| {
                quads.push(quad);
                true
            },
        );
        assert_eq!(quads.len(), 16 + 8 + 8 + 2 + 2);
        for x in 0..8 {
            for z in 0..2 {
                let quad = quads[2 * x + z];
                let (x, z) = (x as f32, z as f32);
                assert_eq!(
                    quad.map(|p| p.pos),
                    [
                        [x, 1.0, z],
                        [x, 1.0, z + 1.0],
                        [x + 1.0, 1.0, z + 1.0],
                        [x + 1.0, 1.0, z]
                    ]
                );
                assert_eq!(
                    quad.map(|p| p.uv),
                    [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
                );
            }
        }
        let north = quads[16];
        assert_eq!(
            north.map(|p| p.pos),
            [[1.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0; 3], [0.0, 1.0, 0.0]]
        );
        assert_eq!(
            north.map(|p| p.uv),
            [[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]]
        );
    }

    #[test]
    fn overflowed_spans_do_not_visit_their_tiles_but_neighbors_still_draw() {
        let pose = Mat4::from_scale(glam::Vec3::new(1.0, 2.0_f32.powi(127), 1.0));
        let options = NativeFluidRenderOptions {
            render_bottom: false,
            uv_shrink_ratio: 0.0,
        };
        fluid_box([0.0, 2.0, 0.0, 1e30, 2.0, 1e30], pose, options, |_| {
            panic!("a face wholly outside the vertex format must not emit")
        });
        let mut quads = Vec::new();
        fluid_box([0.0, 0.0, 0.0, 2.0, 1e30, 1.0], pose, options, |quad| {
            quads.push(quad);
            true
        });
        assert_eq!(quads.len(), 6);
        assert!(
            quads
                .iter()
                .flatten()
                .all(|v| [0.0, 2.0_f32.powi(127)].contains(&v.pos[1]))
        );
    }

    #[test]
    fn the_consumer_stops_a_stream_at_the_first_refused_quad() {
        let mut capped = 0usize;
        fluid_box(
            [0.0, 0.0, 0.0, 10.0, 1.0, 10.0],
            Mat4::IDENTITY,
            NativeFluidRenderOptions {
                render_bottom: true,
                uv_shrink_ratio: 0.0,
            },
            |_| {
                capped += 1;
                capped < 5
            },
        );
        assert_eq!(capped, 5);
    }

    #[test]
    fn reversed_or_non_finite_inputs_emit_nothing() {
        for (bounds, ratio) in [
            ([1.0, 0.0, 0.0, 0.0, 1.0, 1.0], 0.0),
            ([0.0, 0.0, 0.0, f32::NAN, 1.0, 1.0], 0.0),
            ([0.0, 0.0, 0.0, 1.0, 1.0, 1.0], f32::NAN),
        ] {
            fluid_box(
                bounds,
                Mat4::IDENTITY,
                NativeFluidRenderOptions {
                    render_bottom: true,
                    uv_shrink_ratio: ratio,
                },
                |_| panic!("invalid box must not emit: {bounds:?} {ratio:?}"),
            );
        }
    }
    #[test]
    fn luminosity_changes_only_block_light() {
        assert_eq!(fluid_light(0x20a0, 15), 0xf0a0);
        assert_eq!(fluid_light(0x25a5, 15), 0xf0a0);
        assert_eq!(fluid_light(0xe030, 3), 0xe030);
    }
}
