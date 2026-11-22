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

use glam::{Mat4, Vec3};
use sunrise_core::geometry::Direction;

/// Conservative `f64` bound slightly above `f32::MAX` plus four `f32` rounding steps.
/// Any `f32` affine transform component whose exact magnitude exceeds this is provably non-finite.
const FINITE_F32_BOUND: f64 = 3.402_825e38;

/// Exact integer boundary where `f32` unit spacing becomes `>= 2.0`, so `a.floor() + 1.0 == a`.
const F32_UNIT_STEP_LIMIT: f64 = 16_777_216.0;

/// Controls a still-fluid box. The tint is stack ARGB, independent of biome tint.
#[derive(Clone, Copy, Debug)]
pub struct NativeFluidRenderOptions {
    pub argb: Option<u32>,
    pub render_bottom: bool,
    /// TextureAtlasSprite.uvShrinkRatio: 4 / the largest stitched atlas dimension.
    /// Explicit because Sunrise's block sprites occupy arrays rather than a stitched atlas.
    pub uv_shrink_ratio: f32,
}

/// A corner before the caller's pose, with UVs within the still sprite, and its outward normal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FluidVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [i8; 3],
}

/// Preserve sky light and increase block light to the fluid's luminosity (native light packing).
pub fn fluid_light(light: u32, luminosity: u8) -> u32 {
    (light & 0xf0) | (((light >> 8 & 0xf0).max(u32::from(luminosity.min(15)) * 16)) << 8)
}

/// Emit tiled fluid quads transformed by `pose`, deriving renderable tile spans from `pose`
/// before traversing each face so provably overflowing faces or wide spans are skipped without
/// enumerating their individual tiles, while valid neighboring spans are retained. Returning
/// `false` from `emit` stops further emission (for example at a buffer limit). Negative
/// coordinates and exact integer boundaries retain the source's local UV fractions.
pub fn fluid_box(
    bounds: [f32; 6],
    pose: Mat4,
    options: NativeFluidRenderOptions,
    mut emit: impl FnMut([FluidVertex; 4]) -> bool,
) {
    if !options.uv_shrink_ratio.is_finite()
        || !pose.is_finite()
        || bounds.iter().any(|x| !x.is_finite())
        || (0..3).any(|i| bounds[i] > bounds[i + 3])
    {
        return;
    }
    let [x0, y0, z0, x1, y1, z1] = bounds;
    for (side, face) in [
        (Direction::Down, [x0, z0, x1, z1, y0]),
        (Direction::Up, [x0, z0, x1, z1, y1]),
        (Direction::North, [x0, y0, x1, y1, z0]),
        (Direction::South, [x0, y0, x1, y1, z1]),
        (Direction::West, [z0, y0, z1, y1, x0]),
        (Direction::East, [z0, y0, z1, y1, x1]),
    ] {
        if (side != Direction::Down || options.render_bottom)
            && !tiled_face(&mut emit, side, face, pose, options.uv_shrink_ratio)
        {
            return;
        }
    }
}

struct FaceSpans {
    a_range: (f64, f64),
    b_range: (f64, f64),
    col_a: [f64; 3],
    col_b: [f64; 3],
    base: [f64; 3],
    coupled_clip: bool,
}

fn intersect_linear(range: (f64, f64), coeff: f64, offset: f64) -> Option<(f64, f64)> {
    let (lo, hi) = range;
    if lo > hi {
        return None;
    }
    if coeff == 0.0 {
        return (offset.abs() <= FINITE_F32_BOUND).then_some((lo, hi));
    }
    let low_rhs = -FINITE_F32_BOUND - offset;
    let high_rhs = FINITE_F32_BOUND - offset;
    let (bound_lo, bound_hi) = if coeff > 0.0 {
        (low_rhs / coeff, high_rhs / coeff)
    } else {
        (high_rhs / coeff, low_rhs / coeff)
    };
    let lo = lo.max(bound_lo);
    let hi = hi.min(bound_hi);
    (lo <= hi).then_some((lo, hi))
}

