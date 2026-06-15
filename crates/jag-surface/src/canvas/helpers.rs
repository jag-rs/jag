use jag_draw::Rect;

/// Build a closed rectangle path (used to route a solid `fill_rect` through the
/// path-fill clipper when an active rounded clip must round the rect's corners).
pub(crate) fn rect_path(rect: Rect) -> jag_draw::Path {
    use jag_draw::{FillRule, PathCmd};
    jag_draw::Path {
        cmds: vec![
            PathCmd::MoveTo([rect.x, rect.y]),
            PathCmd::LineTo([rect.x + rect.w, rect.y]),
            PathCmd::LineTo([rect.x + rect.w, rect.y + rect.h]),
            PathCmd::LineTo([rect.x, rect.y + rect.h]),
            PathCmd::Close,
        ],
        fill_rule: FillRule::NonZero,
    }
}

/// Intersect two rectangles (device-space); returns None if they do not overlap.
/// Compute the axis-aligned bounding box of a path's control points.
pub(crate) fn path_bounds(path: &jag_draw::Path) -> Option<Rect> {
    use jag_draw::PathCmd;
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut has_points = false;

    let mut extend = |p: &[f32; 2]| {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
        has_points = true;
    };

    for cmd in &path.cmds {
        match cmd {
            PathCmd::MoveTo(p) | PathCmd::LineTo(p) => extend(p),
            PathCmd::QuadTo(a, b) => {
                extend(a);
                extend(b);
            }
            PathCmd::CubicTo(a, b, c) => {
                extend(a);
                extend(b);
                extend(c);
            }
            PathCmd::Close => {}
        }
    }

    if has_points {
        Some(Rect {
            x: min_x,
            y: min_y,
            w: max_x - min_x,
            h: max_y - min_y,
        })
    } else {
        None
    }
}

pub(crate) fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    let ax1 = a.x + a.w;
    let ay1 = a.y + a.h;
    let bx1 = b.x + b.w;
    let by1 = b.y + b.h;

    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = ax1.min(bx1);
    let y1 = ay1.min(by1);

    if x1 <= x0 || y1 <= y0 {
        None
    } else {
        Some(Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }
}

/// Clip a glyph mask to a device-space rectangle, returning a new mask and origin.
pub(crate) fn clip_glyph_to_rect(
    mask: &jag_draw::GlyphMask,
    origin: [f32; 2],
    clip: Rect,
) -> Option<(jag_draw::GlyphMask, [f32; 2])> {
    use jag_draw::{ColorMask, GlyphMask, SubpixelMask};

    let glyph_x0 = origin[0];
    let glyph_y0 = origin[1];
    let (width, height, data, bpp) = match mask {
        GlyphMask::Subpixel(m) => (m.width, m.height, &m.data, m.bytes_per_pixel()),
        GlyphMask::Color(m) => (m.width, m.height, &m.data, m.bytes_per_pixel()),
    };

    let glyph_x1 = glyph_x0 + width as f32;
    let glyph_y1 = glyph_y0 + height as f32;

    let clip_x0 = clip.x;
    let clip_y0 = clip.y;
    let clip_x1 = clip.x + clip.w;
    let clip_y1 = clip.y + clip.h;

    let ix0 = glyph_x0.max(clip_x0);
    let iy0 = glyph_y0.max(clip_y0);
    let ix1 = glyph_x1.min(clip_x1);
    let iy1 = glyph_y1.min(clip_y1);

    if ix0 >= ix1 || iy0 >= iy1 {
        return None;
    }

    // Convert intersection to pixel indices within the glyph mask.
    let start_x = ((ix0 - glyph_x0).floor().max(0.0)) as u32;
    let start_y = ((iy0 - glyph_y0).floor().max(0.0)) as u32;
    let end_x = ((ix1 - glyph_x0).ceil().min(width as f32)) as u32;
    let end_y = ((iy1 - glyph_y0).ceil().min(height as f32)) as u32;

    if end_x <= start_x || end_y <= start_y {
        return None;
    }

    let new_w = end_x - start_x;
    let new_h = end_y - start_y;

    let src_stride = width * bpp as u32;
    let dst_stride = new_w * bpp as u32;
    let mut clipped_data = vec![0u8; (new_w * new_h * bpp as u32) as usize];

    for row in 0..new_h {
        let src_y = start_y + row;
        let src_offset = (src_y * src_stride + start_x * bpp as u32) as usize;
        let dst_offset = (row * dst_stride) as usize;
        clipped_data[dst_offset..dst_offset + dst_stride as usize]
            .copy_from_slice(&data[src_offset..src_offset + dst_stride as usize]);
    }

    let clipped = match mask {
        GlyphMask::Subpixel(m) => GlyphMask::Subpixel(SubpixelMask {
            width: new_w,
            height: new_h,
            format: m.format,
            data: clipped_data,
        }),
        GlyphMask::Color(_) => GlyphMask::Color(ColorMask {
            width: new_w,
            height: new_h,
            data: clipped_data,
        }),
    };

    let new_origin = [glyph_x0 + start_x as f32, glyph_y0 + start_y as f32];
    Some((clipped, new_origin))
}

