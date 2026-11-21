//! Ordered block visits for a finite ray, as in BlockGetter.traverseBlocks.
use super::{Vec3, floor_i32};
use crate::position::BlockPos;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraversalError {
    NonFinite,
    VisitLimit,
    InvalidBounds,
}
impl fmt::Display for TraversalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "block traversal failed: {self:?}")
    }
}
impl std::error::Error for TraversalError {}

/// Inclusive block bounds of a provider that is empty outside this box.
#[derive(Clone, Copy, Debug)]
pub struct BlockTraversalBounds {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

/// Visit blocks in vanilla ray order until the visitor returns Some, or the
/// segment is exhausted. `None` corresponds to vanilla's miss callback.
///
/// Non-finite geometry and exceeding the caller's visit budget return typed
/// errors. Previously invoked callbacks are not rolled back on a budget error.
pub fn traverse_blocks<T>(
    start: Vec3,
    end: Vec3,
    max_visits: usize,
    visitor: impl FnMut(BlockPos) -> Option<T>,
) -> Result<Option<T>, TraversalError> {
    traverse(start, end, Some(max_visits), None, visitor)
}

/// Visit blocks in vanilla order until the visitor returns Some or the segment ends.
/// The caller's provider owns its policy for positions it cannot read.
pub fn traverse_blocks_on_segment<T>(
    start: Vec3,
    end: Vec3,
    visitor: impl FnMut(BlockPos) -> Option<T>,
) -> Result<Option<T>, TraversalError> {
    traverse(start, end, None, None, visitor)
}

/// Visit only blocks in `bounds`, without a visit quota or changing the ray's
/// endpoints. The time and tie order of the first retained step are unchanged.
pub fn traverse_blocks_in_bounds<T>(
    start: Vec3,
    end: Vec3,
    bounds: BlockTraversalBounds,
    visitor: impl FnMut(BlockPos) -> Option<T>,
) -> Result<Option<T>, TraversalError> {
    traverse(start, end, None, Some(bounds), visitor)
}

fn traverse<T>(
    start: Vec3,
    end: Vec3,
    max_visits: Option<usize>,
    bounds: Option<BlockTraversalBounds>,
    mut visitor: impl FnMut(BlockPos) -> Option<T>,
) -> Result<Option<T>, TraversalError> {
    if !start.is_finite() || !end.is_finite() {
        return Err(TraversalError::NonFinite);
    }
    if start == end {
        return Ok(None);
    }
    let finish = end.lerp(start, -1e-7);
    let origin = start.lerp(end, -1e-7);
    let delta = origin.vector_to(finish).components();
    if !origin.is_finite() || !finish.is_finite() || delta.iter().any(|v| !v.is_finite()) {
        return Err(TraversalError::NonFinite);
    }
    let mut position = origin.components().map(floor_i32);
    let sign = delta.map(|v| {
        if v == 0.0 {
            0i32
        } else if v > 0.0 {
            1
        } else {
            -1
        }
    });
    let step = std::array::from_fn::<_, 3, _>(|i| {
        if sign[i] == 0 {
            f64::MAX
        } else {
            sign[i] as f64 / delta[i]
        }
    });
    let mut boundary = std::array::from_fn::<_, 3, _>(|i| {
        let value = origin.components()[i];
        let fraction = value - (value.floor() as i64) as f64;
        step[i]
            * if sign[i] > 0 {
                1.0 - fraction
            } else {
                fraction
            }
    });
    // A NaN cannot participate in the strict boundary ordering below. For example,
    // a subnormal delta leaving an integer face can multiply infinity by zero.
    if boundary
        .iter()
        .any(|t| t.is_nan() || *t == f64::NEG_INFINITY)
    {
        return Err(TraversalError::NonFinite);
    }
    if let Some(bounds) = bounds {
        if (0..3).any(|i| bounds.min[i] > bounds.max[i]) {
            return Err(TraversalError::InvalidBounds);
        }
        if !skip_empty_prefix(bounds, &mut position, sign, &mut boundary, step) {
            return Ok(None);
        }
    }
    let mut remaining = max_visits;
    loop {
        if let Some(count) = &mut remaining {
            if *count == 0 {
                return Err(TraversalError::VisitLimit);
            }
            *count -= 1;
        }
        if let Some(result) = visitor(BlockPos::new(position[0], position[1], position[2])) {
            return Ok(Some(result));
        }
        if !(boundary[0] <= 1.0 || boundary[1] <= 1.0 || boundary[2] <= 1.0) {
            return Ok(None);
        }
        // Strict comparisons give Z priority on a three-way corner tie.
        let axis = if boundary[0] < boundary[1] {
            if boundary[0] < boundary[2] { 0 } else { 2 }
        } else if boundary[1] < boundary[2] {
            1
        } else {
            2
        };
        if bounds.is_some_and(|bounds| {
            (sign[axis] > 0 && position[axis] == bounds.max[axis])
                || (sign[axis] < 0 && position[axis] == bounds.min[axis])
        }) {
            return Ok(None);
        }
        position[axis] = position[axis].wrapping_add(sign[axis]);
        boundary[axis] += step[axis];
    }
}

/// Advance each axis to the moment the last outside axis enters the box. The
/// number of steps on each axis is independent of the interleaving; on an
/// equal boundary time, the DDA orders Z, then Y, then X.
fn skip_empty_prefix(
    bounds: BlockTraversalBounds,
    position: &mut [i32; 3],
    sign: [i32; 3],
    boundary: &mut [f64; 3],
    step: [f64; 3],
) -> bool {
    let mut exit = [0; 3];
    let mut entry: Option<(f64, usize, u64)> = None;
    for i in 0..3 {
        let p = i64::from(position[i]);
        let (min, max) = (i64::from(bounds.min[i]), i64::from(bounds.max[i]));
        let (near, far) = match sign[i] {
            1 => (min - p, max - p),
            -1 => (p - max, p - min),
            _ if (min..=max).contains(&p) => continue,
            _ => return false,
        };
        if far < 0 {
            return false;
        }
        exit[i] = (far + 1) as u64;
        if near > 0 {
            let time = advance_boundary(boundary[i], step[i], (near - 1) as u64);
            if time.is_nan() || time > 1.0 {
                return false;
            }
            if entry.is_none_or(|(t, axis, _)| time > t || (time == t && i < axis)) {
                entry = Some((time, i, near as u64));
            }
        }
    }
    let Some((time, axis, axis_steps)) = entry else {
        return true;
    };
    for i in 0..3 {
        if sign[i] == 0 {
            // A stationary axis on an integer plane still has one boundary at time zero.
            // Consume it if the ordinary walk would have visited it in the skipped prefix.
            if boundary[i] < time || (boundary[i] == time && i > axis) {
                boundary[i] += step[i];
            }
            continue;
        }
        let steps = if i == axis {
            axis_steps
        } else {
            let (mut lo, mut hi) = (0, exit[i]);
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                let t = advance_boundary(boundary[i], step[i], mid);
                if t < time || (t == time && i > axis) {
                    lo = mid + 1;
                } else {
                    hi = mid;
                }
            }
            lo
        };
        if steps == exit[i] {
            return false;
        }
        position[i] = (i64::from(position[i]) + i64::from(sign[i]) * steps as i64) as i32;
        boundary[i] = advance_boundary(boundary[i], step[i], steps);
    }
    true
}

