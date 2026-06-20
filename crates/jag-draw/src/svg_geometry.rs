//! SVG → vector geometry import (Phase 7.5.2).
//!
//! Imports an SVG's filled/stroked paths into a [`crate::painter::Painter`] as
//! native geometry (used for crisp, resolution-independent icons), plus helpers
//! to query an SVG's intrinsic size and whether it needs rasterization. Text,
//! gradients, patterns, masks and filters are not expressed analytically here —
//! those SVGs go through the raster cache instead.

use std::path::Path;

/// Import result counters for basic visibility/debugging.
#[derive(Clone, Copy, Debug, Default)]
pub struct SvgImportStats {
    pub rects: u32,
    pub rounded_rects: u32,
    pub ellipses: u32,
    pub paths: u32,
    pub strokes: u32,
    pub skipped: u32,
}

fn color_from_usvg(color: usvg::Color, opacity: f32) -> crate::scene::ColorLinPremul {
    crate::scene::ColorLinPremul::from_srgba(color.red, color.green, color.blue, opacity)
}

fn transform2d_from_usvg(t: usvg::Transform) -> crate::scene::Transform2D {
    // tiny_skia_path::Transform uses fields (sx, kx, ky, sy, tx, ty)
    crate::scene::Transform2D {
        m: [
            t.sx as f32,
            t.ky as f32,
            t.kx as f32,
            t.sy as f32,
            t.tx as f32,
            t.ty as f32,
        ],
    }
}

fn fill_rule_from_usvg(rule: usvg::FillRule) -> crate::scene::FillRule {
    match rule {
        usvg::FillRule::NonZero => crate::scene::FillRule::NonZero,
        usvg::FillRule::EvenOdd => crate::scene::FillRule::EvenOdd,
    }
}

// Note: usvg outputs only Path/Image/Text/Group nodes; basic shapes are already converted to paths.

fn import_path_fill(
    painter: &mut crate::painter::Painter,
    node_transform: usvg::Transform,
    p: &usvg::Path,
    color: crate::scene::ColorLinPremul,
    stats: &mut SvgImportStats,
) {
    use crate::scene::{Path, PathCmd};
    let mut cmds: Vec<PathCmd> = Vec::new();
    // Convert usvg path data → our PathCmd. This covers move/line/quad/cubic/close.
    for seg in p.data().segments() {
        use usvg::tiny_skia_path::PathSegment;
        match seg {
            PathSegment::MoveTo(pt) => cmds.push(PathCmd::MoveTo([pt.x as f32, pt.y as f32])),
            PathSegment::LineTo(pt) => cmds.push(PathCmd::LineTo([pt.x as f32, pt.y as f32])),
            PathSegment::QuadTo(c, p) => cmds.push(PathCmd::QuadTo(
                [c.x as f32, c.y as f32],
                [p.x as f32, p.y as f32],
            )),
            PathSegment::CubicTo(c1, c2, p) => cmds.push(PathCmd::CubicTo(
                [c1.x as f32, c1.y as f32],
                [c2.x as f32, c2.y as f32],
                [p.x as f32, p.y as f32],
            )),
            PathSegment::Close => cmds.push(PathCmd::Close),
        }
    }
    let fill_rule = p
        .fill()
        .map(|f| fill_rule_from_usvg(f.rule()))
        .unwrap_or(crate::scene::FillRule::NonZero);
    let path = Path { cmds, fill_rule };
    let t = transform2d_from_usvg(node_transform);
    painter.push_transform(t);
    painter.fill_path(path, color, 0);
    painter.pop_transform();
    stats.paths += 1;
}

