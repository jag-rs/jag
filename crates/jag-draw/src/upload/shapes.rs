use crate::scene::{Rect, RoundedRadii, RoundedRect, Stroke, Transform2D};

use super::tessellate::{rounded_rect_to_path, tessellate_path_fill, tessellate_path_stroke};
use super::types::Vertex;
use super::verts::apply_transform;

const ROUNDED_EDGE_AA: f32 = 0.5;
const ROUNDED_SEGMENTS_PER_CORNER: usize = 16;

pub(crate) fn push_rounded_rect(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    let path = rounded_rect_to_path(rrect);
    tessellate_path_fill(vertices, indices, &path, color, z, t, None);
}

pub(crate) fn push_rounded_rect_aa(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    let rrect = normalize_rrect(rrect);
    if rrect.rect.w <= 0.0 || rrect.rect.h <= 0.0 {
        return;
    }

    let edge = rounded_rect_loop(rrect);
    if edge.len() < 3 {
        return;
    }

    let transparent = scale_color(color, 0.0);
    let expanded = expand_rrect(rrect, ROUNDED_EDGE_AA);
    let feather = rounded_rect_loop(expanded);

    if vertices.len() + 1 + edge.len() + feather.len() > u16::MAX as usize {
        return;
    }

    let center = [
        rrect.rect.x + rrect.rect.w * 0.5,
        rrect.rect.y + rrect.rect.h * 0.5,
    ];
    let center_index = push_vertex(vertices, center, color, z, t);
    let edge_base = push_loop(vertices, &edge, color, z, t);

    for i in 0..edge.len() {
        let next = (i + 1) % edge.len();
        indices.extend_from_slice(&[center_index, edge_base + i as u16, edge_base + next as u16]);
    }

    append_loop_band(vertices, indices, &edge, &feather, color, transparent, z, t);
}

pub(crate) fn push_rect_stroke(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rect: Rect,
    stroke: Stroke,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    let w = stroke.width.max(0.0);
    if w <= 0.0001 {
        return;
    }

    // Analytic-style AA for thin 1px strokes:
    // fade the outer edge of the stroke to transparent so the border
    // blends smoothly against the background instead of hard-stepping.
    let use_aa = w <= 1.5;
    let (outer_color, inner_color) = if use_aa {
        let mut oc = [0.0; 4];
        // Scale premultiplied RGBA by a small factor for the outer edge.
        // Using 0.0 gives the sharpest falloff; tweakable if needed.
        let scale = 0.0f32;
        oc[0] = color[0] * scale;
        oc[1] = color[1] * scale;
        oc[2] = color[2] * scale;
        oc[3] = color[3] * scale;
        (oc, color)
    } else {
        (color, color)
    };

    // Outer corners
    let o0 = apply_transform([rect.x, rect.y], t);
    let o1 = apply_transform([rect.x + rect.w, rect.y], t);
    let o2 = apply_transform([rect.x + rect.w, rect.y + rect.h], t);
    let o3 = apply_transform([rect.x, rect.y + rect.h], t);
    // Inner corners (shrink by width)
    let ix0 = rect.x + w;
    let iy0 = rect.y + w;
    let ix1 = (rect.x + rect.w - w).max(ix0);
    let iy1 = (rect.y + rect.h - w).max(iy0);
    let i0 = apply_transform([ix0, iy0], t);
    let i1 = apply_transform([ix1, iy0], t);
    let i2 = apply_transform([ix1, iy1], t);
    let i3 = apply_transform([ix0, iy1], t);

    let base = vertices.len() as u16;
    vertices.extend_from_slice(&[
        Vertex {
            pos: o0,
            color: outer_color,
            z_index: z,
        }, // 0
        Vertex {
            pos: o1,
            color: outer_color,
            z_index: z,
        }, // 1
        Vertex {
            pos: o2,
            color: outer_color,
            z_index: z,
        }, // 2
        Vertex {
            pos: o3,
            color: outer_color,
            z_index: z,
        }, // 3
        Vertex {
            pos: i0,
            color: inner_color,
            z_index: z,
        }, // 4
        Vertex {
            pos: i1,
            color: inner_color,
            z_index: z,
        }, // 5
        Vertex {
            pos: i2,
            color: inner_color,
            z_index: z,
        }, // 6
        Vertex {
            pos: i3,
            color: inner_color,
            z_index: z,
        }, // 7
    ]);
    // Build ring from quads on each edge
    let idx: [u16; 24] = [
        // top edge: o0-o1-i1-i0
        0, 1, 5, 0, 5, 4, // right edge: o1-o2-i2-i1
        1, 2, 6, 1, 6, 5, // bottom edge: o2-o3-i3-i2
        2, 3, 7, 2, 7, 6, // left edge: o3-o0-i0-i3
        3, 0, 4, 3, 4, 7,
    ];
    indices.extend(idx.iter().map(|i| base + i));
}

