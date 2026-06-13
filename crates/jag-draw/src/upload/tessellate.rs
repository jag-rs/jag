use crate::scene::{FillRule, Path, PathCmd, Rect, RoundedRect, Stroke, Transform2D};

use super::types::Vertex;
use super::verts::apply_transform;

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

/// Append local-space tessellated geometry, transformed by `t`, optionally
/// clipping each triangle to `clip` (a rect in the same local space). Colors
/// come from `color_at(local_point)`, so clipped/interpolated vertices get the
/// correct (e.g. gradient) color.
fn append_tessellated<F>(
    out_v: &mut Vec<Vertex>,
    out_i: &mut Vec<u16>,
    geom_v: &[[f32; 2]],
    geom_i: &[u16],
    z: f32,
    t: Transform2D,
    clip: Option<Rect>,
    mut color_at: F,
) where
    F: FnMut([f32; 2]) -> [f32; 4],
{
    match clip {
        None => {
            if out_v.len() + geom_v.len() > u16::MAX as usize {
                return;
            }
            let base = out_v.len() as u16;
            for p in geom_v {
                out_v.push(Vertex {
                    pos: apply_transform(*p, t),
                    color: color_at(*p),
                    z_index: z,
                });
            }
            out_i.extend(geom_i.iter().map(|i| base + *i));
        }
        Some(r) => {
            for tri in geom_i.chunks_exact(3) {
                let poly = clip_triangle_to_rect(
                    [
                        geom_v[tri[0] as usize],
                        geom_v[tri[1] as usize],
                        geom_v[tri[2] as usize],
                    ],
                    r,
                );
                if poly.len() < 3 || out_v.len() + poly.len() > u16::MAX as usize {
                    continue;
                }
                let base = out_v.len() as u16;
                for p in &poly {
                    out_v.push(Vertex {
                        pos: apply_transform(*p, t),
                        color: color_at(*p),
                        z_index: z,
                    });
                }
                for k in 1..(poly.len() as u16 - 1) {
                    out_i.push(base);
                    out_i.push(base + k);
                    out_i.push(base + k + 1);
                }
            }
        }
    }
}

pub(crate) fn tessellate_path_fill(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    path: &Path,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
    clip: Option<Rect>,
) {
    tessellate_path_fill_with_color_fn(vertices, indices, path, z, t, clip, |_| color);
}

pub(crate) fn tessellate_path_fill_with_color_fn<F>(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    path: &Path,
    z: f32,
    t: Transform2D,
    clip: Option<Rect>,
    color_at: F,
) where
    F: FnMut([f32; 2]) -> [f32; 4],
{
    let Some(geom) = tessellate_path_fill_geometry(path) else {
        return;
    };
    append_tessellated(
        vertices,
        indices,
        &geom.vertices,
        &geom.indices,
        z,
        t,
        clip,
        color_at,
    );
}

pub(crate) fn tessellate_path_fill_subdivided_with_color_fn<F>(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    path: &Path,
    z: f32,
    t: Transform2D,
    max_edge: f32,
    mut color_at: F,
) where
    F: FnMut([f32; 2]) -> [f32; 4],
{
    let Some(geom) = tessellate_path_fill_geometry(path) else {
        return;
    };

    let max_edge = max_edge.max(1.0);
    for tri in geom.indices.chunks_exact(3) {
        let p0 = geom.vertices[tri[0] as usize];
        let p1 = geom.vertices[tri[1] as usize];
        let p2 = geom.vertices[tri[2] as usize];
        append_subdivided_triangle(vertices, indices, p0, p1, p2, z, t, max_edge, &mut color_at);
    }
}

fn tessellate_path_fill_geometry(
    path: &Path,
) -> Option<lyon_tessellation::VertexBuffers<[f32; 2], u16>> {
    use lyon_geom::point;
    use lyon_path::Path as LyonPath;
    use lyon_tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, VertexBuffers,
    };

    let mut builder = lyon_path::Path::builder();
    let mut started = false;
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::MoveTo(p) => {
                if started {
                    builder.end(false);
                }
                builder.begin(point(p[0], p[1]));
                started = true;
            }
            PathCmd::LineTo(p) => {
                if !started {
                    builder.begin(point(p[0], p[1]));
                    started = true;
                } else {
                    builder.line_to(point(p[0], p[1]));
                }
            }
            PathCmd::QuadTo(c, p) => {
                builder.quadratic_bezier_to(point(c[0], c[1]), point(p[0], p[1]));
            }
            PathCmd::CubicTo(c1, c2, p) => {
                builder.cubic_bezier_to(
                    point(c1[0], c1[1]),
                    point(c2[0], c2[1]),
                    point(p[0], p[1]),
                );
            }
            PathCmd::Close => {
                builder.end(true);
                started = false;
            }
        }
    }
    if started {
        builder.end(false);
    }

    let lyon_path: LyonPath = builder.build();
    let mut tess = FillTessellator::new();
    let tol = std::env::var("LYON_TOLERANCE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.1);
    let base_opts = FillOptions::default().with_tolerance(tol);
    let options = match path.fill_rule {
        FillRule::NonZero => base_opts.with_fill_rule(lyon_tessellation::FillRule::NonZero),
        FillRule::EvenOdd => base_opts.with_fill_rule(lyon_tessellation::FillRule::EvenOdd),
    };

    let mut geom: VertexBuffers<[f32; 2], u16> = VertexBuffers::new();
    let result = tess.tessellate_path(
        lyon_path.as_slice(),
        &options,
        &mut BuffersBuilder::new(&mut geom, |fv: FillVertex| {
            let p = fv.position();
            [p.x, p.y]
        }),
    );
    if result.is_err() {
        return None;
    }
    Some(geom)
}

