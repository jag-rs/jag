//! Linear gradient fill of an axis-aligned rect along any gradient line.

use crate::scene::{Rect, Transform2D};

use super::gradients::sample_gradient_stops;
use super::types::Vertex;
use super::verts::apply_transform;

/// Fill `rect` with a linear gradient running from `start` to `end` (local
/// coordinates, before `t`). `stops` must be sorted with positions in [0, 1].
///
/// Each stop-to-stop span is the rect clipped to the band between the two
/// stops' perpendicular lines. Color is affine inside a band, so per-vertex
/// colors reproduce the gradient exactly. Past the first and last stops,
/// flat bands paint the end colors, as CSS does.
pub(crate) fn push_rect_linear_gradient(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rect: Rect,
    start: [f32; 2],
    end: [f32; 2],
    stops: &[(f32, [f32; 4])],
    t: Transform2D,
    z: f32,
) {
    if stops.len() < 2 {
        return;
    }
    let axis = [end[0] - start[0], end[1] - start[1]];
    let len_sq = axis[0] * axis[0] + axis[1] * axis[1];
    if len_sq <= f32::EPSILON {
        return;
    }
    // Position of `p` along the gradient line, 0 at `start` and 1 at `end`.
    let along = |p: [f32; 2]| ((p[0] - start[0]) * axis[0] + (p[1] - start[1]) * axis[1]) / len_sq;
    let corners = [
        [rect.x, rect.y],
        [rect.x + rect.w, rect.y],
        [rect.x + rect.w, rect.y + rect.h],
        [rect.x, rect.y + rect.h],
    ];
    // One band per stop span, plus a flat band before the first stop and after
    // the last: color is affine inside each, so per-vertex colors are exact.
    let limits: Vec<f32> = std::iter::once(f32::NEG_INFINITY)
        .chain(stops.iter().map(|stop| stop.0))
        .chain(std::iter::once(f32::INFINITY))
        .collect();
    for pair in limits.windows(2) {
        let (lo, hi) = (pair[0], pair[1]);
        if hi <= lo {
            continue;
        }
        let band = clip(&clip(&corners, &along, lo, true), &along, hi, false);
        if band.len() < 3 {
            continue;
        }
        let base = vertices.len() as u16;
        for p in &band {
            vertices.push(Vertex {
                pos: apply_transform(*p, t),
                color: sample_gradient_stops(stops, along(*p)),
                z_index: z,
            });
        }
        for k in 1..band.len() as u16 - 1 {
            indices.extend_from_slice(&[base, base + k, base + k + 1]);
        }
    }
}

/// Keep the part of convex `poly` where `along(p) >= limit` (`keep_above`) or
/// `along(p) <= limit` (Sutherland–Hodgman against one line).
fn clip(
    poly: &[[f32; 2]],
    along: &impl Fn([f32; 2]) -> f32,
    limit: f32,
    keep_above: bool,
) -> Vec<[f32; 2]> {
    if !limit.is_finite() {
        return poly.to_vec();
    }
    let side = |p: [f32; 2]| {
        let d = along(p) - limit;
        if keep_above { d } else { -d }
    };
    let mut out = Vec::with_capacity(poly.len() + 2);
    for (i, &a) in poly.iter().enumerate() {
        let b = poly[(i + 1) % poly.len()];
        let (da, db) = (side(a), side(b));
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let k = da / (da - db);
            out.push([a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k]);
        }
    }
    out
}

#[cfg(test)]
#[path = "rect_gradient_tests.rs"]
mod tests;
