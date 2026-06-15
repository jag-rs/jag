use crate::scene::{Rect, RoundedRect, Transform2D};

use super::tessellate::{
    rounded_rect_to_path, tessellate_path_fill_subdivided_with_color_fn,
    tessellate_path_fill_with_color_fn,
};
use super::types::Vertex;
use super::verts::apply_transform;

pub(crate) fn push_rect_linear_gradient(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rect: Rect,
    stops: &[(f32, [f32; 4])],
    t: Transform2D,
    z: f32,
) {
    if stops.len() < 2 {
        return;
    }
    // ensure sorted
    let mut s = stops.to_vec();
    s.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let y0 = rect.y;
    let y1 = rect.y + rect.h;
    for pair in s.windows(2) {
        let (t0, c0) = (pair[0].0.clamp(0.0, 1.0), pair[0].1);
        let (t1, c1) = (pair[1].0.clamp(0.0, 1.0), pair[1].1);
        if (t1 - t0).abs() < 1e-6 {
            continue;
        }
        let x0 = rect.x + rect.w * t0;
        let x1 = rect.x + rect.w * t1;
        let p0 = apply_transform([x0, y0], t);
        let p1 = apply_transform([x1, y0], t);
        let p2 = apply_transform([x1, y1], t);
        let p3 = apply_transform([x0, y1], t);
        let base = vertices.len() as u16;
        vertices.extend_from_slice(&[
            Vertex {
                pos: p0,
                color: c0,
                z_index: z,
            },
            Vertex {
                pos: p1,
                color: c1,
                z_index: z,
            },
            Vertex {
                pos: p2,
                color: c1,
                z_index: z,
            },
            Vertex {
                pos: p3,
                color: c0,
                z_index: z,
            },
        ]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn normalize_gradient_stops(stops: &[(f32, [f32; 4])]) -> Vec<(f32, [f32; 4])> {
    if stops.is_empty() {
        return Vec::new();
    }
    let mut out = stops.to_vec();
    out.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    if out.first().map(|s| s.0).unwrap_or(0.0) > 0.0
        && let Some((_, c)) = out.first().copied()
    {
        out.insert(0, (0.0, c));
    }
    if out.last().map(|s| s.0).unwrap_or(1.0) < 1.0
        && let Some((_, c)) = out.last().copied()
    {
        out.push((1.0, c));
    }
    for stop in &mut out {
        stop.0 = stop.0.clamp(0.0, 1.0);
    }
    out
}

fn linear_to_srgb(c: f32) -> f32 {
    let x = c.clamp(0.0, 1.0);
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    let x = c.clamp(0.0, 1.0);
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

pub fn lerp_color(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    // CSS gradients interpolate in sRGB by default.
    let ar = linear_to_srgb(a[0]);
    let ag = linear_to_srgb(a[1]);
    let ab = linear_to_srgb(a[2]);
    let br = linear_to_srgb(b[0]);
    let bg = linear_to_srgb(b[1]);
    let bb = linear_to_srgb(b[2]);

    let rr = ar + (br - ar) * t;
    let rg = ag + (bg - ag) * t;
    let rb = ab + (bb - ab) * t;

    [
        srgb_to_linear(rr),
        srgb_to_linear(rg),
        srgb_to_linear(rb),
        a[3] + (b[3] - a[3]) * t,
    ]
}

pub fn sample_gradient_stops(stops: &[(f32, [f32; 4])], t: f32) -> [f32; 4] {
    if stops.is_empty() {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let tc = t.clamp(0.0, 1.0);
    if tc <= stops[0].0 {
        return stops[0].1;
    }
    if tc >= stops[stops.len() - 1].0 {
        return stops[stops.len() - 1].1;
    }

    for pair in stops.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if tc >= t0 && tc <= t1 {
            let span = (t1 - t0).max(1e-6);
            let local_t = ((tc - t0) / span).clamp(0.0, 1.0);
            return lerp_color(c0, c1, local_t);
        }
    }
    stops[stops.len() - 1].1
}

pub(crate) fn push_ellipse(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    center: [f32; 2],
    radii: [f32; 2],
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    let segs = 64u32;
    let base = vertices.len() as u16;
    let c = apply_transform(center, t);
    vertices.push(Vertex {
        pos: c,
        color,
        z_index: z,
    });

    for i in 0..segs {
        let theta = (i as f32) / (segs as f32) * std::f32::consts::TAU;
        let p = [
            center[0] + radii[0] * theta.cos(),
            center[1] + radii[1] * theta.sin(),
        ];
        let p = apply_transform(p, t);
        vertices.push(Vertex {
            pos: p,
            color,
            z_index: z,
        });
    }
    for i in 0..segs {
        let i0 = base;
        let i1 = base + 1 + i as u16;
        let i2 = base + 1 + ((i + 1) % segs) as u16;
        indices.extend_from_slice(&[i0, i1, i2]);
    }
}

pub(crate) fn push_ellipse_radial_gradient(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    center: [f32; 2],
    radii: [f32; 2],
    stops: &[(f32, [f32; 4])],
    z: f32,
    t: Transform2D,
) {
    if stops.len() < 2 {
        return;
    }
    let mut s = stops.to_vec();
    s.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let segs = 64u32;
    let base_center = vertices.len() as u16;
    // Center vertex with first stop color
    let cpos = apply_transform(center, t);
    vertices.push(Vertex {
        pos: cpos,
        color: s[0].1,
        z_index: z,
    });

    // First ring
    let mut prev_ring_start = vertices.len() as u16;
    let prev_color = s[0].1;
    let prev_t0 = s[0].0.clamp(0.0, 1.0);
    let prev_t = if prev_t0 <= 0.0 { 0.0 } else { prev_t0 };
    for i in 0..segs {
        let theta = (i as f32) / (segs as f32) * std::f32::consts::TAU;
        let p = [
            center[0] + radii[0] * prev_t * theta.cos(),
            center[1] + radii[1] * prev_t * theta.sin(),
        ];
        let p = apply_transform(p, t);
        vertices.push(Vertex {
            pos: p,
            color: prev_color,
            z_index: z,
        });
    }
    // Connect center to first ring if needed
    if prev_t == 0.0 {
        for i in 0..segs {
            let i1 = base_center;
            let i2 = prev_ring_start + i as u16;
            let i3 = prev_ring_start + ((i + 1) % segs) as u16;
            indices.extend_from_slice(&[i1, i2, i3]);
        }
    }

    for si in 1..s.len() {
        let (tcur, ccur) = (s[si].0.clamp(0.0, 1.0), s[si].1);
        let ring_start = vertices.len() as u16;
        for i in 0..segs {
            let theta = (i as f32) / (segs as f32) * std::f32::consts::TAU;
            let p = [
                center[0] + radii[0] * tcur * theta.cos(),
                center[1] + radii[1] * tcur * theta.sin(),
            ];
            let p = apply_transform(p, t);
            vertices.push(Vertex {
                pos: p,
                color: ccur,
                z_index: z,
            });
        }
        // stitch prev ring to current ring
        for i in 0..segs {
            let a0 = prev_ring_start + i as u16;
            let a1 = prev_ring_start + ((i + 1) % segs) as u16;
            let b0 = ring_start + i as u16;
            let b1 = ring_start + ((i + 1) % segs) as u16;
            indices.extend_from_slice(&[a0, b0, b1, a0, b1, a1]);
        }
        prev_ring_start = ring_start;
    }
}

pub(crate) fn push_rounded_rect_linear_gradient(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    start: [f32; 2],
    end: [f32; 2],
    stops: &[(f32, [f32; 4])],
    z: f32,
    t: Transform2D,
) {
    let packed = normalize_gradient_stops(stops);
    if packed.len() < 2 {
        return;
    }

    let path = rounded_rect_to_path(rrect);
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    let denom = (dx * dx + dy * dy).max(1e-6);
    let color_at = |p: [f32; 2]| {
        let proj = ((p[0] - start[0]) * dx + (p[1] - start[1]) * dy) / denom;
        sample_gradient_stops(&packed, proj)
    };

    // The path-fill triangulation samples gradient color only at its (coarse)
    // vertices and interpolates linearly across triangles. A 2-3 stop gradient
    // is smooth enough for that, but a many-stop gradient (e.g. CSS
    // `background-repeat` tiling of a gradient into a border ring) has sharp
    // transitions the coarse rounded-corner triangulation cannot follow,
    // producing diagonal seams. Subdivide so color is sampled densely enough to
    // track the stops. Gated on stop count to keep smooth gradients cheap.
    if packed.len() > 3 {
        tessellate_path_fill_subdivided_with_color_fn(
            vertices, indices, &path, z, t, 1.5, color_at,
        );
    } else {
        tessellate_path_fill_with_color_fn(vertices, indices, &path, z, t, None, color_at);
    }
}

pub(crate) fn push_rounded_rect_radial_gradient(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    center: [f32; 2],
    radius: f32,
    stops: &[(f32, [f32; 4])],
    z: f32,
    t: Transform2D,
) {
    let packed = normalize_gradient_stops(stops);
    if packed.len() < 2 {
        return;
    }

    let path = rounded_rect_to_path(rrect);
    let r = radius.abs().max(1e-6);
    tessellate_path_fill_subdivided_with_color_fn(vertices, indices, &path, z, t, 6.0, |p| {
        let dx = p[0] - center[0];
        let dy = p[1] - center[1];
        let dist = (dx * dx + dy * dy).sqrt();
        sample_gradient_stops(&packed, dist / r)
    });
}

/// Tessellate a conic gradient filling a rectangle using angular wedge sectors.
/// `start_angle` is in radians (CSS convention: 0 = up/north, clockwise).
pub(crate) fn push_rect_conic_gradient(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rect: Rect,
    center: [f32; 2],
    start_angle: f32,
    stops: &[(f32, [f32; 4])],
    z: f32,
    t: Transform2D,
) {
    let packed = normalize_gradient_stops(stops);
    if packed.len() < 2 {
        return;
    }
    let segs = 128u32;
    let base_center = vertices.len() as u16;
    // Center vertex colored with first stop
    let cpos = apply_transform(center, t);
    vertices.push(Vertex {
        pos: cpos,
        color: packed[0].1,
        z_index: z,
    });

    // Compute max distance from center to any corner for the outer radius
    let corners = [
        [rect.x, rect.y],
        [rect.x + rect.w, rect.y],
        [rect.x + rect.w, rect.y + rect.h],
        [rect.x, rect.y + rect.h],
    ];
    let max_r = corners
        .iter()
        .map(|c| {
            let dx = c[0] - center[0];
            let dy = c[1] - center[1];
            (dx * dx + dy * dy).sqrt()
        })
        .fold(0.0f32, f32::max)
        * 1.5; // extend to cover the full rect after clipping

    // Emit a triangle fan: center → rim vertex i → rim vertex i+1
    // Each rim vertex is at angle (i/segs * TAU + start_angle), colored by gradient t
    let tau = std::f32::consts::TAU;
    for i in 0..=segs {
        let frac = (i as f32) / (segs as f32);
        // CSS conic: 0 = top (north), goes clockwise
        // atan2 convention: 0 = right, counter-clockwise
        // So angle = start_angle + frac * TAU, where 0 = north, clockwise
        let angle = start_angle + frac * tau;
        // Convert to standard math: north=up means -PI/2, clockwise means negate
        // x = sin(angle), y = -cos(angle) gives north-up clockwise
        let px = center[0] + max_r * angle.sin();
        let py = center[1] - max_r * angle.cos();
        let p = apply_transform([px, py], t);
        let color = sample_gradient_stops(&packed, frac);
        vertices.push(Vertex {
            pos: p,
            color,
            z_index: z,
        });
    }

    // Triangle fan indices
    for i in 0..segs {
        let i0 = base_center;
        let i1 = base_center + 1 + i as u16;
        let i2 = base_center + 1 + (i + 1) as u16;
        indices.extend_from_slice(&[i0, i1, i2]);
    }
}

/// Tessellate a conic gradient filling a rounded rectangle.
pub(crate) fn push_rounded_rect_conic_gradient(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    center: [f32; 2],
    start_angle: f32,
    stops: &[(f32, [f32; 4])],
    z: f32,
    t: Transform2D,
) {
    let packed = normalize_gradient_stops(stops);
    if packed.len() < 2 {
        return;
    }

    let path = rounded_rect_to_path(rrect);
    let tau = std::f32::consts::TAU;
    tessellate_path_fill_subdivided_with_color_fn(vertices, indices, &path, z, t, 6.0, |p| {
        let dx = p[0] - center[0];
        let dy = p[1] - center[1];
        // atan2(dx, -dy) gives angle from north, clockwise
        let angle = dx.atan2(-dy) - start_angle;
        // Normalize to [0, 1)
        let frac = ((angle % tau) + tau) % tau / tau;
        sample_gradient_stops(&packed, frac)
    });
}
