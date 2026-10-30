//! Native coverage/paint frontend, reusing JAG's tiny-skia SVG raster machinery.
//! Browser callbacks never run here. Pixels are premultiplied sRGB.
use super::geometry::{self, Commands, StrokeStyle};
use serde::Deserialize;
use std::sync::Arc;
use tiny_skia::{FillRule, Mask, Transform};
mod blend;
mod effects;
mod filters;
pub mod image;
pub use effects::Effects;
mod paint;
#[path = "raster/pixels.rs"]
mod pixels;
pub use paint::{Paint, PatternImage};
#[derive(Clone, Debug, Deserialize)]
pub struct Clip {
    pub path: Commands,
    pub rule: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Draw {
    #[serde(default)]
    pub effects: Effects,
    pub path: Commands,
    pub rule: String,
    pub stroke: Option<StrokeStyle>,
    pub transform: [f64; 6],
    pub clips: Vec<Clip>,
    pub paint: Paint,
    pub alpha: f64,
    pub clear: bool,
}
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub alpha: bool,
    pixels: Vec<u8>,
    coverage: Mask,
}
impl Raster {
    pub fn new(width: u32, height: u32, alpha: bool) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }
        let size = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if size > 64 * 1024 * 1024 {
            return None;
        }
        let mut r = Self {
            width,
            height,
            alpha,
            pixels: vec![0; size],
            coverage: Mask::new(width, height)?,
        };
        r.clear();
        Some(r)
    }
    pub fn clear(&mut self) {
        self.pixels.fill(0);
        if !self.alpha {
            for p in self.pixels.chunks_exact_mut(4) {
                p[3] = 255;
            }
        }
    }
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
    pub fn snapshot(&self) -> PatternImage {
        PatternImage {
            width: self.width,
            height: self.height,
            pixels: Arc::from(self.pixels.clone()),
        }
    }
    pub fn draw(&mut self, op: &Draw, images: &std::collections::HashMap<u64, PatternImage>) {
        if !op.valid() {
            return;
        }
        let Some(mut path) = geometry::path(&op.path) else {
            return;
        };
        if let Some(stroke) = &op.stroke {
            let Some(p) = geometry::outline(path, stroke, op.transform) else {
                return;
            };
            path = p;
        }
        // Limit scratch coverage and pixel visits to conservative path bounds.
        // Boundary-crossing paths retain the original canvas clip rectangle:
        // changing it can alter tiny-skia's curve clipping near bitmap edges.
        let bounds = path.bounds();
        let x0 = (bounds.left().floor() - 1.).max(0.).min(self.width as f32) as u32;
        let y0 = (bounds.top().floor() - 1.).max(0.).min(self.height as f32) as u32;
        let x1 = (bounds.right().ceil() + 1.).max(0.).min(self.width as f32) as u32;
        let y1 = (bounds.bottom().ceil() + 1.)
            .max(0.)
            .min(self.height as f32) as u32;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let boundary = x0 == 0 || y0 == 0 || x1 == self.width || y1 == self.height;
        let (ox, oy, mw, mh) = if boundary {
            (0, 0, self.width, self.height)
        } else {
            (x0, y0, x1 - x0, y1 - y0)
        };
        if self.coverage.width() != mw || self.coverage.height() != mh {
            let Some(mask) = Mask::new(mw, mh) else {
                return;
            };
            self.coverage = mask;
        } else {
            self.coverage.data_mut().fill(0);
        }
        let mask = &mut self.coverage;
        let shift = Transform::from_translate(-(ox as f32), -(oy as f32));
        mask.fill_path(&path, rule(&op.rule), true, shift);
        for clip in &op.clips {
            if let Some(p) = geometry::path(&clip.path) {
                mask.intersect_path(&p, rule(&clip.rule), true, shift);
            } else {
                return;
            }
        }
        let row_constant = matches!(&op.paint, Paint::Solid { .. })
            || matches!(&op.paint,
            Paint::Linear {args, matrix, ..} if args[0] == args[2] && matrix[1] == 0.);
        for gy in y0 as usize..y1 as usize {
            let row_start = (gy - oy as usize) * mw as usize;
            let coverage_row =
                &mask.data()[row_start + (x0 - ox) as usize..row_start + (x1 - ox) as usize];
            let row_color = row_constant.then(|| {
                op.paint
                    .sample(f64::from(x0) + 0.5, gy as f64 + 0.5, images)
            });
            let opaque = row_color
                .filter(|c| c[3] * op.alpha == 1.)
                .map(|c| c.map(|v| (v * 255.).round().clamp(0., 255.) as u8));
            let pixel_start = (gy * self.width as usize + x0 as usize) * 4;
            let pixel_end = pixel_start + (x1 - x0) as usize * 4;
            let pixels = &mut self.pixels[pixel_start..pixel_end];
            for (col, (coverage, dest)) in coverage_row
                .iter()
                .copied()
                .zip(pixels.chunks_exact_mut(4))
                .enumerate()
            {
                let gx = x0 as usize + col;
                if coverage == 0 {
                    continue;
                }
                if coverage == 255 {
                    if op.clear {
                        dest.copy_from_slice(&[0, 0, 0, if self.alpha { 0 } else { 255 }]);
                        continue;
                    }
                    if let Some(color) = opaque {
                        dest.copy_from_slice(&color);
                        continue;
                    }
                }
                let k = f64::from(coverage) / 255.;
                let c = if op.clear {
                    [0., 0., 0., k]
                } else {
                    let c = row_color.unwrap_or_else(|| {
                        op.paint.sample(gx as f64 + 0.5, gy as f64 + 0.5, images)
                    });
                    let a = c[3] * op.alpha * k;
                    [c[0] * a, c[1] * a, c[2] * a, a]
                };
                for j in 0..4 {
                    let source = if op.clear { 0. } else { c[j] };
                    dest[j] = ((source + f64::from(dest[j]) / 255. * (1. - c[3])) * 255.)
                        .round()
                        .clamp(0., 255.) as u8;
                }
                if !self.alpha {
                    dest[3] = 255;
                }
            }
        }
    }
}
fn rule(s: &str) -> FillRule {
    if s == "evenodd" {
        FillRule::EvenOdd
    } else {
        FillRule::Winding
    }
}
impl Draw {
    pub fn valid(&self) -> bool {
        self.effects.valid()
            && self.alpha.is_finite()
            && (0.0..=1.0).contains(&self.alpha)
            && self.transform.iter().all(|n| n.is_finite())
            && self.path.len() <= 65536
            && self.clips.len() <= 256
            && self.clips.iter().map(|c| c.path.len()).sum::<usize>() <= 262144
            && self.stroke.as_ref().is_none_or(|s| {
                s.width.is_finite()
                    && s.width > 0.
                    && s.miter.is_finite()
                    && s.miter > 0.
                    && s.offset.is_finite()
                    && s.dash.len() <= 65536
                    && s.dash.iter().all(|n| n.is_finite() && *n >= 0.)
            })
            && self.paint.valid()
    }
}

#[cfg(test)]
#[path = "raster/crop_tests.rs"]
mod crop_tests;
#[cfg(test)]
#[path = "raster/pixels_tests.rs"]
mod pixels_tests;
