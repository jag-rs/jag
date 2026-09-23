//! Brush-filled text (CSS `background-clip: text`): tints rasterized glyph
//! masks by sampling the fill brush at each pixel's position in the run's
//! local space, so the gradient follows the text through scroll, opacity,
//! and transform layers.

use crate::scene::{Brush, ColorLinPremul, Transform2D};
use crate::text::{ColorMask, GlyphMask, MaskFormat, RasterizedGlyph};

use super::gradients::sample_gradient_stops;
use super::types::ExtractedTextDraw;

const WHITE: ColorLinPremul = ColorLinPremul {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

impl ExtractedTextDraw {
    /// Glyph and vertex color to submit for one rasterized glyph of this run.
    ///
    /// `origin` is the glyph's top-left in world logical pixels and `dpi` the
    /// device scale the mask was rasterized at. A filled run returns a
    /// pre-tinted color mask with a white vertex color.
    pub fn glyph_for_draw(
        &self,
        glyph: &RasterizedGlyph,
        origin: [f32; 2],
        dpi: f32,
    ) -> (RasterizedGlyph, ColorLinPremul) {
        match &self.fill {
            Some(fill) => (
                tint_glyph_with_brush(glyph, origin, dpi, self.transform, fill),
                WHITE,
            ),
            None => (glyph.clone(), self.run.color),
        }
    }
}

/// Premultiplied linear color of `brush` at `p` (in the brush's space).
pub(crate) fn brush_color_at(brush: &Brush, p: [f32; 2]) -> [f32; 4] {
    let (t, stops) = match brush {
        Brush::Solid(c) => return [c.r, c.g, c.b, c.a],
        Brush::LinearGradient { start, end, stops } => {
            let axis = [end[0] - start[0], end[1] - start[1]];
            let len_sq = axis[0] * axis[0] + axis[1] * axis[1];
            let t = if len_sq <= f32::EPSILON {
                0.0
            } else {
                ((p[0] - start[0]) * axis[0] + (p[1] - start[1]) * axis[1]) / len_sq
            };
            (t, stops)
        }
        Brush::RadialGradient {
            center,
            radius,
            stops,
        } => {
            let dist = ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2)).sqrt();
            (dist / radius.abs().max(1e-6), stops)
        }
        Brush::ConicGradient {
            center,
            start_angle,
            stops,
        } => {
            // 0 = north, clockwise — the same convention as the rect painter.
            let angle = (p[0] - center[0]).atan2(center[1] - p[1]);
            let tau = std::f32::consts::TAU;
            ((angle - start_angle).rem_euclid(tau) / tau, stops)
        }
    };
    let packed: Vec<(f32, [f32; 4])> = stops
        .iter()
        .map(|(pos, c)| (*pos, [c.r, c.g, c.b, c.a]))
        .collect();
    sample_gradient_stops(&packed, t)
}

fn invert(t: Transform2D) -> Option<Transform2D> {
    let [a, b, c, d, e, f] = t.m;
    let det = a * d - b * c;
    if det.abs() <= f32::EPSILON {
        return None;
    }
    let inv = 1.0 / det;
    Some(Transform2D {
        m: [
            d * inv,
            -b * inv,
            -c * inv,
            a * inv,
            (c * f - d * e) * inv,
            (b * e - a * f) * inv,
        ],
    })
}

/// Pixel coverage of a glyph mask pixel, 0..=1.
fn coverage(mask: &GlyphMask, idx: usize) -> f32 {
    match mask {
        GlyphMask::Color(m) => m.data[idx * 4 + 3] as f32 / 255.0,
        GlyphMask::Subpixel(m) => match m.format {
            MaskFormat::Rgba8 => {
                let px = &m.data[idx * 4..idx * 4 + 3];
                px[0].max(px[1]).max(px[2]) as f32 / 255.0
            }
            MaskFormat::Rgba16 => {
                let px = &m.data[idx * 8..idx * 8 + 6];
                let channel = |i: usize| u16::from_le_bytes([px[i], px[i + 1]]);
                channel(0).max(channel(2)).max(channel(4)) as f32 / 65535.0
            }
        },
    }
}

/// Replace a glyph's color with `brush`, keeping its coverage.
///
/// Each mask pixel center is mapped from world logical space back through
/// `transform` into the brush's space. A color glyph (emoji) is used for its
/// alpha only, as CSS paints the background through the glyph shape.
fn tint_glyph_with_brush(
    glyph: &RasterizedGlyph,
    origin: [f32; 2],
    dpi: f32,
    transform: Transform2D,
    brush: &Brush,
) -> RasterizedGlyph {
    let width = glyph.mask.width();
    let height = glyph.mask.height();
    let (w, h) = (width as usize, height as usize);
    let mut data = vec![0u8; w * h * 4];
    // A non-invertible transform collapses the run to zero area: paint nothing.
    if let Some(to_local) = invert(transform) {
        let [a, b, c, d, e, f] = to_local.m;
        for row in 0..h {
            for col in 0..w {
                let idx = row * w + col;
                let cov = coverage(&glyph.mask, idx);
                if cov == 0.0 {
                    continue;
                }
                let x = origin[0] + (col as f32 + 0.5) / dpi;
                let y = origin[1] + (row as f32 + 0.5) / dpi;
                let local = [a * x + c * y + e, b * x + d * y + f];
                let color = brush_color_at(brush, local);
                for (channel, value) in color.iter().enumerate() {
                    data[idx * 4 + channel] = (value * cov * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    RasterizedGlyph {
        offset: glyph.offset,
        mask: GlyphMask::Color(ColorMask {
            width,
            height,
            data,
        }),
    }
}

#[cfg(test)]
#[path = "text_fill_tests.rs"]
mod tests;
