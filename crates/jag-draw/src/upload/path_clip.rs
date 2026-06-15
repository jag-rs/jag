//! CPU clipping of tessellated path triangles against a clip region.
//!
//! Arbitrary filled/stroked paths have no per-draw GPU scissor in the batched
//! solid pass, so a path that straddles a clip boundary is cut here, on the CPU,
//! before transform — Sutherland–Hodgman against the clip's half-planes. Both an
//! axis-aligned rect and a rounded rect (CSS `border-radius` + `overflow:hidden`
//! around an inline SVG `<path>`) are supported; the rounded boundary is
//! approximated by chording each corner arc and clipping against the resulting
//! convex polygon.

use crate::scene::Rect;

/// Number of chord segments per rounded corner. 8 keeps the arc within ~0.1px of
/// the true circle at the radii these clips use (≤ ~40px), invisible after AA.
const CORNER_SEGMENTS: usize = 8;

/// Clip a polygon against one axis-aligned bound on `axis` (0 = x, 1 = y):
/// `keep_ge` keeps points whose coord is `>= v`, else `<= v`. Sutherland–
/// Hodgman, interpolating the crossing on the other axis.
fn clip_poly_axis(poly: &[[f32; 2]], axis: usize, v: f32, keep_ge: bool) -> Vec<[f32; 2]> {
    if poly.is_empty() {
        return Vec::new();
    }
    let inside = |p: &[f32; 2]| if keep_ge { p[axis] >= v } else { p[axis] <= v };
    let other = axis ^ 1;
    let intersect = |a: &[f32; 2], b: &[f32; 2]| {
        let denom = b[axis] - a[axis];
        let s = if denom.abs() < 1e-6 {
            0.0
        } else {
            (v - a[axis]) / denom
        };
        let mut p = [0.0f32; 2];
        p[axis] = v;
        p[other] = a[other] + s * (b[other] - a[other]);
        p
    };
    let mut out = Vec::with_capacity(poly.len() + 1);
    let n = poly.len();
    for i in 0..n {
        let cur = poly[i];
        let prev = poly[(i + n - 1) % n];
        let (cin, pin) = (inside(&cur), inside(&prev));
        if cin {
            if !pin {
                out.push(intersect(&prev, &cur));
            }
            out.push(cur);
        } else if pin {
            out.push(intersect(&prev, &cur));
        }
    }
    out
}

/// Clip a triangle to an axis-aligned rect, returning the convex polygon
/// (3–7 verts) in the same local space, or empty if fully outside.
pub(crate) fn clip_triangle_to_rect(tri: [[f32; 2]; 3], r: Rect) -> Vec<[f32; 2]> {
    let mut poly = tri.to_vec();
    poly = clip_poly_axis(&poly, 0, r.x, true);
    poly = clip_poly_axis(&poly, 0, r.x + r.w, false);
    poly = clip_poly_axis(&poly, 1, r.y, true);
    poly = clip_poly_axis(&poly, 1, r.y + r.h, false);
    poly
}

/// Clip a convex `poly` to the half-plane on the `inside_ref` side of the line
/// through `a`→`b`. Sutherland–Hodgman; the inside side is determined by the
/// reference point's sign so winding direction doesn't matter.
fn clip_poly_halfplane(
    poly: &[[f32; 2]],
    a: [f32; 2],
    b: [f32; 2],
    inside_ref: [f32; 2],
) -> Vec<[f32; 2]> {
    if poly.is_empty() {
        return Vec::new();
    }
    // Signed side of point `p` relative to the directed line a→b.
    let side = |p: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let ref_side = side(inside_ref);
    if ref_side.abs() < 1e-9 {
        // Degenerate edge (reference on the line) — no constraint.
        return poly.to_vec();
    }
    let inside = |p: [f32; 2]| side(p) * ref_side >= 0.0;
    let intersect = |p0: [f32; 2], p1: [f32; 2]| {
        let s0 = side(p0);
        let s1 = side(p1);
        let denom = s0 - s1;
        let t = if denom.abs() < 1e-9 { 0.0 } else { s0 / denom };
        [p0[0] + t * (p1[0] - p0[0]), p0[1] + t * (p1[1] - p0[1])]
    };
    let mut out = Vec::with_capacity(poly.len() + 1);
    let n = poly.len();
    for i in 0..n {
        let cur = poly[i];
        let prev = poly[(i + n - 1) % n];
        let (cin, pin) = (inside(cur), inside(prev));
        if cin {
            if !pin {
                out.push(intersect(prev, cur));
            }
            out.push(cur);
        } else if pin {
            out.push(intersect(prev, cur));
        }
    }
    out
}