pub(crate) fn push_rounded_rect_stroke(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    stroke: Stroke,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    let w = stroke.width.max(0.0);
    if w <= 0.0001 {
        return;
    }

    let path = rounded_rect_to_path(rrect);
    tessellate_path_stroke(
        vertices,
        indices,
        &path,
        Stroke { width: w },
        color,
        z,
        t,
        None,
    );
}

pub(crate) fn push_rounded_rect_stroke_aa(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    stroke: Stroke,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    let w = stroke.width.max(0.0);
    if w <= 0.0001 {
        return;
    }

    let rrect = normalize_rrect(rrect);
    if rrect.rect.w <= 0.0 || rrect.rect.h <= 0.0 {
        return;
    }

    let inner = inset_rrect(rrect, w);
    if inner.rect.w <= 0.0 || inner.rect.h <= 0.0 {
        push_rounded_rect_aa(vertices, indices, rrect, color, z, t);
        return;
    }

    let outer_edge = rounded_rect_loop(rrect);
    let inner_edge = rounded_rect_loop(inner);
    if outer_edge.len() < 3 || inner_edge.len() != outer_edge.len() {
        return;
    }

    let outer_feather = rounded_rect_loop(expand_rrect(rrect, ROUNDED_EDGE_AA));
    let inner_feather = rounded_rect_loop(inset_rrect(inner, ROUNDED_EDGE_AA));
    let transparent = scale_color(color, 0.0);

    let needed = (outer_edge.len() + inner_edge.len() + outer_feather.len() + inner_feather.len())
        .saturating_mul(2);
    if vertices.len() + needed > u16::MAX as usize {
        return;
    }

    append_loop_band(
        vertices,
        indices,
        &outer_edge,
        &inner_edge,
        color,
        color,
        z,
        t,
    );
    append_loop_band(
        vertices,
        indices,
        &outer_edge,
        &outer_feather,
        color,
        transparent,
        z,
        t,
    );
    if inner_feather.len() == inner_edge.len()
        && inner.rect.w > ROUNDED_EDGE_AA * 2.0
        && inner.rect.h > ROUNDED_EDGE_AA * 2.0
    {
        append_loop_band(
            vertices,
            indices,
            &inner_edge,
            &inner_feather,
            color,
            transparent,
            z,
            t,
        );
    }
}

fn normalize_rrect(rrect: RoundedRect) -> RoundedRect {
    let rect = Rect {
        x: rrect.rect.x,
        y: rrect.rect.y,
        w: rrect.rect.w.max(0.0),
        h: rrect.rect.h.max(0.0),
    };
    let max_radius = rect.w.min(rect.h) * 0.5;
    RoundedRect {
        rect,
        radii: RoundedRadii {
            tl: rrect.radii.tl.max(0.0).min(max_radius),
            tr: rrect.radii.tr.max(0.0).min(max_radius),
            br: rrect.radii.br.max(0.0).min(max_radius),
            bl: rrect.radii.bl.max(0.0).min(max_radius),
        },
    }
}

fn expand_rrect(rrect: RoundedRect, amount: f32) -> RoundedRect {
    normalize_rrect(RoundedRect {
        rect: Rect {
            x: rrect.rect.x - amount,
            y: rrect.rect.y - amount,
            w: rrect.rect.w + amount * 2.0,
            h: rrect.rect.h + amount * 2.0,
        },
        radii: RoundedRadii {
            tl: rrect.radii.tl + amount,
            tr: rrect.radii.tr + amount,
            br: rrect.radii.br + amount,
            bl: rrect.radii.bl + amount,
        },
    })
}

