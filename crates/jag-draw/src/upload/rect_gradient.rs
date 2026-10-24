//! Linear gradient fill of a convex shape (rects and rounded rects) along any
//! gradient line.

use crate::scene::{Rect, Transform2D};

use super::gradients::{lerp_color, linear_to_srgb};
use super::types::Vertex;
use super::verts::apply_transform;

/// Half an 8-bit output step: a color error below this cannot change what an
/// 8-bit target displays.
const HALF_OUTPUT_STEP: f32 = 0.5 / 255.0;

/// Fill `rect` with a linear gradient running from `start` to `end` (local
/// coordinates, before `t`). `stops` must be sorted with positions in [0, 1].
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
    let corners = [
        [rect.x, rect.y],
        [rect.x + rect.w, rect.y],
        [rect.x + rect.w, rect.y + rect.h],
        [rect.x, rect.y + rect.h],
    ];
    push_convex_linear_gradient(vertices, indices, &corners, start, end, stops, t, z);
}

/// Fill convex `poly` with a linear gradient running from `start` to `end`.
///
/// The shape is cut into bands between lines perpendicular to the gradient.
/// The GPU blends vertex colors in linear light, but CSS gradients blend in
/// sRGB, so each stop span is split until linear blending stays within
/// [`HALF_OUTPUT_STEP`] of the sRGB color. Past the first and last stops,
/// flat bands paint the end colors, as CSS does.
#[allow(clippy::too_many_arguments)]
pub(crate) fn push_convex_linear_gradient(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    poly: &[[f32; 2]],
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
    // Each band is `(lo, hi, c0, c1)`: its limits along the line and the colors
    // of the stop span it lies in. A band takes its colors from its own span,
    // so two stops at one position (a hard stop) switch color at that line.
    let (first, last) = (stops[0], stops[stops.len() - 1]);
    let mut bands = vec![(f32::NEG_INFINITY, first.0, first, first)];
    for pair in stops.windows(2) {
        let (s0, s1) = (pair[0], pair[1]);
        if s1.0 <= s0.0 {
            continue;
        }
        let mut lo = s0.0;
        for f in srgb_split_points(s0.1, s1.1)
            .into_iter()
            .chain(std::iter::once(1.0))
        {
            let hi = s0.0 + (s1.0 - s0.0) * f;
            bands.push((lo, hi, s0, s1));
            lo = hi;
        }
    }
    bands.push((last.0, f32::INFINITY, last, last));

    for (lo, hi, (t0, c0), (t1, c1)) in bands {
        if hi <= lo {
            continue;
        }
        let band = clip(&clip(poly, &along, lo, true), &along, hi, false);
        if band.len() < 3 {
            continue;
        }
        let color_at = |p: [f32; 2]| {
            if t1 > t0 {
                lerp_color(c0, c1, ((along(p) - t0) / (t1 - t0)).clamp(0.0, 1.0))
            } else {
                c0
            }
        };
        let base = vertices.len() as u16;
        for p in &band {
            vertices.push(Vertex {
                pos: apply_transform(*p, t),
                color: color_at(*p),
                z_index: z,
            });
        }
        for k in 1..band.len() as u16 - 1 {
            indices.extend_from_slice(&[base, base + k, base + k + 1]);
        }
    }
}

/// Fractions in (0, 1), ascending, at which to split the span from `c0` to
/// `c1` so linear-light blending between neighbours matches the sRGB blend.
fn srgb_split_points(c0: [f32; 4], c1: [f32; 4]) -> Vec<f32> {
    fn refine(c0: [f32; 4], c1: [f32; 4], a: f32, b: f32, out: &mut Vec<f32>) {
        let mid = 0.5 * (a + b);
        if mid <= a || mid >= b {
            return;
        }
        let (ca, cb) = (lerp_color(c0, c1, a), lerp_color(c0, c1, b));
        let exact = lerp_color(c0, c1, mid);
        let error = (0..3)
            .map(|i| (linear_to_srgb(0.5 * (ca[i] + cb[i])) - linear_to_srgb(exact[i])).abs())
            .fold(0.0, f32::max);
        if error <= HALF_OUTPUT_STEP {
            return;
        }
        refine(c0, c1, a, mid, out);
        out.push(mid);
        refine(c0, c1, mid, b, out);
    }
    let mut out = Vec::new();
    refine(c0, c1, 0.0, 1.0, &mut out);
    out
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
