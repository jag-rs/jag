use crate::scene::{Rect, Transform2D};

use super::types::Vertex;

pub(crate) fn apply_transform(p: [f32; 2], t: Transform2D) -> [f32; 2] {
    let [a, b, c, d, e, f] = t.m;
    [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
}

pub(crate) fn rect_to_verts(
    rect: Rect,
    color: [f32; 4],
    t: Transform2D,
    z: f32,
) -> ([Vertex; 4], [u16; 6]) {
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.w;
    let y1 = rect.y + rect.h;
    let p0 = apply_transform([x0, y0], t);
    let p1 = apply_transform([x1, y0], t);
    let p2 = apply_transform([x1, y1], t);
    let p3 = apply_transform([x0, y1], t);
    (
        [
            Vertex {
                pos: p0,
                color,
                z_index: z,
            },
            Vertex {
                pos: p1,
                color,
                z_index: z,
            },
            Vertex {
                pos: p2,
                color,
                z_index: z,
            },
            Vertex {
                pos: p3,
                color,
                z_index: z,
            },
        ],
        [0, 1, 2, 0, 2, 3],
    )
}
