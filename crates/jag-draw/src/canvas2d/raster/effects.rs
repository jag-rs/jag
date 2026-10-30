//! Ordered source -> filter -> shadow -> clipped destination. Scratch is per call.
use super::{Draw, Mask, Paint, PatternImage, Raster, Transform, blend, filters, geometry, rule};
use serde::Deserialize;
use std::collections::HashMap;
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Effects {
    pub composite: String,
    pub shadow: [f64; 4],
    pub blur: f64,
    pub offset: [f64; 2],
    pub filters: Vec<filters::Filter>,
}
impl Default for Effects {
    fn default() -> Self {
        Self {
            composite: "source-over".into(),
            shadow: [0.; 4],
            blur: 0.,
            offset: [0.; 2],
            filters: vec![],
        }
    }
}
impl Effects {
    pub fn valid(&self) -> bool {
        blend::MODES.contains(&self.composite.as_str())
            && self
                .shadow
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self.blur.is_finite()
            && self.blur >= 0.
            && self.offset.iter().all(|v| v.is_finite())
            && self.filters.len() <= 64
            && self.filters.iter().all(filters::Filter::valid)
    }
    fn shadowed(&self) -> bool {
        self.shadow[3] > 0. && (self.blur > 0. || self.offset != [0.; 2])
    }
    pub fn active(&self) -> bool {
        self.composite != "source-over" || self.shadowed() || !self.filters.is_empty()
    }
}
impl Raster {
    /// Admission happens before changing destination pixels. Failure is explicit
    /// at the native binding; no truncated effect, historical command or scratch is retained.
    pub fn draw_checked(
        &mut self,
        op: &Draw,
        images: &HashMap<u64, PatternImage>,
    ) -> Result<(), &'static str> {
        if !op.valid() {
            return Err("QuotaExceededError");
        }
        if op.clear || !op.effects.active() {
            self.draw(op, images);
            return Ok(());
        }
        if geometry::path(&op.path).is_none() {
            return Ok(());
        }
        let e = &op.effects;
        let shadowed = e.shadowed();
        let padding = e.filters.iter().map(filters::Filter::pad).sum::<f64>()
            + if shadowed {
                e.blur.min(512.) * 1.5 + e.offset[0].abs().max(e.offset[1].abs()) + 2.
            } else {
                0.
            };
        if padding > 8192. {
            return Err("QuotaExceededError");
        }
        let pad = padding.ceil() as u32;
        let (w, h) = (self.width + 2 * pad, self.height + 2 * pad);
        let bytes_per_pixel = if shadowed
            || e.filters
                .iter()
                .any(|f| matches!(f.kind.as_str(), "blur" | "drop-shadow"))
        {
            136
        } else {
            40
        };
        if u64::from(w) * u64::from(h) * bytes_per_pixel > 256 * 1024 * 1024
            || w > 16384
            || h > 16384
        {
            return Err("QuotaExceededError");
        }
        // Both source geometry and paint spaces shift together. Input is not
        // clipped to the destination: off-bitmap content can cast a visible shadow.
        let mut source = Raster::new(w, h, true).ok_or("QuotaExceededError")?;
        let mut local = op.clone();
        local.clips.clear();
        local.effects = Effects::default();
        local.transform[4] += pad as f64;
        local.transform[5] += pad as f64;
        for c in &mut local.path {
            for p in c[1..].chunks_exact_mut(2) {
                p[0] += pad as f64;
                p[1] += pad as f64;
            }
        }
        match &mut local.paint {
            Paint::Solid { .. } => {}
            Paint::Linear { matrix, .. }
            | Paint::Radial { matrix, .. }
            | Paint::Conic { matrix, .. }
            | Paint::Pattern { matrix, .. }
            | Paint::Image { matrix, .. } => {
                matrix[4] += pad as f64;
                matrix[5] += pad as f64;
            }
        }
        source.draw(&local, images);
        let mut pixels: Vec<filters::Pixel> = source
            .pixels
            .chunks_exact(4)
            .map(|p| std::array::from_fn(|i| p[i] as f64 / 255.))
            .collect();
        drop(source);
        for filter in &e.filters {
            filters::apply(&mut pixels, w as usize, h as usize, filter);
        }
        let mut clip = Mask::new(self.width, self.height).ok_or("QuotaExceededError")?;
        clip.data_mut().fill(255);
        for c in &op.clips {
            if let Some(p) = geometry::path(&c.path) {
                clip.intersect_path(&p, rule(&c.rule), true, Transform::identity());
            } else {
                clip.data_mut().fill(0);
                break;
            }
        }
        if shadowed {
            let shadow = filters::shadow(
                &pixels,
                w as usize,
                h as usize,
                e.blur / 2.,
                e.offset,
                e.shadow,
            );
            self.composite_layer(&shadow, w, pad, &clip, &e.composite);
        }
        self.composite_layer(&pixels, w, pad, &clip, &e.composite);
        Ok(())
    }
    fn composite_layer(
        &mut self,
        pixels: &[filters::Pixel],
        width: u32,
        pad: u32,
        clip: &Mask,
        mode: &str,
    ) {
        for (i, d) in self.pixels.chunks_exact_mut(4).enumerate() {
            let k = clip.data()[i] as f64 / 255.;
            if k == 0. {
                continue;
            }
            let x = i % self.width as usize;
            let y = i / self.width as usize;
            let s = pixels[(y + pad as usize) * width as usize + x + pad as usize];
            let dest = std::array::from_fn(|j| d[j] as f64 / 255.);
            let out = blend::composite(s, dest, mode);
            for j in 0..4 {
                d[j] = ((out[j] * k + dest[j] * (1. - k)) * 255.)
                    .round()
                    .clamp(0., 255.) as u8;
            }
            if !self.alpha {
                d[3] = 255;
            }
        }
    }
}
