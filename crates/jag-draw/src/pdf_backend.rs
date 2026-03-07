//! PDF backend for engine-core display lists using `pdf-writer`.
//!
//! This focuses on a minimal, robust subset of drawing commands:
//! rects, rounded rects (approximated), solid fills, simple strokes,
//! ellipses (approximated), and basic text. Box-shadows, images,
//! SVGs, and complex clipping are currently ignored.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use pdf_writer::{Content, Finish, Name, Pdf, Rect as PdfRect, Ref, Str};

use crate::display_list::{Command, DisplayList};
use crate::scene::{Brush, ColorLinPremul, Path as ScenePath, PathCmd, Transform2D};

/// Render a `DisplayList` to a single-page PDF using `pdf-writer`.
///
/// - Page size is taken from `list.viewport` (logical pixels).
/// - Coordinates are mapped 1:1 to PDF user units with origin at top-left.
pub fn render_display_list_to_pdf(list: &DisplayList, output: &Path) -> Result<()> {
    let width = list.viewport.width.max(1) as f32;
    let height = list.viewport.height.max(1) as f32;

    let file = File::create(output)
        .with_context(|| format!("Failed to create PDF file at {}", output.display()))?;
    let mut writer = BufWriter::new(file);

    let mut pdf = Pdf::new();

    // Basic objects
    let catalog_ref = Ref::new(1);
    let pages_ref = Ref::new(2);
    let page_ref = Ref::new(3);
    let contents_ref = Ref::new(4);

    // Simple font: built-in Helvetica
    let font_ref = Ref::new(10);

    // Catalog
    pdf.catalog(catalog_ref).pages(pages_ref);

    // Pages
    pdf.pages(pages_ref).kids([page_ref]).count(1);

    // Page
    pdf.page(page_ref)
        .parent(pages_ref)
        .media_box(PdfRect {
            x1: 0.0,
            y1: 0.0,
            x2: width,
            y2: height,
        })
        .contents(contents_ref)
        .resources()
        .fonts()
        .pair(Name(b"F1"), font_ref)
        .finish()
        .finish();

    // Font object
    {
        let mut font = pdf.type1_font(font_ref);
        font.base_font(Name(b"Helvetica"));
        font.finish();
    }

    // Content stream
    let mut content = Content::new();

    // White background
    content.set_fill_rgb(1.0, 1.0, 1.0);
    content.rect(0.0, 0.0, width, height);
    content.fill_nonzero();

    render_commands_to_content(&mut content, list, font_ref);

    let data = content.finish();
    pdf.stream(contents_ref, &data);

    let bytes = pdf.finish();
    writer.write_all(&bytes)?;
    Ok(())
}