/// Fast-forward `boundary += step` without replacing repeated rounding with a
/// multiplication. Within one sign/exponent band the values are an integer
/// lattice. Two equal rounded strides (including ties-to-even) establish the
/// stride until the next band. Cross each band boundary with ordinary addition.
fn advance_boundary(mut boundary: f64, step: f64, mut count: u64) -> f64 {
    const FRACTION: u64 = (1 << 52) - 1;
    while count != 0 {
        let next = boundary + step;
        if count == 1 || !next.is_finite() || next == boundary {
            return next;
        }
        let a = boundary.to_bits();
        let b = next.to_bits();
        let c = (next + step).to_bits();
        if a & !FRACTION == b & !FRACTION && b & !FRACTION == c & !FRACTION {
            let stride = a.abs_diff(b);
            if stride != 0 && b.abs_diff(c) == stride {
                let negative = boundary < 0.0;
                let room = if negative {
                    a & FRACTION
                } else {
                    FRACTION - (a & FRACTION)
                };
                // Leave the boundary itself to a real addition (the neighbouring
                // exponent may have another spacing).
                let steps = count.min(room.saturating_sub(1) / stride);
                if steps != 0 {
                    let distance = steps * stride;
                    boundary = f64::from_bits(if negative { a - distance } else { a + distance });
                    count -= steps;
                    continue;
                }
            }
        }
        boundary = next;
        count -= 1;
    }
    boundary
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_traversal_preserves_corner_order_after_skipping_an_empty_prefix() {
        let bounds = BlockTraversalBounds {
            min: [0, 0, 0],
            max: [1, 1, 1],
        };
        for (start, end, expected) in [
            (
                Vec3::new(-63.5, -63.5, -63.5),
                Vec3::new(9.5, 9.5, 9.5),
                &[[0, 0, 0], [0, 0, 1], [0, 1, 1], [1, 1, 1]][..],
            ),
            (
                Vec3::new(63.5, 63.5, 63.5),
                Vec3::new(-9.5, -9.5, -9.5),
                &[[1, 1, 1], [1, 1, 0], [1, 0, 0], [0, 0, 0]][..],
            ),
            (
                Vec3::new(0.0, 0.5, -8.5),
                Vec3::new(0.0, 0.5, 3.5),
                &[[0, 0, 0], [0, 0, 1]][..],
            ),
        ] {
            let mut actual = Vec::new();
            traverse_blocks_in_bounds(start, end, bounds, |p| {
                actual.push([p.x, p.y, p.z]);
                None::<()>
            })
            .unwrap();
            assert_eq!(actual.as_slice(), expected);
        }
    }

    #[test]
    fn corner_order_and_early_exit() {
        let mut visited = Vec::new();
        assert_eq!(
            traverse_blocks::<()>(
                Vec3::new(0.5, 0.5, 0.5),
                Vec3::new(1.5, 1.5, 1.5),
                10,
                |pos| {
                    visited.push(pos);
                    None
                }
            ),
            Ok(None)
        );
        assert_eq!(
            visited,
            vec![
                BlockPos::new(0, 0, 0),
                BlockPos::new(0, 0, 1),
                BlockPos::new(0, 1, 1),
                BlockPos::new(1, 1, 1)
            ]
        );
        let result = traverse_blocks(
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::new(1.5, 1.5, 1.5),
            10,
            |pos| (pos.z == 1).then_some(pos),
        );
        assert_eq!(result, Ok(Some(BlockPos::new(0, 0, 1))));
    }
    #[test]
    fn miss_zero_identity_and_budget_errors() {
        let mut count = 0;
        assert_eq!(
            traverse_blocks::<()>(Vec3::ZERO, Vec3::ZERO, 0, |_| panic!(
                "no visits for equal endpoints"
            )),
            Ok(None)
        );
        assert_eq!(
            traverse_blocks::<()>(Vec3::new(-0.0, 0.0, 0.0), Vec3::ZERO, 10, |_| {
                count += 1;
                None
            }),
            Ok(None)
        );
        assert_eq!(count, 4);
        assert_eq!(
            traverse_blocks::<()>(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0), 1, |_| None),
            Err(TraversalError::VisitLimit)
        );
        assert_eq!(
            traverse_blocks::<()>(Vec3::ZERO, Vec3::new(f64::NAN, 1.0, 1.0), 1, |_| None),
            Err(TraversalError::NonFinite)
        );
    }
}
