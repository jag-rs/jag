use jag_draw::{Brush, FillRule, Path, PathCmd, Rect, RoundedRect};

use crate::canvas::Canvas;

#[derive(Clone, Copy, Debug, Default)]
pub struct BorderWidths {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Debug)]
pub struct BorderStyle {
    pub widths: BorderWidths,
    pub brush: Brush,
}

#[derive(Clone, Debug, Default)]
pub struct RectStyle {
    pub fill: Option<Brush>,
    pub border: Option<BorderStyle>,
}

fn snapped_rect(canvas: &Canvas, x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    let snapped = canvas.snap_rect_logical_to_device(Rect { x, y, w, h });
    (snapped.x, snapped.y, snapped.w, snapped.h)
}

/// Public helper for snapping a rect to device pixels using the canvas DPI.
pub fn snap_rect_to_device(canvas: &Canvas, rect: Rect) -> Rect {
    canvas.snap_rect_logical_to_device(rect)
}

/// Draw a rectangle with optional fill and per-side border widths,
/// snapping edges to device pixels for crisp 1px borders.
pub fn draw_snapped_rectangle(
    canvas: &mut Canvas,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    style: &RectStyle,
    z: i32,
) {
    let (x, y, w, h) = snapped_rect(canvas, x, y, w, h);
    draw_rectangle(canvas, x, y, w, h, style, z);
}

/// Draw a rectangle with optional fill and per-side border widths.
pub fn draw_rectangle(
    canvas: &mut Canvas,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    style: &RectStyle,
    z: i32,
) {
    if let Some(fill) = &style.fill {
        canvas.fill_rect(x, y, w, h, fill.clone(), z);
    }
    if let Some(border) = &style.border {
        let b = &border.widths;
        let brush = border.brush.clone();
        if b.top > 0.0 {
            canvas.fill_rect(x, y, w, b.top, brush.clone(), z + 1);
        }
        if b.right > 0.0 {
            canvas.fill_rect(x + w - b.right, y, b.right, h, brush.clone(), z + 1);
        }
        if b.bottom > 0.0 {
            canvas.fill_rect(x, y + h - b.bottom, w, b.bottom, brush.clone(), z + 1);
        }
        if b.left > 0.0 {
            canvas.fill_rect(x, y, b.left, h, brush, z + 1);
        }
    }
}

/// Draw a rounded rectangle with optional fill and uniform stroke/border.
pub fn draw_rounded_rectangle(
    canvas: &mut Canvas,
    rrect: RoundedRect,
    fill: Option<Brush>,
    stroke_width: Option<f32>,
    stroke_brush: Option<Brush>,
    z: i32,
) {
    if let Some(f) = fill {
        canvas.rounded_rect(rrect, f, z);
    }
    if let (Some(w), Some(b)) = (stroke_width, stroke_brush) {
        canvas.stroke_rounded_rect(rrect, w, b, z + 1);
    }
}

/// Draw a rounded rectangle with edges snapped to device pixels.
///
/// The rounded-rect radii are preserved; only the outer rect is snapped so
/// borders land cleanly on integral physical pixels (especially important
/// on Windows with fractional DPI scaling).
pub fn draw_snapped_rounded_rectangle(
    canvas: &mut Canvas,
    mut rrect: RoundedRect,
    fill: Option<Brush>,
    stroke_width: Option<f32>,
    stroke_brush: Option<Brush>,
    z: i32,
) {
    let (x, y, w, h) = snapped_rect(
        canvas,
        rrect.rect.x,
        rrect.rect.y,
        rrect.rect.w,
        rrect.rect.h,
    );
    rrect.rect.x = x;
    rrect.rect.y = y;
    rrect.rect.w = w;
    rrect.rect.h = h;
    draw_rounded_rectangle(canvas, rrect, fill, stroke_width, stroke_brush, z);
}

/// Draw a circle with optional fill and stroke. Stroke supports solid color only.
pub fn draw_circle(
    canvas: &mut Canvas,
    center: [f32; 2],
    radius: f32,
    fill: Option<Brush>,
    stroke_width: Option<f32>,
    stroke_brush: Option<Brush>,
    z: i32,
) {
    if let Some(f) = fill {
        canvas.circle(center, radius, f, z);
    }
    if let (Some(w), Some(sb)) = (stroke_width, stroke_brush) {
        // Only solid strokes are supported for path-based circle stroke
        if let Brush::Solid(col) = sb {
            let segs = 48u32;
            let mut path = Path {
                cmds: Vec::new(),
                fill_rule: FillRule::NonZero,
            };
            let mut first = true;
            for i in 0..=segs {
                let t = (i as f32) / (segs as f32);
                let ang = std::f32::consts::TAU * t;
                let x = center[0] + radius * ang.cos();
                let y = center[1] + radius * ang.sin();
                if first {
                    path.cmds.push(PathCmd::MoveTo([x, y]));
                    first = false;
                } else {
                    path.cmds.push(PathCmd::LineTo([x, y]));
                }
            }
            path.cmds.push(PathCmd::Close);
            canvas.stroke_path(path, w, col, z + 1);
        }
    }
}

/// Draw an ellipse with optional fill and stroke. Stroke supports solid color only.
pub fn draw_ellipse(
    canvas: &mut Canvas,
    center: [f32; 2],
    radii: [f32; 2],
    fill: Option<Brush>,
    stroke_width: Option<f32>,
    stroke_brush: Option<Brush>,
    z: i32,
) {
    if let Some(f) = fill {
        canvas.ellipse(center, radii, f, z);
    }
    if let (Some(w), Some(sb)) = (stroke_width, stroke_brush) {
        // Only solid strokes are supported for path-based ellipse stroke
        if let Brush::Solid(col) = sb {
            let segs = 64u32;
            let mut path = Path {
                cmds: Vec::new(),
                fill_rule: FillRule::NonZero,
            };
            let mut first = true;
            for i in 0..=segs {
                let t = (i as f32) / (segs as f32);
                let ang = std::f32::consts::TAU * t;
                let x = center[0] + radii[0] * ang.cos();
                let y = center[1] + radii[1] * ang.sin();
                if first {
                    path.cmds.push(PathCmd::MoveTo([x, y]));
                    first = false;
                } else {
                    path.cmds.push(PathCmd::LineTo([x, y]));
                }
            }
            path.cmds.push(PathCmd::Close);
            canvas.stroke_path(path, w, col, z + 1);
        }
    }
}