/// If the given usvg path is an axis-aligned rectangle made of straight
/// line segments (MoveTo + 3x LineTo + Close), return it as a Rect in
/// local coordinates. Rounded corners and curves are not considered a match.
fn detect_axis_aligned_rect(p: &usvg::Path) -> Option<crate::scene::Rect> {
    use usvg::tiny_skia_path::PathSegment;
    // Collect the first closed subpath consisting only of MoveTo/LineTo/Close
    let mut points: Vec<[f32; 2]> = Vec::new();
    let mut started = false;
    for seg in p.data().segments() {
        match seg {
            PathSegment::MoveTo(pt) => {
                if started {
                    break;
                } // Only consider first subpath
                started = true;
                points.clear();
                points.push([pt.x as f32, pt.y as f32]);
            }
            PathSegment::LineTo(pt) => {
                if !started {
                    return None;
                }
                let q = [pt.x as f32, pt.y as f32];
                // Skip exact duplicates
                if points
                    .last()
                    .map_or(true, |last| last[0] != q[0] || last[1] != q[1])
                {
                    points.push(q);
                }
            }
            PathSegment::QuadTo(..) | PathSegment::CubicTo(..) => {
                // Curves present → not a simple rect
                return None;
            }
            PathSegment::Close => {
                break;
            }
        }
    }
    if points.len() != 4 {
        return None;
    }
    // Verify axis alignment: each edge must be horizontal or vertical
    for i in 0..4 {
        let a = points[i];
        let b = points[(i + 1) % 4];
        let dx = (a[0] - b[0]).abs();
        let dy = (a[1] - b[1]).abs();
        if dx > 1e-4 && dy > 1e-4 {
            return None;
        }
    }
    // Build rect from min/max
    let mut minx = f32::INFINITY;
    let mut miny = f32::INFINITY;
    let mut maxx = f32::NEG_INFINITY;
    let mut maxy = f32::NEG_INFINITY;
    for p in &points {
        minx = minx.min(p[0]);
        miny = miny.min(p[1]);
        maxx = maxx.max(p[0]);
        maxy = maxy.max(p[1]);
    }
    let w = (maxx - minx).abs();
    let h = (maxy - miny).abs();
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some(crate::scene::Rect {
        x: minx.min(maxx),
        y: miny.min(maxy),
        w,
        h,
    })
}

fn paint_from_fill(fill: &usvg::Fill) -> Option<crate::scene::Brush> {
    match fill.paint() {
        usvg::Paint::Color(c) => Some(crate::scene::Brush::Solid(color_from_usvg(
            *c,
            fill.opacity().get() as f32,
        ))),
        _ => None,
    }
}

/// Import an SVG file into the display list as vector geometry.
///
/// Notes:
/// - Supports Rect/RoundedRect/Circle/Ellipse and basic filled Paths.
/// - Only solid fills are mapped. Unsupported paints/filters/masks/text are skipped.
pub fn import_svg_geometry_to_painter(
    painter: &mut crate::painter::Painter,
    path: &Path,
) -> Option<SvgImportStats> {
    let data = std::fs::read(path).ok()?;
    let mut opt = usvg::Options::default();
    opt.resources_dir = path.parent().map(|p| p.to_path_buf());
    opt.fontdb = crate::svg_fontdb::svg_font_db().db;
    let tree = usvg::Tree::from_data(&data, &opt).ok()?;
    let mut stats = SvgImportStats::default();

    // Traverse the tree in document order; apply node-local transforms only for now.
    fn walk(
        group: &usvg::Group,
        painter: &mut crate::painter::Painter,
        stats: &mut SvgImportStats,
    ) {
        for node in group.children() {
            match node {
                usvg::Node::Path(p) => {
                    if let Some(fill) = p.fill() {
                        if let Some(crate::scene::Brush::Solid(col)) = paint_from_fill(fill) {
                            // Try fast-path: detect simple axis-aligned rectangle and emit as a primitive
                            if let Some(rect) = detect_axis_aligned_rect(p) {
                                let t = transform2d_from_usvg(p.abs_transform());
                                painter.push_transform(t);
                                painter.rect(rect, crate::scene::Brush::Solid(col), 0);
                                painter.pop_transform();
                                stats.rects += 1;
                            } else {
                                import_path_fill(painter, p.abs_transform(), p, col, stats);
                            }
                        } else {
                            // Unsupported paint servers (gradients/patterns) are skipped for geometry import.
                            stats.skipped += 1;
                        }
                    }
                    // Stroke (solid-only for now)
                    if let Some(st) = p.stroke() {
                        if let usvg::Paint::Color(c) = st.paint() {
                            let col = color_from_usvg(*c, st.opacity().get() as f32);
                            // If the path is a simple rect, stroke it via the rect stroke primitive
                            if let Some(rect) = detect_axis_aligned_rect(p) {
                                let t = transform2d_from_usvg(p.abs_transform());
                                painter.push_transform(t);
                                painter.stroke_rect(
                                    rect,
                                    crate::scene::Stroke {
                                        width: st.width().get() as f32,
                                    },
                                    crate::scene::Brush::Solid(col),
                                    0,
                                );
                                painter.pop_transform();
                                stats.strokes += 1;
                            } else {
                                // Build a Path copy from usvg data for stroke as well
                                use crate::scene::{Path as EPath, PathCmd};
                                let mut cmds: Vec<PathCmd> = Vec::new();
                                for seg in p.data().segments() {
                                    use usvg::tiny_skia_path::PathSegment;
                                    match seg {
                                        PathSegment::MoveTo(pt) => {
                                            cmds.push(PathCmd::MoveTo([pt.x as f32, pt.y as f32]))
                                        }
                                        PathSegment::LineTo(pt) => {
                                            cmds.push(PathCmd::LineTo([pt.x as f32, pt.y as f32]))
                                        }
                                        PathSegment::QuadTo(c, q) => cmds.push(PathCmd::QuadTo(
                                            [c.x as f32, c.y as f32],
                                            [q.x as f32, q.y as f32],
                                        )),
                                        PathSegment::CubicTo(c1, c2, q) => {
                                            cmds.push(PathCmd::CubicTo(
                                                [c1.x as f32, c1.y as f32],
                                                [c2.x as f32, c2.y as f32],
                                                [q.x as f32, q.y as f32],
                                            ))
                                        }
                                        PathSegment::Close => cmds.push(PathCmd::Close),
                                    }
                                }
                                let epath = EPath {
                                    cmds,
                                    fill_rule: crate::scene::FillRule::NonZero,
                                };
                                let t = transform2d_from_usvg(p.abs_transform());
                                painter.push_transform(t);
                                painter.stroke_path(
                                    epath,
                                    crate::scene::Stroke {
                                        width: st.width().get() as f32,
                                    },
                                    col,
                                    0,
                                );
                                painter.pop_transform();
                                stats.strokes += 1;
                            }
                        } else {
                            stats.skipped += 1;
                        }
                    }
                }
                usvg::Node::Group(g) => {
                    // Render group contents normally.
                    walk(g, painter, stats);
                }
                usvg::Node::Image(_img) => {
                    // Only traverse subroots for embedded SVG images.
                    // This avoids drawing clipPath/mask/pattern definition subtrees.
                    node.subroots(|subroot| walk(subroot, painter, stats));
                }
                usvg::Node::Text(_) => {
                    // Text-as-geometry not supported yet.
                }
            }
        }
    }

    let root = tree.root();
    walk(root, painter, &mut stats);

    Some(stats)
}