fn render_commands_to_content(content: &mut Content, list: &DisplayList, _font_ref: Ref) {
    for cmd in &list.commands {
        match cmd {
            Command::DrawRect {
                rect,
                brush,
                transform,
                ..
            } => {
                if let Some(color) = brush_solid_color(brush) {
                    let (x, y) = apply_translation(rect.x, rect.y, *transform);
                    set_fill_color(content, color);
                    content.rect(x, y, rect.w, rect.h);
                    content.fill_nonzero();
                }
            }
            Command::DrawRoundedRect {
                rrect,
                brush,
                transform,
                ..
            } => {
                if let Some(color) = brush_solid_color(brush) {
                    let rect = rrect.rect;
                    let (x, y) = apply_translation(rect.x, rect.y, *transform);
                    set_fill_color(content, color);
                    content.rect(x, y, rect.w, rect.h);
                    content.fill_nonzero();
                }
            }
            Command::StrokeRect {
                rect,
                stroke,
                brush,
                transform,
                ..
            } => {
                if let Some(color) = brush_solid_color(brush) {
                    let (x, y) = apply_translation(rect.x, rect.y, *transform);
                    set_stroke_color(content, color);
                    content.set_line_width(stroke.width.max(0.0));
                    content.rect(x, y, rect.w, rect.h);
                    content.stroke();
                }
            }
            Command::StrokeRoundedRect {
                rrect,
                stroke,
                brush,
                transform,
                ..
            } => {
                if let Some(color) = brush_solid_color(brush) {
                    let rect = rrect.rect;
                    let (x, y) = apply_translation(rect.x, rect.y, *transform);
                    set_stroke_color(content, color);
                    content.set_line_width(stroke.width.max(0.0));
                    content.rect(x, y, rect.w, rect.h);
                    content.stroke();
                }
            }
            Command::DrawEllipse {
                center,
                radii,
                brush,
                transform,
                ..
            } => {
                if let Some(color) = brush_solid_color(brush) {
                    // Approximate ellipse with a rectangle bounds for now.
                    let (cx, cy) = apply_translation(center[0], center[1], *transform);
                    let x = cx - radii[0];
                    let y = cy - radii[1];
                    set_fill_color(content, color);
                    content.rect(x, y, radii[0] * 2.0, radii[1] * 2.0);
                    content.fill_nonzero();
                }
            }
            Command::FillPath {
                path,
                color,
                transform,
                ..
            } => {
                set_fill_color(content, *color);
                draw_path(content, path, *transform);
                content.fill_nonzero();
            }
            Command::StrokePath {
                path,
                stroke,
                color,
                transform,
                ..
            } => {
                set_stroke_color(content, *color);
                content.set_line_width(stroke.width.max(0.0));
                draw_path(content, path, *transform);
                content.stroke();
            }
            Command::DrawText { run, transform, .. } => {
                let (x, y) = apply_translation(run.pos[0], run.pos[1], *transform);
                set_fill_color(content, run.color);
                content.begin_text();
                content.set_font(Name(b"F1"), run.size.max(1.0));
                content.set_text_matrix([1.0, 0.0, 0.0, 1.0, x, y]);
                content.show(Str(run.text.as_bytes()));
                content.end_text();
            }
            Command::DrawHyperlink {
                hyperlink,
                transform,
                ..
            } => {
                let (x, y) = apply_translation(hyperlink.pos[0], hyperlink.pos[1], *transform);
                set_fill_color(content, hyperlink.color);
                content.begin_text();
                content.set_font(Name(b"F1"), hyperlink.size.max(1.0));
                content.set_text_matrix([1.0, 0.0, 0.0, 1.0, x, y]);
                content.show(Str(hyperlink.text.as_bytes()));
                content.end_text();

                if hyperlink.underline {
                    let underline_color = hyperlink.underline_color.unwrap_or(hyperlink.color);
                    set_stroke_color(content, underline_color);
                    let thickness = (hyperlink.size * 0.05).max(0.5);
                    content.set_line_width(thickness);
                    let y_ul = y - hyperlink.size * 0.15;
                    let width = hyperlink
                        .measured_width
                        .unwrap_or_else(|| hyperlink.size * hyperlink.text.len() as f32 * 0.5);
                    content.move_to(x, y_ul);
                    content.line_to(x + width, y_ul);
                    content.stroke();
                }
            }
            // Box shadows, images, SVGs, hit-only regions, and transform/clip stack
            // are ignored in this initial backend.
            Command::BoxShadow { .. }
            | Command::HitRegionRect { .. }
            | Command::HitRegionRoundedRect { .. }
            | Command::HitRegionEllipse { .. }
            | Command::DrawSvg { .. }
            | Command::DrawImage { .. }
            | Command::PushClip(_)
            | Command::PopClip
            | Command::PushTransform(_)
            | Command::PopTransform => {}
        }
    }
}

fn brush_solid_color(brush: &Brush) -> Option<ColorLinPremul> {
    match brush {
        Brush::Solid(c) => Some(*c),
        Brush::LinearGradient { stops, .. } => stops.last().map(|(_, c)| *c),
        Brush::RadialGradient { stops, .. } => stops.last().map(|(_, c)| *c),
    }
}

fn set_fill_color(content: &mut Content, color: ColorLinPremul) {
    let [r, g, b, _a] = color.to_srgba_u8();
    content.set_fill_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
}

fn set_stroke_color(content: &mut Content, color: ColorLinPremul) {
    let [r, g, b, _a] = color.to_srgba_u8();
    content.set_stroke_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
}

fn apply_translation(x: f32, y: f32, t: Transform2D) -> (f32, f32) {
    let [a, b, c, d, e, f] = t.m;
    let tx = a * x + c * y + e;
    let ty = b * x + d * y + f;
    (tx, ty)
}

fn draw_path(content: &mut Content, path: &ScenePath, t: Transform2D) {
    let mut first = true;
    for cmd in &path.cmds {
        match *cmd {
            PathCmd::MoveTo(p) => {
                let (x, y) = apply_translation(p[0], p[1], t);
                content.move_to(x, y);
                first = false;
            }
            PathCmd::LineTo(p) => {
                if first {
                    let (x, y) = apply_translation(p[0], p[1], t);
                    content.move_to(x, y);
                    first = false;
                } else {
                    let (x, y) = apply_translation(p[0], p[1], t);
                    content.line_to(x, y);
                }
            }
            PathCmd::QuadTo(c, p) => {
                // Approximate quadratic curve using cubic by repeating control point.
                let (cx, cy) = apply_translation(c[0], c[1], t);
                let (px, py) = apply_translation(p[0], p[1], t);
                content.cubic_to(cx, cy, cx, cy, px, py);
            }
            PathCmd::CubicTo(c1, c2, p) => {
                let (c1x, c1y) = apply_translation(c1[0], c1[1], t);
                let (c2x, c2y) = apply_translation(c2[0], c2[1], t);
                let (px, py) = apply_translation(p[0], p[1], t);
                content.cubic_to(c1x, c1y, c2x, c2y, px, py);
            }
            PathCmd::Close => {
                content.close_path();
                first = true;
            }
        }
    }
}
