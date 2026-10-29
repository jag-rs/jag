//! Native coverage/paint frontend, reusing JAG's tiny-skia SVG raster machinery.
//! Browser callbacks never run here. Pixels are premultiplied sRGB.
use super::geometry::{self, Commands, StrokeStyle};
use serde::Deserialize;
use std::sync::Arc;
use tiny_skia::{FillRule, Mask, Transform};
mod paint;
pub use paint::{Paint, PatternImage};
#[derive(Clone, Debug, Deserialize)]
pub struct Clip {
    pub path: Commands,
    pub rule: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Draw {
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
        let Some(mut mask) = Mask::new(self.width, self.height) else {
            return;
        };
        mask.fill_path(&path, rule(&op.rule), true, Transform::identity());
        for clip in &op.clips {
            if let Some(p) = geometry::path(&clip.path) {
                mask.intersect_path(&p, rule(&clip.rule), true, Transform::identity());
            } else {
                return;
            }
        }
        for (i, coverage) in mask.data().iter().copied().enumerate() {
            if coverage == 0 {
                continue;
            }
            let dest = &mut self.pixels[i * 4..i * 4 + 4];
            let k = f64::from(coverage) / 255.;
            let c = if op.clear {
                [0., 0., 0., k]
            } else {
                let c = op.paint.sample(
                    (i % self.width as usize) as f64 + 0.5,
                    (i / self.width as usize) as f64 + 0.5,
                    images,
                );
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
fn rule(s: &str) -> FillRule {
    if s == "evenodd" {
        FillRule::EvenOdd
    } else {
        FillRule::Winding
    }
}
impl Draw {
    pub fn valid(&self) -> bool {
        self.alpha.is_finite()
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
