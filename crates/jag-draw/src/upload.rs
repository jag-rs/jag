use anyhow::Result;
use bytemuck::{Pod, Zeroable};

use crate::allocator::{BufKey, OwnedBuffer, RenderAllocator};
use crate::display_list::{Command, DisplayList, ExternalTextureId};
use crate::scene::{
    Brush, FillRule, Path, PathCmd, Rect, RoundedRect, Stroke, TextRun, Transform2D,
};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
    pub z_index: f32,
}

pub struct GpuScene {
    pub vertex: OwnedBuffer,
    pub index: OwnedBuffer,
    pub vertices: u32,
    pub indices: u32,
}

/// Extracted text draw from DisplayList
#[derive(Clone, Debug)]
pub struct ExtractedTextDraw {
    pub run: TextRun,
    pub z: i32,
    pub transform: Transform2D,
}

/// Extracted image draw from DisplayList (placeholder for future)
#[derive(Clone, Debug)]
pub struct ExtractedImageDraw {
    pub path: std::path::PathBuf,
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub z: i32,
    pub transform: Transform2D,
    pub opacity: f32,
}

/// Extracted SVG draw from DisplayList (placeholder for future)
#[derive(Clone, Debug)]
pub struct ExtractedSvgDraw {
    pub path: std::path::PathBuf,
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub z: i32,
    pub transform: Transform2D,
    pub opacity: f32,
}

/// Extracted external texture draw from DisplayList.
#[derive(Clone, Debug)]
pub struct ExtractedExternalTextureDraw {
    pub texture_id: ExternalTextureId,
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub z: i32,
    pub opacity: f32,
    pub premultiplied: bool,
}

/// A contiguous range inside the transparent index buffer for a given z-index.
#[derive(Clone, Copy, Debug)]
pub struct TransparentBatch {
    pub z: i32,
    pub index_start: u32,
    pub index_count: u32,
}

/// Complete unified scene data extracted from DisplayList
pub struct UnifiedSceneData {
    pub gpu_scene: GpuScene,
    pub transparent_gpu_scene: GpuScene,
    pub transparent_batches: Vec<TransparentBatch>,
    pub text_draws: Vec<ExtractedTextDraw>,
    pub image_draws: Vec<ExtractedImageDraw>,
    pub svg_draws: Vec<ExtractedSvgDraw>,
    pub external_texture_draws: Vec<ExtractedExternalTextureDraw>,
}

fn apply_transform(p: [f32; 2], t: Transform2D) -> [f32; 2] {
    let [a, b, c, d, e, f] = t.m;
    [a * p[0] + c * p[1] + e, b * p[0] + d * p[1] + f]
}