fn derive_face_spans(side: Direction, face: [f32; 5], pose: Mat4) -> Option<FaceSpans> {
    let [a0, b0, a1, b1, plane] = face;
    if a0 >= a1 || b0 >= b1 {
        return None;
    }
    let col_f64 = |v: glam::Vec4| [f64::from(v.x), f64::from(v.y), f64::from(v.z)];
    let (col_a, col_b, col_plane_f32) = match side {
        Direction::Down | Direction::Up => (
            col_f64(pose.x_axis),
            col_f64(pose.z_axis),
            pose.y_axis.truncate().to_array(),
        ),
        Direction::North | Direction::South => (
            col_f64(pose.x_axis),
            col_f64(pose.y_axis),
            pose.z_axis.truncate().to_array(),
        ),
        Direction::West | Direction::East => (
            col_f64(pose.z_axis),
            col_f64(pose.y_axis),
            pose.x_axis.truncate().to_array(),
        ),
    };
    let w = col_f64(pose.w_axis);
    let mut base = [0.0; 3];
    for k in 0..3 {
        if !(col_plane_f32[k] * plane).is_finite() {
            return None;
        }
        base[k] = f64::from(col_plane_f32[k]) * f64::from(plane) + w[k];
    }

    let mut a_range = (f64::from(a0), f64::from(a1));
    let mut b_range = (f64::from(b0), f64::from(b1));
    for k in 0..3 {
        a_range = intersect_linear(a_range, col_a[k], 0.0)?;
        b_range = intersect_linear(b_range, col_b[k], 0.0)?;
    }

    // Exact 2D Fourier-Motzkin projection of |col_a * a + col_b * b + base| <= FINITE_F32_BOUND
    // and b in b_range onto `a`, eliminating any face or `a`-subspan with no feasible `b`.
    let mut b_bounds = [(0.0_f64, b_range.0, b_range.1); 4];
    let mut b_bound_count = 1usize;
    for k in 0..3 {
        let cb = col_b[k];
        if cb == 0.0 {
            a_range = intersect_linear(a_range, col_a[k], base[k])?;
        } else {
            let slope = -col_a[k] / cb;
            let center = -base[k] / cb;
            let half = FINITE_F32_BOUND / cb.abs();
            b_bounds[b_bound_count] = (slope, center - half, center + half);
            b_bound_count += 1;
        }
    }
    for i in 0..b_bound_count {
        let (slope_i, lo_i, _) = b_bounds[i];
        for j in 0..b_bound_count {
            let (slope_j, _, hi_j) = b_bounds[j];
            let s = slope_i - slope_j;
            let rhs = hi_j - lo_i;
            if s == 0.0 {
                if rhs < 0.0 {
                    return None;
                }
            } else if s > 0.0 {
                a_range.1 = a_range.1.min(rhs / s);
            } else {
                a_range.0 = a_range.0.max(rhs / s);
            }
            if a_range.0 > a_range.1 {
                return None;
            }
        }
    }

    for k in 0..3 {
        let ca = col_a[k];
        let (ca_min, ca_max) = if ca >= 0.0 {
            (ca * a_range.0, ca * a_range.1)
        } else {
            (ca * a_range.1, ca * a_range.0)
        };
        let cb = col_b[k];
        if cb == 0.0 {
            if base[k] + ca_min > FINITE_F32_BOUND || base[k] + ca_max < -FINITE_F32_BOUND {
                return None;
            }
        } else {
            let low_rhs = -FINITE_F32_BOUND - (base[k] + ca_max);
            let high_rhs = FINITE_F32_BOUND - (base[k] + ca_min);
            let (lo, hi) = if cb > 0.0 {
                (low_rhs / cb, high_rhs / cb)
            } else {
                (high_rhs / cb, low_rhs / cb)
            };
            b_range.0 = b_range.0.max(lo);
            b_range.1 = b_range.1.min(hi);
            if b_range.0 > b_range.1 {
                return None;
            }
        }
    }

    let mut coupled_clip = false;
    for k in 0..3 {
        for a in [a_range.0, a_range.1] {
            for b in [b_range.0, b_range.1] {
                if (col_a[k] * a + col_b[k] * b + base[k]).abs() > FINITE_F32_BOUND {
                    coupled_clip = true;
                }
            }
        }
    }

    Some(FaceSpans {
        a_range,
        b_range,
        col_a,
        col_b,
        base,
        coupled_clip,
    })
}