/// Pre-tint a glyph mask with per-pixel-column gradient colors.
///
/// For each pixel column in the mask, samples the gradient at that column's
/// logical x position and multiplies the mask's RGB coverage by the gradient
/// color. The result can be rendered with `color = white` so the shader
/// passes through the pre-tinted mask directly — giving smooth per-pixel
/// gradient text that matches browser `background-clip: text` rendering.
pub(crate) fn tint_glyph_mask_with_gradient(
    glyph: &jag_draw::RasterizedGlyph,
    glyph_x_device: f32,
    sf: f32,
    text_width: f32,
    grad_stops: &[(f32, [f32; 4])],
) -> jag_draw::RasterizedGlyph {
    use jag_draw::{ColorMask, GlyphMask, MaskFormat};

    let tinted_mask = match &glyph.mask {
        GlyphMask::Subpixel(mask) => {
            let bpp = mask.bytes_per_pixel();
            let w = mask.width as usize;
            let h = mask.height as usize;
            let mut data = vec![0u8; w * h * 4];

            for col in 0..w {
                // Sample gradient at this pixel column's logical x position.
                let pixel_x_logical = (glyph_x_device + col as f32) / sf;
                let t = (pixel_x_logical / text_width).clamp(0.0, 1.0);
                let [gr, gg, gb, ga] = jag_draw::sample_gradient_stops(grad_stops, t);

                for row in 0..h {
                    let idx = (row * w + col) * bpp;
                    let coverage = match mask.format {
                        MaskFormat::Rgba8 if bpp == 4 && idx + 2 < mask.data.len() => {
                            let r = mask.data[idx] as f32 / 255.0;
                            let g = mask.data[idx + 1] as f32 / 255.0;
                            let b = mask.data[idx + 2] as f32 / 255.0;
                            r.max(g).max(b)
                        }
                        MaskFormat::Rgba16 if bpp == 8 && idx + 5 < mask.data.len() => {
                            let r = u16::from_le_bytes([mask.data[idx], mask.data[idx + 1]]) as f32
                                / 65535.0;
                            let g = u16::from_le_bytes([mask.data[idx + 2], mask.data[idx + 3]])
                                as f32
                                / 65535.0;
                            let b = u16::from_le_bytes([mask.data[idx + 4], mask.data[idx + 5]])
                                as f32
                                / 65535.0;
                            r.max(g).max(b)
                        }
                        _ => 0.0,
                    };
                    let alpha = (coverage * ga).clamp(0.0, 1.0);
                    let out_idx = (row * w + col) * 4;
                    data[out_idx] = (gr * coverage * 255.0).round().clamp(0.0, 255.0) as u8;
                    data[out_idx + 1] = (gg * coverage * 255.0).round().clamp(0.0, 255.0) as u8;
                    data[out_idx + 2] = (gb * coverage * 255.0).round().clamp(0.0, 255.0) as u8;
                    data[out_idx + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }

            GlyphMask::Color(ColorMask {
                width: mask.width,
                height: mask.height,
                data,
            })
        }
        GlyphMask::Color(mask) => {
            // Color emoji — tint RGBA pixels the same way.
            let mut data = mask.data.clone();
            let w = mask.width as usize;
            let h = mask.height as usize;

            for col in 0..w {
                let pixel_x_logical = (glyph_x_device + col as f32) / sf;
                let t = (pixel_x_logical / text_width).clamp(0.0, 1.0);
                let [gr, gg, gb, ga] = jag_draw::sample_gradient_stops(grad_stops, t);

                for row in 0..h {
                    let idx = (row * w + col) * 4;
                    if idx + 3 < data.len() {
                        data[idx] = (data[idx] as f32 * gr).round().min(255.0) as u8;
                        data[idx + 1] = (data[idx + 1] as f32 * gg).round().min(255.0) as u8;
                        data[idx + 2] = (data[idx + 2] as f32 * gb).round().min(255.0) as u8;
                        data[idx + 3] = (data[idx + 3] as f32 * ga).round().min(255.0) as u8;
                    }
                }
            }

            GlyphMask::Color(ColorMask {
                width: mask.width,
                height: mask.height,
                data,
            })
        }
    };

    jag_draw::RasterizedGlyph {
        offset: glyph.offset,
        mask: tinted_mask,
    }
}