fn inset_rrect(rrect: RoundedRect, amount: f32) -> RoundedRect {
    if amount <= 0.0 {
        return rrect;
    }
    normalize_rrect(RoundedRect {
        rect: Rect {
            x: rrect.rect.x + amount,
            y: rrect.rect.y + amount,
            w: (rrect.rect.w - amount * 2.0).max(0.0),
            h: (rrect.rect.h - amount * 2.0).max(0.0),
        },
        radii: RoundedRadii {
            tl: (rrect.radii.tl - amount).max(0.0),
            tr: (rrect.radii.tr - amount).max(0.0),
            br: (rrect.radii.br - amount).max(0.0),
            bl: (rrect.radii.bl - amount).max(0.0),
        },
    })
}

fn rounded_rect_loop(rrect: RoundedRect) -> Vec<[f32; 2]> {
    let rect = rrect.rect;
    let radii = rrect.radii;
    let corners = [
        (
            [rect.x + radii.tl, rect.y + radii.tl],
            radii.tl,
            std::f32::consts::FRAC_PI_2,
            std::f32::consts::PI,
            [rect.x, rect.y],
        ),
        (
            [rect.x + radii.bl, rect.y + rect.h - radii.bl],
            radii.bl,
            std::f32::consts::PI,
            std::f32::consts::FRAC_PI_2 * 3.0,
            [rect.x, rect.y + rect.h],
        ),
        (
            [rect.x + rect.w - radii.br, rect.y + rect.h - radii.br],
            radii.br,
            std::f32::consts::FRAC_PI_2 * 3.0,
            std::f32::consts::TAU,
            [rect.x + rect.w, rect.y + rect.h],
        ),
        (
            [rect.x + rect.w - radii.tr, rect.y + radii.tr],
            radii.tr,
            0.0,
            std::f32::consts::FRAC_PI_2,
            [rect.x + rect.w, rect.y],
        ),
    ];

    let mut points = Vec::with_capacity(4 * (ROUNDED_SEGMENTS_PER_CORNER + 1));
    for (center, radius, start, end, fallback) in corners {
        for i in 0..=ROUNDED_SEGMENTS_PER_CORNER {
            if radius <= 0.0001 {
                points.push(fallback);
                continue;
            }
            let t = i as f32 / ROUNDED_SEGMENTS_PER_CORNER as f32;
            let angle = start + t * (end - start);
            points.push([
                center[0] + radius * angle.cos(),
                center[1] - radius * angle.sin(),
            ]);
        }
    }
    points
}

fn scale_color(color: [f32; 4], scale: f32) -> [f32; 4] {
    [
        color[0] * scale,
        color[1] * scale,
        color[2] * scale,
        color[3] * scale,
    ]
}

fn push_vertex(
    vertices: &mut Vec<Vertex>,
    pos: [f32; 2],
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) -> u16 {
    let index = vertices.len() as u16;
    vertices.push(Vertex {
        pos: apply_transform(pos, t),
        color,
        z_index: z,
    });
    index
}

fn push_loop(
    vertices: &mut Vec<Vertex>,
    points: &[[f32; 2]],
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) -> u16 {
    let base = vertices.len() as u16;
    for point in points {
        vertices.push(Vertex {
            pos: apply_transform(*point, t),
            color,
            z_index: z,
        });
    }
    base
}

#[allow(clippy::too_many_arguments)]
fn append_loop_band(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    a: &[[f32; 2]],
    b: &[[f32; 2]],
    color_a: [f32; 4],
    color_b: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    if a.len() < 3 || a.len() != b.len() || vertices.len() + a.len() + b.len() > u16::MAX as usize {
        return;
    }

    let a_base = push_loop(vertices, a, color_a, z, t);
    let b_base = push_loop(vertices, b, color_b, z, t);
    for i in 0..a.len() {
        let next = (i + 1) % a.len();
        indices.extend_from_slice(&[
            a_base + i as u16,
            a_base + next as u16,
            b_base + next as u16,
            a_base + i as u16,
            b_base + next as u16,
            b_base + i as u16,
        ]);
    }
}