fn rect_to_verts(rect: Rect, color: [f32; 4], t: Transform2D, z: f32) -> ([Vertex; 4], [u16; 6]) {
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

fn push_rect_linear_gradient(
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

fn push_ellipse(
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

fn push_ellipse_radial_gradient(
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

fn push_rounded_rect_linear_gradient(
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

    tessellate_path_fill_with_color_fn(vertices, indices, &path, z, t, |p| {
        let proj = ((p[0] - start[0]) * dx + (p[1] - start[1]) * dy) / denom;
        sample_gradient_stops(&packed, proj)
    });
}

fn push_rounded_rect_radial_gradient(
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

fn tessellate_path_fill(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    path: &Path,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    tessellate_path_fill_with_color_fn(vertices, indices, path, z, t, |_| color);
}

fn tessellate_path_fill_with_color_fn<F>(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    path: &Path,
    z: f32,
    t: Transform2D,
    mut color_at: F,
) where
    F: FnMut([f32; 2]) -> [f32; 4],
{
    let Some(geom) = tessellate_path_fill_geometry(path) else {
        return;
    };

    // Transform and append
    if vertices.len() > u16::MAX as usize {
        return;
    }
    let base = vertices.len() as u16;
    for p in &geom.vertices {
        let tp = apply_transform(*p, t);
        vertices.push(Vertex {
            pos: tp,
            color: color_at(*p),
            z_index: z,
        });
    }
    indices.extend(geom.indices.iter().map(|i| base + *i));
}

fn tessellate_path_fill_subdivided_with_color_fn<F>(
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

fn tessellate_path_stroke(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    path: &Path,
    stroke: Stroke,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
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
    let base = vertices.len() as u16;
    for p in &geom.vertices {
        let tp = apply_transform(*p, t);
        vertices.push(Vertex {
            pos: tp,
            color,
            z_index: z,
        });
    }
    indices.extend(geom.indices.iter().map(|i| base + *i));
}

/// Build a Path representing a rounded rectangle using cubic Beziers (kappa approximation).
/// This path is then tessellated by lyon for precise coverage (avoids fan artifacts on small radii).
fn rounded_rect_to_path(rrect: RoundedRect) -> Path {
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

fn push_rounded_rect(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u16>,
    rrect: RoundedRect,
    color: [f32; 4],
    z: f32,
    t: Transform2D,
) {
    // Delegate to lyon's robust tessellator via our generic path fill
    let path = rounded_rect_to_path(rrect);
    tessellate_path_fill(vertices, indices, &path, color, z, t);
}

fn push_rect_stroke(
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

fn push_rounded_rect_stroke(
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
    tessellate_path_stroke(vertices, indices, &path, Stroke { width: w }, color, z, t);
}

pub fn upload_display_list(
    allocator: &mut RenderAllocator,
    queue: &wgpu::Queue,
    list: &DisplayList,
) -> Result<GpuScene> {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();

    // NOTE: Z-index sorting disabled because it breaks clip/transform stacks.
    // For proper z-ordering, we need to either:
    // 1. Use a depth buffer, or
    // 2. Ensure commands are emitted in the correct z-order from the start
    // let mut sorted_list = list.clone();
    // sorted_list.sort_by_z();

    for cmd in &list.commands {
        match cmd {
            Command::DrawRect {
                rect,
                brush,
                transform,
                z,
                ..
            } => {
                match brush {
                    Brush::Solid(col) => {
                        let color = [col.r, col.g, col.b, col.a];
                        let (v, i) = rect_to_verts(*rect, color, *transform, *z as f32);
                        let base = vertices.len() as u16;
                        vertices.extend_from_slice(&v);
                        indices.extend(i.iter().map(|idx| base + idx));
                    }
                    Brush::LinearGradient { stops, .. } => {
                        // Only handle horizontal gradients for now: map t along x within rect
                        let mut packed: Vec<(f32, [f32; 4])> = stops
                            .iter()
                            .map(|(tpos, c)| (*tpos, [c.r, c.g, c.b, c.a]))
                            .collect();
                        if packed.is_empty() {
                            continue;
                        }
                        // Clamp and ensure 0 and 1 exist
                        if packed.first().unwrap().0 > 0.0 {
                            let c = packed.first().unwrap().1;
                            packed.insert(0, (0.0, c));
                        }
                        if packed.last().unwrap().0 < 1.0 {
                            let c = packed.last().unwrap().1;
                            packed.push((1.0, c));
                        }
                        push_rect_linear_gradient(
                            &mut vertices,
                            &mut indices,
                            *rect,
                            &packed,
                            *transform,
                            *z as f32,
                        );
                    }
                    _ => {}
                }
            }
            Command::DrawRoundedRect {
                rrect,
                brush,
                transform,
                z,
                ..
            } => match brush {
                Brush::Solid(col) => {
                    let color = [col.r, col.g, col.b, col.a];
                    push_rounded_rect(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        color,
                        *z as f32,
                        *transform,
                    );
                }
                Brush::LinearGradient { start, end, stops } => {
                    let packed: Vec<(f32, [f32; 4])> = stops
                        .iter()
                        .map(|(tpos, c)| (*tpos, [c.r, c.g, c.b, c.a]))
                        .collect();
                    if packed.is_empty() {
                        continue;
                    }
                    push_rounded_rect_linear_gradient(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        *start,
                        *end,
                        &packed,
                        *z as f32,
                        *transform,
                    );
                }
                Brush::RadialGradient {
                    center,
                    radius,
                    stops,
                } => {
                    let packed: Vec<(f32, [f32; 4])> = stops
                        .iter()
                        .map(|(tpos, c)| (*tpos, [c.r, c.g, c.b, c.a]))
                        .collect();
                    if packed.is_empty() {
                        continue;
                    }
                    push_rounded_rect_radial_gradient(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        *center,
                        *radius,
                        &packed,
                        *z as f32,
                        *transform,
                    );
                }
            },
            Command::StrokeRect {
                rect,
                stroke,
                brush,
                transform,
                z,
                ..
            } => {
                if let Brush::Solid(col) = brush {
                    let color = [col.r, col.g, col.b, col.a];
                    push_rect_stroke(
                        &mut vertices,
                        &mut indices,
                        *rect,
                        *stroke,
                        color,
                        *z as f32,
                        *transform,
                    );
                }
            }
            Command::StrokeRoundedRect {
                rrect,
                stroke,
                brush,
                transform,
                z,
                ..
            } => {
                if let Brush::Solid(col) = brush {
                    let color = [col.r, col.g, col.b, col.a];
                    push_rounded_rect_stroke(
                        &mut vertices,
                        &mut indices,
                        *rrect,
                        *stroke,
                        color,
                        *z as f32,
                        *transform,
                    );
                }
            }
            Command::DrawEllipse {
                center,
                radii,
                brush,
                transform,
                z,
                ..
            } => match brush {
                Brush::Solid(col) => {
                    let color = [col.r, col.g, col.b, col.a];
                    push_ellipse(
                        &mut vertices,
                        &mut indices,
                        *center,
                        *radii,
                        color,
                        *z as f32,
                        *transform,
                    );
                }
                Brush::RadialGradient {
                    center: _gcenter,
                    radius: _r,
                    stops,
                } => {
                    let mut packed: Vec<(f32, [f32; 4])> = stops
                        .iter()
                        .map(|(t, c)| (*t, [c.r, c.g, c.b, c.a]))
                        .collect();
                    if packed.is_empty() {
                        continue;
                    }
                    if packed.first().unwrap().0 > 0.0 {
                        let c = packed.first().unwrap().1;
                        packed.insert(0, (0.0, c));
                    }
                    if packed.last().unwrap().0 < 1.0 {
                        let c = packed.last().unwrap().1;
                        packed.push((1.0, c));
                    }
                    push_ellipse_radial_gradient(
                        &mut vertices,
                        &mut indices,
                        *center,
                        *radii,
                        &packed,
                        *z as f32,
                        *transform,
                    );
                }
                _ => {}
            },
            Command::FillPath {
                path,
                color,
                transform,
                z,
                ..
            } => {
                let col = [color.r, color.g, color.b, color.a];
                tessellate_path_fill(
                    &mut vertices,
                    &mut indices,
                    path,
                    col,
                    *z as f32,
                    *transform,
                );
            }
            Command::StrokePath {
                path,
                stroke,
                color,
                transform,
                z,
                ..
            } => {
                let col = [color.r, color.g, color.b, color.a];
                tessellate_path_stroke(
                    &mut vertices,
                    &mut indices,
                    path,
                    *stroke,
                    col,
                    *z as f32,
                    *transform,
                );
            }
            // BoxShadow commands are handled by PassManager as a separate pipeline.
            Command::BoxShadow { .. } => {}
            // Hit-only regions: intentionally not rendered.
            Command::HitRegionRect { .. } => {}
            Command::HitRegionRoundedRect { .. } => {}
            Command::HitRegionEllipse { .. } => {}
            _ => {}
        }
    }

    // Ensure index buffer size meets COPY_BUFFER_ALIGNMENT (4 bytes)
    if (indices.len() % 2) != 0 {
        if indices.len() >= 3 {
            let a = indices[indices.len() - 3];
            let b = indices[indices.len() - 2];
            let c = indices[indices.len() - 1];
            indices.extend_from_slice(&[a, b, c]);
        } else {
            indices.push(0);
        }
    }

    // Allocate GPU buffers and upload
    let vsize = (vertices.len() * std::mem::size_of::<Vertex>()) as u64;
    let isize = (indices.len() * std::mem::size_of::<u16>()) as u64;
    let vbuf = allocator.allocate_buffer(BufKey {
        size: vsize.max(4),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });
    let ibuf = allocator.allocate_buffer(BufKey {
        size: isize.max(4),
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
    });
    if vsize > 0 {
        queue.write_buffer(&vbuf.buffer, 0, bytemuck::cast_slice(&vertices));
    }
    if isize > 0 {
        queue.write_buffer(&ibuf.buffer, 0, bytemuck::cast_slice(&indices));
    }

    Ok(GpuScene {
        vertex: vbuf,
        index: ibuf,
        vertices: vertices.len() as u32,
        indices: indices.len() as u32,
    })
}

/// Upload a DisplayList extracting all element types for unified rendering.
/// This is the main entry point for the unified rendering system.
///
/// Returns:
/// - GpuScene: Uploaded solid geometry (rectangles, paths, etc.)
/// - text_draws: Text runs with their transforms and z-indices
/// - image_draws: Image draws (currently placeholder, will be implemented)
/// - svg_draws: SVG draws (currently placeholder, will be implemented)
pub fn upload_display_list_unified(
    allocator: &mut RenderAllocator,
    queue: &wgpu::Queue,
    list: &DisplayList,
) -> Result<UnifiedSceneData> {
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    let mut transparent_vertices: Vec<Vertex> = Vec::new();
    let mut transparent_indices: Vec<u16> = Vec::new();
    let mut transparent_batches: Vec<TransparentBatch> = Vec::new();
    let mut text_draws: Vec<ExtractedTextDraw> = Vec::new();
    let mut image_draws: Vec<ExtractedImageDraw> = Vec::new();
    let mut svg_draws: Vec<ExtractedSvgDraw> = Vec::new();
    let mut external_texture_draws: Vec<ExtractedExternalTextureDraw> = Vec::new();
    let is_transparent = |alpha: f32| alpha < 0.999;
    let record_transparent_batch =
        |batches: &mut Vec<TransparentBatch>, z: i32, index_start: usize, index_end: usize| {
            if index_end <= index_start {
                return;
            }
            let start = index_start as u32;
            let count = (index_end - index_start) as u32;
            if let Some(last) = batches.last_mut()
                && last.z == z
                && last.index_start + last.index_count == start
            {
                last.index_count += count;
            } else {
                batches.push(TransparentBatch {
                    z,
                    index_start: start,
                    index_count: count,
                });
            }
        };

    // Track transform stack for completeness, but note that draw commands
    // already carry fully-composed world transforms. For unified upload we
    // treat the per-command transform as authoritative and use the stack
    // only to mirror the current state (kept for potential future use).
    let mut transform_stack: Vec<Transform2D> = vec![Transform2D::identity()];
    let mut _current_transform = Transform2D::identity();

    // Track CSS-style group opacity. Each PushOpacity pushes the effective
    // (accumulated) opacity onto the stack; PopOpacity restores the previous
    // level.  All vertex colours are pre-multiplied by the current effective
    // opacity so that nested opacities compose correctly.
    let mut opacity_stack: Vec<f32> = vec![1.0];
    let current_opacity = |stack: &[f32]| *stack.last().unwrap_or(&1.0);

    // Helper: multiply a premultiplied-alpha colour by group opacity.
    // All four channels are scaled so the premultiplied invariant holds.
    fn premul_opa(c: [f32; 4], o: f32) -> [f32; 4] {
        if o >= 0.999 {
            return c;
        }
        [c[0] * o, c[1] * o, c[2] * o, c[3] * o]
    }

    for cmd in &list.commands {
        match cmd {
            // Handle transform stack
            Command::PushTransform(t) => {
                // `t` is already the composed world transform at this stack depth.
                _current_transform = *t;
                transform_stack.push(_current_transform);
            }
            Command::PopTransform => {
                transform_stack.pop();
                _current_transform = transform_stack
                    .last()
                    .copied()
                    .unwrap_or(Transform2D::identity());
            }

            // Extract text commands
            Command::DrawText {
                run, z, transform, ..
            } => {
                // Text draws already carry the full world transform.
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);
                let mut text_run = run.clone();
                if opa < 0.999 {
                    text_run.color.r *= opa;
                    text_run.color.g *= opa;
                    text_run.color.b *= opa;
                    text_run.color.a *= opa;
                }
                text_draws.push(ExtractedTextDraw {
                    run: text_run,
                    z: *z,
                    transform: final_transform,
                });
            }

            // Extract hyperlink as text + optional underline
            Command::DrawHyperlink {
                hyperlink,
                z,
                transform,
                ..
            } => {
                // Hyperlink commands also carry their full world transform.
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);

                // Extract hyperlink text as a text draw (with group opacity)
                let mut link_color = hyperlink.color;
                if opa < 0.999 {
                    link_color.r *= opa;
                    link_color.g *= opa;
                    link_color.b *= opa;
                    link_color.a *= opa;
                }
                let text_run = TextRun {
                    text: hyperlink.text.clone(),
                    pos: hyperlink.pos,
                    size: hyperlink.size,
                    color: link_color,
                    weight: hyperlink.weight,
                    style: hyperlink.style,
                    family: hyperlink.family.clone(),
                };
                text_draws.push(ExtractedTextDraw {
                    run: text_run,
                    z: *z,
                    transform: final_transform,
                });

                // Draw underline if enabled
                if hyperlink.underline {
                    let underline_color = hyperlink.underline_color.unwrap_or(hyperlink.color);
                    let color = premul_opa(
                        [
                            underline_color.r,
                            underline_color.g,
                            underline_color.b,
                            underline_color.a,
                        ],
                        opa,
                    );

                    // Prefer explicit measured width from layout. Fall back to heuristic.
                    let (underline_x, text_width) =
                        if let Some(w) = hyperlink.measured_width.map(|v| v.max(0.0)) {
                            (hyperlink.pos[0], w)
                        } else {
                            let trimmed = hyperlink.text.trim_end();
                            let char_count = trimmed.chars().count() as f32;
                            let weight_boost = ((hyperlink.weight - 400.0).max(0.0) / 500.0) * 0.08;
                            let char_width = hyperlink.size * (0.50 + weight_boost);
                            let mut width = char_count * char_width;
                            let inset = hyperlink.size * 0.10;
                            if width > inset * 2.0 {
                                width -= inset * 2.0;
                            }
                            (hyperlink.pos[0] + inset, width)
                        };

                    // Underline is a thin rect slightly below the baseline.
                    // `hyperlink.pos[1]` is the baseline Y coordinate; place the
                    // underline about ~10% of the font size below it.
                    let underline_thickness = (hyperlink.size * 0.08).max(1.0);
                    let underline_offset = hyperlink.size * 0.10; // Slightly closer to glyphs

                    let underline_rect = Rect {
                        x: underline_x,
                        y: hyperlink.pos[1] + underline_offset,
                        w: text_width,
                        h: underline_thickness,
                    };

                    let (v, i) = rect_to_verts(underline_rect, color, final_transform, *z as f32);
                    if is_transparent(color[3]) {
                        let index_start = transparent_indices.len();
                        let base = transparent_vertices.len() as u16;
                        transparent_vertices.extend_from_slice(&v);
                        transparent_indices.extend(i.iter().map(|idx| base + idx));
                        record_transparent_batch(
                            &mut transparent_batches,
                            *z,
                            index_start,
                            transparent_indices.len(),
                        );
                    } else {
                        let base = vertices.len() as u16;
                        vertices.extend_from_slice(&v);
                        indices.extend(i.iter().map(|idx| base + idx));
                    }
                }
            }

            // Process solid geometry commands
            Command::DrawRect {
                rect,
                brush,
                transform,
                z,
                ..
            } => {
                // Rect draws already carry the full world transform.
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);
                match brush {
                    Brush::Solid(col) => {
                        let color = premul_opa([col.r, col.g, col.b, col.a], opa);
                        let (v, i) = rect_to_verts(*rect, color, final_transform, *z as f32);
                        if is_transparent(color[3]) {
                            let index_start = transparent_indices.len();
                            let base = transparent_vertices.len() as u16;
                            transparent_vertices.extend_from_slice(&v);
                            transparent_indices.extend(i.iter().map(|idx| base + idx));
                            record_transparent_batch(
                                &mut transparent_batches,
                                *z,
                                index_start,
                                transparent_indices.len(),
                            );
                        } else {
                            let base = vertices.len() as u16;
                            vertices.extend_from_slice(&v);
                            indices.extend(i.iter().map(|idx| base + idx));
                        }
                    }
                    Brush::LinearGradient { stops, .. } => {
                        // Only handle horizontal gradients for now: map t along x within rect
                        let mut packed: Vec<(f32, [f32; 4])> = stops
                            .iter()
                            .map(|(tpos, c)| (*tpos, premul_opa([c.r, c.g, c.b, c.a], opa)))
                            .collect();
                        if packed.is_empty() {
                            continue;
                        }
                        // Clamp and ensure 0 and 1 exist
                        if packed.first().unwrap().0 > 0.0 {
                            let c = packed.first().unwrap().1;
                            packed.insert(0, (0.0, c));
                        }
                        if packed.last().unwrap().0 < 1.0 {
                            let c = packed.last().unwrap().1;
                            packed.push((1.0, c));
                        }
                        let gradient_transparent = packed.iter().any(|(_, c)| is_transparent(c[3]));
                        if gradient_transparent {
                            let index_start = transparent_indices.len();
                            push_rect_linear_gradient(
                                &mut transparent_vertices,
                                &mut transparent_indices,
                                *rect,
                                &packed,
                                final_transform,
                                *z as f32,
                            );
                            record_transparent_batch(
                                &mut transparent_batches,
                                *z,
                                index_start,
                                transparent_indices.len(),
                            );
                        } else {
                            push_rect_linear_gradient(
                                &mut vertices,
                                &mut indices,
                                *rect,
                                &packed,
                                final_transform,
                                *z as f32,
                            );
                        }
                    }
                    _ => {}
                }
            }
            Command::DrawRoundedRect {
                rrect,
                brush,
                transform,
                z,
                ..
            } => {
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);
                match brush {
                    Brush::Solid(col) => {
                        let color = premul_opa([col.r, col.g, col.b, col.a], opa);
                        if is_transparent(color[3]) {
                            let index_start = transparent_indices.len();
                            push_rounded_rect(
                                &mut transparent_vertices,
                                &mut transparent_indices,
                                *rrect,
                                color,
                                *z as f32,
                                final_transform,
                            );
                            record_transparent_batch(
                                &mut transparent_batches,
                                *z,
                                index_start,
                                transparent_indices.len(),
                            );
                        } else {
                            push_rounded_rect(
                                &mut vertices,
                                &mut indices,
                                *rrect,
                                color,
                                *z as f32,
                                final_transform,
                            );
                        }
                    }
                    Brush::LinearGradient { start, end, stops } => {
                        let packed: Vec<(f32, [f32; 4])> = stops
                            .iter()
                            .map(|(tpos, c)| (*tpos, premul_opa([c.r, c.g, c.b, c.a], opa)))
                            .collect();
                        if packed.is_empty() {
                            continue;
                        }
                        let gradient_transparent = packed.iter().any(|(_, c)| is_transparent(c[3]));
                        if gradient_transparent {
                            let index_start = transparent_indices.len();
                            push_rounded_rect_linear_gradient(
                                &mut transparent_vertices,
                                &mut transparent_indices,
                                *rrect,
                                *start,
                                *end,
                                &packed,
                                *z as f32,
                                final_transform,
                            );
                            record_transparent_batch(
                                &mut transparent_batches,
                                *z,
                                index_start,
                                transparent_indices.len(),
                            );
                        } else {
                            push_rounded_rect_linear_gradient(
                                &mut vertices,
                                &mut indices,
                                *rrect,
                                *start,
                                *end,
                                &packed,
                                *z as f32,
                                final_transform,
                            );
                        }
                    }
                    Brush::RadialGradient {
                        center,
                        radius,
                        stops,
                    } => {
                        let packed: Vec<(f32, [f32; 4])> = stops
                            .iter()
                            .map(|(tpos, c)| (*tpos, premul_opa([c.r, c.g, c.b, c.a], opa)))
                            .collect();
                        if packed.is_empty() {
                            continue;
                        }
                        let gradient_transparent = packed.iter().any(|(_, c)| is_transparent(c[3]));
                        if gradient_transparent {
                            let index_start = transparent_indices.len();
                            push_rounded_rect_radial_gradient(
                                &mut transparent_vertices,
                                &mut transparent_indices,
                                *rrect,
                                *center,
                                *radius,
                                &packed,
                                *z as f32,
                                final_transform,
                            );
                            record_transparent_batch(
                                &mut transparent_batches,
                                *z,
                                index_start,
                                transparent_indices.len(),
                            );
                        } else {
                            push_rounded_rect_radial_gradient(
                                &mut vertices,
                                &mut indices,
                                *rrect,
                                *center,
                                *radius,
                                &packed,
                                *z as f32,
                                final_transform,
                            );
                        }
                    }
                }
            }
            Command::StrokeRect {
                rect,
                stroke,
                brush,
                transform,
                z,
                ..
            } => {
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);
                if let Brush::Solid(col) = brush {
                    let color = premul_opa([col.r, col.g, col.b, col.a], opa);
                    if is_transparent(color[3]) {
                        let index_start = transparent_indices.len();
                        push_rect_stroke(
                            &mut transparent_vertices,
                            &mut transparent_indices,
                            *rect,
                            *stroke,
                            color,
                            *z as f32,
                            final_transform,
                        );
                        record_transparent_batch(
                            &mut transparent_batches,
                            *z,
                            index_start,
                            transparent_indices.len(),
                        );
                    } else {
                        push_rect_stroke(
                            &mut vertices,
                            &mut indices,
                            *rect,
                            *stroke,
                            color,
                            *z as f32,
                            final_transform,
                        );
                    }
                }
            }
            Command::StrokeRoundedRect {
                rrect,
                stroke,
                brush,
                transform,
                z,
                ..
            } => {
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);
                if let Brush::Solid(col) = brush {
                    let color = premul_opa([col.r, col.g, col.b, col.a], opa);
                    if is_transparent(color[3]) {
                        let index_start = transparent_indices.len();
                        push_rounded_rect_stroke(
                            &mut transparent_vertices,
                            &mut transparent_indices,
                            *rrect,
                            *stroke,
                            color,
                            *z as f32,
                            final_transform,
                        );
                        record_transparent_batch(
                            &mut transparent_batches,
                            *z,
                            index_start,
                            transparent_indices.len(),
                        );
                    } else {
                        push_rounded_rect_stroke(
                            &mut vertices,
                            &mut indices,
                            *rrect,
                            *stroke,
                            color,
                            *z as f32,
                            final_transform,
                        );
                    }
                }
            }
            Command::DrawEllipse {
                center,
                radii,
                brush,
                transform,
                z,
                ..
            } => {
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);
                match brush {
                    Brush::Solid(col) => {
                        let color = premul_opa([col.r, col.g, col.b, col.a], opa);
                        if is_transparent(color[3]) {
                            let index_start = transparent_indices.len();
                            push_ellipse(
                                &mut transparent_vertices,
                                &mut transparent_indices,
                                *center,
                                *radii,
                                color,
                                *z as f32,
                                final_transform,
                            );
                            record_transparent_batch(
                                &mut transparent_batches,
                                *z,
                                index_start,
                                transparent_indices.len(),
                            );
                        } else {
                            push_ellipse(
                                &mut vertices,
                                &mut indices,
                                *center,
                                *radii,
                                color,
                                *z as f32,
                                final_transform,
                            );
                        }
                    }
                    Brush::RadialGradient {
                        center: _gcenter,
                        radius: _r,
                        stops,
                    } => {
                        let mut packed: Vec<(f32, [f32; 4])> = stops
                            .iter()
                            .map(|(t, c)| (*t, premul_opa([c.r, c.g, c.b, c.a], opa)))
                            .collect();
                        if packed.is_empty() {
                            continue;
                        }
                        if packed.first().unwrap().0 > 0.0 {
                            let c = packed.first().unwrap().1;
                            packed.insert(0, (0.0, c));
                        }
                        if packed.last().unwrap().0 < 1.0 {
                            let c = packed.last().unwrap().1;
                            packed.push((1.0, c));
                        }
                        let gradient_transparent = packed.iter().any(|(_, c)| is_transparent(c[3]));
                        if gradient_transparent {
                            let index_start = transparent_indices.len();
                            push_ellipse_radial_gradient(
                                &mut transparent_vertices,
                                &mut transparent_indices,
                                *center,
                                *radii,
                                &packed,
                                *z as f32,
                                final_transform,
                            );
                            record_transparent_batch(
                                &mut transparent_batches,
                                *z,
                                index_start,
                                transparent_indices.len(),
                            );
                        } else {
                            push_ellipse_radial_gradient(
                                &mut vertices,
                                &mut indices,
                                *center,
                                *radii,
                                &packed,
                                *z as f32,
                                final_transform,
                            );
                        }
                    }
                    _ => {}
                }
            }
            Command::FillPath {
                path,
                color,
                transform,
                z,
                ..
            } => {
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);
                let col = premul_opa([color.r, color.g, color.b, color.a], opa);
                if is_transparent(col[3]) {
                    let index_start = transparent_indices.len();
                    tessellate_path_fill(
                        &mut transparent_vertices,
                        &mut transparent_indices,
                        path,
                        col,
                        *z as f32,
                        final_transform,
                    );
                    record_transparent_batch(
                        &mut transparent_batches,
                        *z,
                        index_start,
                        transparent_indices.len(),
                    );
                } else {
                    tessellate_path_fill(
                        &mut vertices,
                        &mut indices,
                        path,
                        col,
                        *z as f32,
                        final_transform,
                    );
                }
            }
            Command::StrokePath {
                path,
                stroke,
                color,
                transform,
                z,
                ..
            } => {
                let final_transform = *transform;
                let opa = current_opacity(&opacity_stack);
                let col = premul_opa([color.r, color.g, color.b, color.a], opa);
                if is_transparent(col[3]) {
                    let index_start = transparent_indices.len();
                    tessellate_path_stroke(
                        &mut transparent_vertices,
                        &mut transparent_indices,
                        path,
                        *stroke,
                        col,
                        *z as f32,
                        final_transform,
                    );
                    record_transparent_batch(
                        &mut transparent_batches,
                        *z,
                        index_start,
                        transparent_indices.len(),
                    );
                } else {
                    tessellate_path_stroke(
                        &mut vertices,
                        &mut indices,
                        path,
                        *stroke,
                        col,
                        *z as f32,
                        final_transform,
                    );
                }
            }
            Command::DrawImage {
                path,
                origin,
                size,
                z,
                transform,
            } => {
                // Apply the command's world transform to the image origin.
                let final_transform = *transform;
                let world_origin = apply_transform(*origin, final_transform);
                let opa = current_opacity(&opacity_stack);
                image_draws.push(ExtractedImageDraw {
                    path: path.clone(),
                    origin: world_origin,
                    size: *size,
                    z: *z,
                    transform: final_transform,
                    opacity: opa,
                });
            }
            Command::DrawSvg {
                path,
                origin,
                max_size,
                z,
                transform,
            } => {
                // Apply the command's world transform to the SVG origin.
                let final_transform = *transform;
                let world_origin = apply_transform(*origin, final_transform);
                let opa = current_opacity(&opacity_stack);
                svg_draws.push(ExtractedSvgDraw {
                    path: path.clone(),
                    origin: world_origin,
                    size: *max_size,
                    z: *z,
                    transform: final_transform,
                    opacity: opa,
                });
            }
            Command::DrawExternalTexture {
                rect,
                texture_id,
                z,
                transform,
                opacity,
                premultiplied,
            } => {
                let final_transform = *transform;
                let world_origin = apply_transform([rect.x, rect.y], final_transform);
                let opa = current_opacity(&opacity_stack);
                external_texture_draws.push(ExtractedExternalTextureDraw {
                    texture_id: *texture_id,
                    origin: world_origin,
                    size: [rect.w, rect.h],
                    z: *z,
                    opacity: *opacity * opa,
                    premultiplied: *premultiplied,
                });
            }
            // BoxShadow commands are handled by PassManager as a separate pipeline.
            Command::BoxShadow { .. } => {}
            // Hit-only regions: intentionally not rendered.
            Command::HitRegionRect { .. } => {}
            Command::HitRegionRoundedRect { .. } => {}
            Command::HitRegionEllipse { .. } => {}
            // Clip commands would need special handling in unified rendering
            Command::PushClip(_) => {}
            Command::PopClip => {}
            Command::PushOpacity(alpha) => {
                let parent = current_opacity(&opacity_stack);
                opacity_stack.push(parent * alpha.clamp(0.0, 1.0));
            }
            Command::PopOpacity => {
                if opacity_stack.len() > 1 {
                    opacity_stack.pop();
                }
            }
        }
    }

    // Ensure index buffer size meets COPY_BUFFER_ALIGNMENT (4 bytes)
    let align_indices = |indices: &mut Vec<u16>| {
        if (indices.len() % 2) != 0 {
            if indices.len() >= 3 {
                let a = indices[indices.len() - 3];
                let b = indices[indices.len() - 2];
                let c = indices[indices.len() - 1];
                indices.extend_from_slice(&[a, b, c]);
            } else {
                indices.push(0);
            }
        }
    };
    align_indices(&mut indices);
    align_indices(&mut transparent_indices);

    // Allocate GPU buffers and upload
    let upload_scene = |allocator: &mut RenderAllocator,
                        queue: &wgpu::Queue,
                        vertices: &[Vertex],
                        indices: &[u16]|
     -> GpuScene {
        let vsize = (vertices.len() * std::mem::size_of::<Vertex>()) as u64;
        let isize = (indices.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = allocator.allocate_buffer(BufKey {
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        });
        let ibuf = allocator.allocate_buffer(BufKey {
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf.buffer, 0, bytemuck::cast_slice(vertices));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf.buffer, 0, bytemuck::cast_slice(indices));
        }
        GpuScene {
            vertex: vbuf,
            index: ibuf,
            vertices: vertices.len() as u32,
            indices: indices.len() as u32,
        }
    };
    let gpu_scene = upload_scene(allocator, queue, &vertices, &indices);
    let transparent_gpu_scene = upload_scene(
        allocator,
        queue,
        &transparent_vertices,
        &transparent_indices,
    );

    Ok(UnifiedSceneData {
        gpu_scene,
        transparent_gpu_scene,
        transparent_batches,
        text_draws,
        image_draws,
        svg_draws,
        external_texture_draws,
    })
}