fn append_subdivided_triangle<F>(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    z: f32,
    t: Transform2D,
    max_edge: f32,
    color_at: &mut F,
) where
    F: FnMut([f32; 2]) -> [f32; 4],
{
    let edge_len = |a: [f32; 2], b: [f32; 2]| -> f32 {
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        (dx * dx + dy * dy).sqrt()
    };
    let max_len = edge_len(p0, p1).max(edge_len(p1, p2)).max(edge_len(p2, p0));
    let steps = ((max_len / max_edge).ceil() as usize).clamp(1, 12);
    let steps_f = steps as f32;

    let base = vertices.len() as u16;
    let mut row_offsets = Vec::with_capacity(steps + 1);
    let mut offset = 0usize;
    for row in 0..=steps {
        row_offsets.push(offset);
        offset += steps - row + 1;
    }

    for i in 0..=steps {
        for j in 0..=(steps - i) {
            let a = i as f32 / steps_f;
            let b = j as f32 / steps_f;
            let c = 1.0 - a - b;
            let p = [
                p0[0] * c + p1[0] * a + p2[0] * b,
                p0[1] * c + p1[1] * a + p2[1] * b,
            ];
            vertices.push(Vertex {
                pos: apply_transform(p, t),
                color: color_at(p),
                z_index: z,
            });
        }
    }

    let tri_area = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
    let ccw = tri_area >= 0.0;
    let idx = |row: usize, col: usize| -> u16 { base + (row_offsets[row] + col) as u16 };

    for i in 0..steps {
        for j in 0..(steps - i) {
            let a = idx(i, j);
            let b = idx(i + 1, j);
            let c = idx(i, j + 1);

            if ccw {
                indices.extend_from_slice(&[a, b, c]);
            } else {
                indices.extend_from_slice(&[a, c, b]);
            }

            if j < (steps - i - 1) {
                let d = idx(i + 1, j + 1);
                if ccw {
                    indices.extend_from_slice(&[b, d, c]);
                } else {
                    indices.extend_from_slice(&[b, c, d]);
                }
            }
        }
    }
}

pub(crate) fn tessellate_path_stroke(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    path: &Path,
    stroke: Stroke,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
    clip: Option<Rect>,
) {
    use lyon_geom::point;
    use lyon_path::Path as LyonPath;
    use lyon_tessellation::{
        BuffersBuilder, LineCap, LineJoin, StrokeOptions, StrokeTessellator, StrokeVertex,
        VertexBuffers,
    };

    // Build lyon path
    let mut builder = lyon_path::Path::builder();
    let mut started = false;
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::MoveTo(p) => {
                if started {
                    builder.end(false);
                }
                builder.begin(point(p[0], p[1]));
                started = true;
            }
            PathCmd::LineTo(p) => {
                if !started {
                    builder.begin(point(p[0], p[1]));
                    started = true;
                } else {
                    builder.line_to(point(p[0], p[1]));
                }
            }
            PathCmd::QuadTo(c, p) => {
                builder.quadratic_bezier_to(point(c[0], c[1]), point(p[0], p[1]));
            }
            PathCmd::CubicTo(c1, c2, p) => {
                builder.cubic_bezier_to(
                    point(c1[0], c1[1]),
                    point(c2[0], c2[1]),
                    point(p[0], p[1]),
                );
            }
            PathCmd::Close => {
                builder.end(true);
                started = false;
            }
        }
    }
    // End any open sub-path
    if started {
        builder.end(false);
    }
    let lyon_path: LyonPath = builder.build();

    let mut tess = StrokeTessellator::new();
    let tol = std::env::var("LYON_TOLERANCE")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.1);
    let options = StrokeOptions::default()
        .with_line_width(stroke.width.max(0.0))
        .with_tolerance(tol)
        .with_line_join(LineJoin::Round)
        .with_start_cap(LineCap::Round)
        .with_end_cap(LineCap::Round);
    let mut geom: VertexBuffers<[f32; 2], u16> = VertexBuffers::new();
    let result = tess.tessellate_path(
        lyon_path.as_slice(),
        &options,
        &mut BuffersBuilder::new(&mut geom, |sv: StrokeVertex| {
            let p = sv.position();
            [p.x, p.y]
        }),
    );
    if result.is_err() {
        return;
    }
    append_tessellated(
        vertices,
        indices,
        &geom.vertices,
        &geom.indices,
        z,
        t,
        clip,
        |_| color,
    );
}

