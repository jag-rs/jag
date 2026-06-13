use crate::scene::{Rect, RoundedRect, Stroke, Transform2D};

use super::tessellate::{rounded_rect_to_path, tessellate_path_fill, tessellate_path_stroke};
use super::types::Vertex;
use super::verts::apply_transform;

pub(crate) fn push_rounded_rect(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    // Delegate to lyon's robust tessellator via our generic path fill
    let path = rounded_rect_to_path(rrect);
    tessellate_path_fill(vertices, indices, &path, color, z, t, None);
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