/// One rounded corner: the arc-center, radius, and angular sweep, plus the axis-
/// aligned `r×r` square it occupies (used to skip corners a triangle can't reach).
struct Corner {
    cx: f32,
    cy: f32,
    rad: f32,
    a0: f32,
    a1: f32,
    /// Corner square bounds [min_x, min_y, max_x, max_y] for bbox culling.
    square: [f32; 4],
}

/// The four corners of a rounded rect. Per-corner radii are `(tl, tr, br, bl)`,
/// clamped to half the shorter side; a zero-radius corner is skipped (its square
/// is already fully covered by the straight-edge clip).
fn corners(r: Rect, radii: [f32; 4]) -> Vec<Corner> {
    use std::f32::consts::PI;
    let max_r = (r.w.min(r.h) * 0.5).max(0.0);
    let (x0, y0, x1, y1) = (r.x, r.y, r.x + r.w, r.y + r.h);
    // (radius, center, angular sweep, corner-square origin) per corner. Screen
    // space is y-down; each arc sweeps a quarter turn around its center.
    let specs = [
        (radii[0], x0 + 0.0, y0 + 0.0, PI, 1.5 * PI, x0, y0), // tl square at (x0,y0)
        (radii[1], x1, y0, 1.5 * PI, 2.0 * PI, x1, y0),       // tr square at (x1,y0)
        (radii[2], x1, y1, 0.0, 0.5 * PI, x1, y1),            // br square at (x1,y1)
        (radii[3], x0, y1, 0.5 * PI, PI, x0, y1),             // bl square at (x0,y1)
    ];
    let mut out = Vec::with_capacity(4);
    for (rad, corner_x, corner_y, a0, a1, sq_x, sq_y) in specs {
        let rad = rad.clamp(0.0, max_r);
        if rad <= 0.0 {
            continue;
        }
        // Arc center is inset from the rect corner by `rad` on both axes.
        let cx = corner_x + (r.x + r.w * 0.5 - corner_x).signum() * rad;
        let cy = corner_y + (r.y + r.h * 0.5 - corner_y).signum() * rad;
        out.push(Corner {
            cx,
            cy,
            rad,
            a0,
            a1,
            square: [sq_x.min(cx), sq_y.min(cy), sq_x.max(cx), sq_y.max(cy)],
        });
    }
    out
}

/// Clip a triangle to a rounded rect, returning the convex polygon clipped
/// against the rect's straight edges and corner arcs (chorded). Empty if fully
/// outside. `radii` are `(tl, tr, br, bl)` in the same local space as `tri`/`r`.
pub(crate) fn clip_triangle_to_rounded(
    tri: [[f32; 2]; 3],
    r: Rect,
    radii: [f32; 4],
) -> Vec<[f32; 2]> {
    // Clip to the straight edges first — cheap, and rejects far-outside triangles
    // before the corner work.
    let mut poly = clip_triangle_to_rect(tri, r);
    if poly.len() < 3 {
        return poly;
    }
    // Rect center is interior to every corner-arc chord (convexity), so it is a
    // valid inside reference.
    let center = [r.x + r.w * 0.5, r.y + r.h * 0.5];
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for p in &poly {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
    }
    for c in corners(r, radii) {
        // Skip a corner the (rect-clipped) polygon can't reach — its straight-edge
        // clip already left it whole inside this corner's square.
        let [sx0, sy0, sx1, sy1] = c.square;
        if max_x <= sx0 || min_x >= sx1 || max_y <= sy0 || min_y >= sy1 {
            continue;
        }
        for i in 0..CORNER_SEGMENTS {
            let t0 = c.a0 + (c.a1 - c.a0) * (i as f32 / CORNER_SEGMENTS as f32);
            let t1 = c.a0 + (c.a1 - c.a0) * ((i + 1) as f32 / CORNER_SEGMENTS as f32);
            let a = [c.cx + c.rad * t0.cos(), c.cy + c.rad * t0.sin()];
            let b = [c.cx + c.rad * t1.cos(), c.cy + c.rad * t1.sin()];
            poly = clip_poly_halfplane(&poly, a, b, center);
            if poly.len() < 3 {
                return poly;
            }
        }
    }
    poly
}