/// Build a Path representing a rounded rectangle using cubic Beziers (kappa approximation).
/// This path is then tessellated by lyon for precise coverage (avoids fan artifacts on small radii).
pub(crate) fn rounded_rect_to_path(rrect: RoundedRect) -> Path {
    let rect = rrect.rect;
    let mut tl = rrect.radii.tl.min(rect.w * 0.5).min(rect.h * 0.5);
    let mut tr = rrect.radii.tr.min(rect.w * 0.5).min(rect.h * 0.5);
    let mut br = rrect.radii.br.min(rect.w * 0.5).min(rect.h * 0.5);
    let mut bl = rrect.radii.bl.min(rect.w * 0.5).min(rect.h * 0.5);

    // Clamp negative or NaN just in case
    for r in [&mut tl, &mut tr, &mut br, &mut bl] {
        if !r.is_finite() || *r < 0.0 {
            *r = 0.0;
        }
    }

    // If radii are effectively zero, fall back to a plain rect path
    if tl <= 0.0 && tr <= 0.0 && br <= 0.0 && bl <= 0.0 {
        return Path {
            cmds: vec![
                PathCmd::MoveTo([rect.x, rect.y]),
                PathCmd::LineTo([rect.x + rect.w, rect.y]),
                PathCmd::LineTo([rect.x + rect.w, rect.y + rect.h]),
                PathCmd::LineTo([rect.x, rect.y + rect.h]),
                PathCmd::Close,
            ],
            fill_rule: FillRule::NonZero,
        };
    }

    // Kappa for quarter circle cubic approximation
    const K: f32 = 0.552_284_749_831;
    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.w;
    let y1 = rect.y + rect.h;

    // Start at top-left corner top edge tangent
    let mut cmds: Vec<PathCmd> = Vec::new();
    cmds.push(PathCmd::MoveTo([x0 + tl, y0]));

    // Top edge to before TR arc
    cmds.push(PathCmd::LineTo([x1 - tr, y0]));
    // TR arc (clockwise): from (x1 - tr, y0) to (x1, y0 + tr)
    if tr > 0.0 {
        let c1 = [x1 - tr + K * tr, y0];
        let c2 = [x1, y0 + tr - K * tr];
        let p = [x1, y0 + tr];
        cmds.push(PathCmd::CubicTo(c1, c2, p));
    } else {
        cmds.push(PathCmd::LineTo([x1, y0]));
        cmds.push(PathCmd::LineTo([x1, y0 + tr]));
    }

    // Right edge down to before BR arc
    cmds.push(PathCmd::LineTo([x1, y1 - br]));
    // BR arc: from (x1, y1 - br) to (x1 - br, y1)
    if br > 0.0 {
        let c1 = [x1, y1 - br + K * br];
        let c2 = [x1 - br + K * br, y1];
        let p = [x1 - br, y1];
        cmds.push(PathCmd::CubicTo(c1, c2, p));
    } else {
        cmds.push(PathCmd::LineTo([x1, y1]));
        cmds.push(PathCmd::LineTo([x1 - br, y1]));
    }

    // Bottom edge to before BL arc
    cmds.push(PathCmd::LineTo([x0 + bl, y1]));
    // BL arc: from (x0 + bl, y1) to (x0, y1 - bl)
    if bl > 0.0 {
        let c1 = [x0 + bl - K * bl, y1];
        let c2 = [x0, y1 - bl + K * bl];
        let p = [x0, y1 - bl];
        cmds.push(PathCmd::CubicTo(c1, c2, p));
    } else {
        cmds.push(PathCmd::LineTo([x0, y1]));
        cmds.push(PathCmd::LineTo([x0, y1 - bl]));
    }

    // Left edge up to before TL arc
    cmds.push(PathCmd::LineTo([x0, y0 + tl]));
    // TL arc: from (x0, y0 + tl) to (x0 + tl, y0)
    if tl > 0.0 {
        let c1 = [x0, y0 + tl - K * tl];
        let c2 = [x0 + tl - K * tl, y0];
        let p = [x0 + tl, y0];
        cmds.push(PathCmd::CubicTo(c1, c2, p));
    } else {
        cmds.push(PathCmd::LineTo([x0, y0]));
        cmds.push(PathCmd::LineTo([x0 + tl, y0]));
    }

    cmds.push(PathCmd::Close);
    Path {
        cmds,
        fill_rule: FillRule::NonZero,
    }
}
