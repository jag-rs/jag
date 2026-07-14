use jag_draw::{Brush, ExternalTextureId, FilterEffect, MaskEffect, MaskMode, Rect};

use super::{Canvas, GeneratedMaskTexture};

impl Canvas {
    /// Resolve a linear-gradient brush into a texture and begin an owned mask scope.
    /// Returns false for unsupported brushes or non-axis-aligned transforms.
    pub fn push_generated_mask(&mut self, rect: Rect, brush: &Brush, mode: MaskMode) -> bool {
        let Brush::LinearGradient { start, end, stops } = brush else {
            return false;
        };
        let [a, b, c, d, e, f] = self.current_transform().m;
        if b.abs() > f32::EPSILON || c.abs() > f32::EPSILON || a == 0.0 || d == 0.0 {
            return false;
        }
        let map = |p: [f32; 2]| [a * p[0] + e, d * p[1] + f];
        let p0 = map([rect.x, rect.y]);
        let p1 = map([rect.x + rect.w, rect.y + rect.h]);
        let mapped_rect = Rect {
            x: p0[0].min(p1[0]),
            y: p0[1].min(p1[1]),
            w: (p1[0] - p0[0]).abs(),
            h: (p1[1] - p0[1]).abs(),
        };
        if mapped_rect.w <= 0.0 || mapped_rect.h <= 0.0 || stops.is_empty() {
            return false;
        }
        let mapped_start = map(*start);
        let mapped_end = map(*end);
        let width = mapped_rect.w.ceil().max(1.0) as u32;
        let height = mapped_rect.h.ceil().max(1.0) as u32;
        let pixels =
            raster_linear_gradient(mapped_rect, width, height, mapped_start, mapped_end, stops);
        let id = ExternalTextureId(self.next_generated_mask_texture_id);
        self.next_generated_mask_texture_id = self.next_generated_mask_texture_id.wrapping_add(1);
        self.generated_mask_textures.push(GeneratedMaskTexture {
            id,
            width,
            height,
            pixels,
        });
        self.push_filter(FilterEffect::Mask(MaskEffect {
            texture_id: id,
            mode,
            rect: mapped_rect,
        }));
        true
    }
}

fn raster_linear_gradient(
    rect: Rect,
    width: u32,
    height: u32,
    start: [f32; 2],
    end: [f32; 2],
    stops: &[(f32, jag_draw::ColorLinPremul)],
) -> Vec<u8> {
    let delta = [end[0] - start[0], end[1] - start[1]];
    let length2 = (delta[0] * delta[0] + delta[1] * delta[1]).max(f32::EPSILON);
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let p = [
                rect.x + (x as f32 + 0.5) * rect.w / width as f32,
                rect.y + (y as f32 + 0.5) * rect.h / height as f32,
            ];
            let t = (((p[0] - start[0]) * delta[0] + (p[1] - start[1]) * delta[1]) / length2)
                .clamp(0.0, 1.0);
            pixels.extend_from_slice(&sample_stops(stops, t));
        }
    }
    pixels
}

fn sample_stops(stops: &[(f32, jag_draw::ColorLinPremul)], t: f32) -> [u8; 4] {
    let upper = stops
        .iter()
        .position(|stop| stop.0 >= t)
        .unwrap_or(stops.len() - 1);
    let lower = upper.saturating_sub(1);
    let span = stops[upper].0 - stops[lower].0;
    let mix = if span > f32::EPSILON {
        ((t - stops[lower].0) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let a = stops[lower].1.to_srgba_u8();
    let b = stops[upper].1.to_srgba_u8();
    std::array::from_fn(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * mix).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::sample_stops;
    use jag_draw::ColorLinPremul;

    #[test]
    fn samples_linear_gradient_midpoint_in_srgb() {
        let stops = [
            (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
            (1.0, ColorLinPremul::from_srgba_u8([255, 255, 255, 255])),
        ];
        assert_eq!(sample_stops(&stops, 0.5), [128; 4]);
    }
}