fn align_span(start: f32, end: f32, valid: (f64, f64)) -> Option<(f32, f64)> {
    let (lo, hi) = valid;
    if lo > hi || start >= end || hi < f64::from(start) || lo > f64::from(end) {
        return None;
    }
    let next_start = start.floor() + 1.0;
    if next_start <= start {
        return (f64::from(start) >= lo && f64::from(end) <= hi).then_some((start, f64::from(end)));
    }
    let aligned_start = if lo <= f64::from(start) {
        start
    } else {
        let cand = lo.ceil().max(f64::from(next_start));
        if cand > F32_UNIT_STEP_LIMIT || cand >= f64::from(end) {
            return None;
        }
        cand as f32
    };
    let max_end = hi.min(f64::from(end));
    let first_next = aligned_start.floor() + 1.0;
    let first_end = if first_next > aligned_start {
        first_next.min(end)
    } else {
        end
    };
    (f64::from(first_end) <= max_end).then_some((aligned_start, max_end))
}

fn tiled_face(
    out: &mut impl FnMut([FluidVertex; 4]) -> bool,
    side: Direction,
    face: [f32; 5],
    pose: Mat4,
    ratio: f32,
) -> bool {
    let [a0, b0, a1, b1, plane] = face;
    let Some(spans) = derive_face_spans(side, face, pose) else {
        return true;
    };
    let Some((start_a, max_end_a)) = align_span(a0, a1, spans.a_range) else {
        return true;
    };
    let Some((default_start_b, default_max_end_b)) = align_span(b0, b1, spans.b_range) else {
        return true;
    };
    let positive = matches!(side, Direction::Up | Direction::South | Direction::East);
    let shrink = ratio * 0.25;
    let lerp = |v| v + shrink * (0.5 - v);
    let mut a = start_a;
    while a < a1 {
        let next_a = a.floor() + 1.0;
        let end_a = if next_a > a { next_a.min(a1) } else { a1 };
        if f64::from(end_a) > max_end_a {
            break;
        }
        let (start_b, max_end_b) = if spans.coupled_clip {
            let mut strip_b = spans.b_range;
            let mut valid = true;
            for k in 0..3 {
                let Some(r) = intersect_linear(
                    strip_b,
                    spans.col_b[k],
                    spans.col_a[k] * f64::from(a) + spans.base[k],
                ) else {
                    valid = false;
                    break;
                };
                let Some(r) = intersect_linear(
                    r,
                    spans.col_b[k],
                    spans.col_a[k] * f64::from(end_a) + spans.base[k],
                ) else {
                    valid = false;
                    break;
                };
                strip_b = r;
            }
            if !valid {
                a = end_a;
                continue;
            }
            let Some(aligned_b) = align_span(b0, b1, strip_b) else {
                a = end_a;
                continue;
            };
            aligned_b
        } else {
            (default_start_b, default_max_end_b)
        };
        let (u0, u1) = if matches!(side, Direction::North | Direction::East) {
            (end_a.ceil() - end_a, end_a.ceil() - a)
        } else {
            (a - a.floor(), end_a - a.floor())
        };
        let (u0, u1) = (lerp(u0), lerp(u1));
        let mut b = start_b;
        while b < b1 {
            let next_b = b.floor() + 1.0;
            let end_b = if next_b > b { next_b.min(b1) } else { b1 };
            if f64::from(end_b) > max_end_b {
                break;
            }
            let (v0, v1) = if side == Direction::Up {
                (b - b.floor(), end_b - b.floor())
            } else {
                (end_b.ceil() - end_b, end_b.ceil() - b)
            };
            let (v0, v1) = (lerp(v0), lerp(v1));
            let local_positions = match side {
                Direction::West | Direction::East => {
                    let (near, far) = if positive { (end_a, a) } else { (a, end_a) };
                    [
                        [plane, end_b, near],
                        [plane, b, near],
                        [plane, b, far],
                        [plane, end_b, far],
                    ]
                }
                Direction::North | Direction::South => {
                    let (near, far) = if positive { (a, end_a) } else { (end_a, a) };
                    [
                        [near, end_b, plane],
                        [near, b, plane],
                        [far, b, plane],
                        [far, end_b, plane],
                    ]
                }
                Direction::Down | Direction::Up => {
                    let (near, far) = if positive { (b, end_b) } else { (end_b, b) };
                    [
                        [a, plane, near],
                        [a, plane, far],
                        [end_a, plane, far],
                        [end_a, plane, near],
                    ]
                }
            };
            let positions =
                local_positions.map(|p| pose.transform_point3(Vec3::from_array(p)).to_array());
            let normal = match side {
                Direction::Down => [0, -1, 0],
                Direction::Up => [0, 1, 0],
                Direction::North => [0, 0, -1],
                Direction::South => [0, 0, 1],
                Direction::West => [-1, 0, 0],
                Direction::East => [1, 0, 0],
            };
            let uvs = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
            if uvs.iter().flatten().all(|v| v.is_finite()) {
                let quad = std::array::from_fn(|i| FluidVertex {
                    pos: positions[i],
                    uv: uvs[i],
                    normal,
                });
                if !out(quad) {
                    return false;
                }
            }
            b = end_b;
        }
        a = end_a;
    }
    true
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
                argb: None,
                render_bottom: false,
                uv_shrink_ratio: 4.0 / 2048.0,
            },
            |quad| {
                result.extend_from_slice(&quad);
                true
            },
        );
        assert_eq!(result.len(), (4 * 2 * 2 + 2 * 2 * 2 + 4 * 2) * 4);
        for quad in result.as_chunks::<4>().0 {
            let [a, b, c, _] = quad.map(|v| glam::Vec3::from_array(v.pos));
            assert!(
                (b - a)
                    .cross(c - a)
                    .dot(glam::Vec3::from_array(quad[0].normal.map(f32::from)))
                    > 0.0
            );
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
    fn large_coordinates_large_quad_counts_and_unclamped_shrink_emit_without_artificial_caps() {
        // Coordinates beyond the removed 1,048,576-block cap emit normally.
        let mut far_quads = 0usize;
        fluid_box(
            [2_000_000.0, 0.0, 0.0, 2_000_001.0, 1.0, 1.0],
            Mat4::IDENTITY,
            NativeFluidRenderOptions {
                argb: None,
                render_bottom: true,
                uv_shrink_ratio: 1.5,
            },
            |_| {
                far_quads += 1;
                true
            },
        );
        assert_eq!(far_quads, 6);

        // Coordinates at f32 ULP >= 2.0 terminate cleanly instead of looping.
        let mut huge_quads = 0usize;
        fluid_box(
            [20_000_000.0, 0.0, 0.0, 20_000_002.0, 1.0, 1.0],
            Mat4::IDENTITY,
            NativeFluidRenderOptions {
                argb: None,
                render_bottom: true,
                uv_shrink_ratio: 0.0,
            },
            |_| {
                huge_quads += 1;
                true
            },
        );
        assert_eq!(huge_quads, 6);

        // Boxes exceeding the removed 65,536-quad cap emit every tiled quad.
        let mut big_quads = 0usize;
        fluid_box(
            [0.0, 0.0, 0.0, 200.0, 1.0, 200.0],
            Mat4::IDENTITY,
            NativeFluidRenderOptions {
                argb: None,
                render_bottom: true,
                uv_shrink_ratio: 0.0,
            },
            |_| {
                big_quads += 1;
                true
            },
        );
        assert_eq!(big_quads, 200 * 200 * 2 + 200 * 4);

        // Returning false stops further emission immediately (buffer limit).
        let mut capped = 0usize;
        fluid_box(
            [0.0, 0.0, 0.0, 10.0, 1.0, 10.0],
            Mat4::IDENTITY,
            NativeFluidRenderOptions {
                argb: None,
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
    fn overflowing_pose_skips_invalid_wide_spans_and_retains_valid_neighbors() {
        let options = NativeFluidRenderOptions {
            argb: None,
            render_bottom: true,
            uv_shrink_ratio: 0.0,
        };

        // Entire wide X/Z box overflows Y under a large finite Y scale: zero tiles enumerated.
        let mut visited = 0usize;
        fluid_box(
            [0.0, 10.0, 0.0, 1_000_000.0, 11.0, 1_000_000.0],
            Mat4::from_scale(Vec3::new(1.0, 1e38, 1.0)),
            options,
            |_| {
                visited += 1;
                true
            },
        );
        assert_eq!(visited, 0);

        // Wide X span overflows past x = 3 while x in [0, 3] remains valid; top face (y = 10)
        // overflows completely while bottom (y = 0) and side tiles in [0, 3] x [0, 3] remain.
        let mut valid = 0usize;
        let mut total = 0usize;
        fluid_box(
            [0.0, 0.0, 0.0, 1_000_000.0, 10.0, 1.0],
            Mat4::from_scale(Vec3::new(1e38, 1e38, 1.0)),
            options,
            |quad| {
                total += 1;
                if quad.iter().all(|v| v.pos.into_iter().all(f32::is_finite)) {
                    valid += 1;
                }
                true
            },
        );
        // Valid tiles: Down (3x1 = 3), Up (0), North (3x3 = 9), South (3x3 = 9), West (1x3 = 3),
        // East (0 because x1 = 1_000_000 overflows). Total valid = 24, and no wide traversal.
        assert_eq!(valid, 24);
        assert!(total <= 30, "enumerated too many tiles: {total}");

        // Leading invalid wide span [-1_000_000, 0) skipped in O(1); valid neighbor [0, 3] kept.
        let pose = Mat4::from_translation(Vec3::new(-3.4e38, 0.0, 0.0))
            * Mat4::from_scale(Vec3::new(1e36, 1.0, 1.0));
        let mut kept = 0usize;
        let mut seen = 0usize;
        fluid_box(
            [-1_000_000.0, 0.0, 0.0, 3.0, 1.0, 1.0],
            pose,
            options,
            |quad| {
                seen += 1;
                if quad.iter().all(|v| v.pos.into_iter().all(f32::is_finite)) {
                    kept += 1;
                }
                true
            },
        );
        // Valid faces on [0, 3] x [0, 1] x [0, 1]: Down(3) + Up(3) + North(3) + South(3) + East(1) = 13.
        // West is at x0 = -1_000_000 (overflows -> 0).
        assert_eq!(kept, 13);
        assert!(seen <= 20, "leading invalid span enumerated tiles: {seen}");
    }

    #[test]
    fn reversed_or_non_finite_inputs_emit_nothing() {
        for (bounds, pose, ratio) in [
            ([1.0, 0.0, 0.0, 0.0, 1.0, 1.0], Mat4::IDENTITY, 0.0),
            ([0.0, 0.0, 0.0, f32::NAN, 1.0, 1.0], Mat4::IDENTITY, 0.0),
            ([0.0, 0.0, 0.0, 1.0, 1.0, 1.0], Mat4::IDENTITY, f32::NAN),
            (
                [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                Mat4::from_scale(Vec3::splat(f32::INFINITY)),
                0.0,
            ),
        ] {
            fluid_box(
                bounds,
                pose,
                NativeFluidRenderOptions {
                    argb: None,
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