/// Get the intrinsic pixel size of an SVG file according to usvg's parsing
/// of width/height/viewBox. Returns (width,height) rounded to integers.
pub fn svg_intrinsic_size(path: &Path) -> Option<(u32, u32)> {
    let data = std::fs::read(path).ok()?;
    let mut opt = usvg::Options::default();
    opt.resources_dir = path.parent().map(|p| p.to_path_buf());
    opt.fontdb = crate::svg_fontdb::svg_font_db().db;
    let tree = usvg::Tree::from_data(&data, &opt).ok()?;
    let size = tree.size().to_int_size();
    Some((size.width().max(1), size.height().max(1)))
}

/// Determine if an SVG requires rasterization or can be rendered as vector geometry.
/// Returns true if the SVG uses features that cannot be expressed analytically
/// (filters, patterns, masks, gradients, images, text, etc.)
pub fn svg_requires_rasterization(path: &Path) -> Option<bool> {
    let data = std::fs::read(path).ok()?;
    let mut opt = usvg::Options::default();
    opt.resources_dir = path.parent().map(|p| p.to_path_buf());
    opt.fontdb = crate::svg_fontdb::svg_font_db().db;
    let tree = usvg::Tree::from_data(&data, &opt).ok()?;

    fn check_node(node: &usvg::Node) -> bool {
        match node {
            usvg::Node::Path(p) => {
                // Check if fill uses non-solid paint (gradients, patterns)
                if let Some(fill) = p.fill() {
                    if !matches!(fill.paint(), usvg::Paint::Color(_)) {
                        return true; // Gradient or pattern fill
                    }
                }

                // Check if stroke uses non-solid paint
                if let Some(stroke) = p.stroke() {
                    if !matches!(stroke.paint(), usvg::Paint::Color(_)) {
                        return true; // Gradient or pattern stroke
                    }
                }

                // Check subroots (e.g., clipPath definitions)
                let mut needs_raster = false;
                node.subroots(|subroot| {
                    if check_group(subroot) {
                        needs_raster = true;
                    }
                });
                needs_raster
            }
            usvg::Node::Image(_) => {
                // Embedded images require rasterization
                true
            }
            usvg::Node::Text(_) => {
                // Text-as-graphics requires rasterization
                true
            }
            usvg::Node::Group(g) => check_group(g),
        }
    }

    fn check_group(group: &usvg::Group) -> bool {
        // Check if group has filters, masks, or other complex features
        // Note: usvg pre-flattens many attributes, so we check children
        for child in group.children() {
            if check_node(&child) {
                return true;
            }
        }
        false
    }

    let requires_raster = check_group(tree.root());
    Some(requires_raster)
}
