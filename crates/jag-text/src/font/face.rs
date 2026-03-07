use std::sync::Arc;

use swash::scale::image::Image;
use swash::scale::outline::Outline;
use swash::scale::{ScaleContext, StrikeWith};
use swash::{FontRef, GlyphId, Metrics};

use crate::font::{FontError, FontMetrics, Result, ScaledFontMetrics};

/// Loaded font face backed by a font file (TTF/OTF).
///
/// This is a thin wrapper around `swash::FontRef` that owns the
/// underlying font data and exposes high-level metrics and glyph
/// outline/bitmap helpers.
#[derive(Debug, Clone)]
pub struct FontFace {
    /// Full font data.
    data: Arc<[u8]>,
    /// Font index within a TTC/OTC collection (0 for non-collections).
    index: usize,
    /// Offset to the table directory for this font.
    offset: u32,
    /// Cache key used internally by swash.
    key: swash::CacheKey,
    /// Extracted font metrics in font units.
    metrics: FontMetrics,
}

impl FontFace {
    /// Create a font face from raw bytes and a font index within the file.
    pub fn from_bytes(data: Arc<[u8]>, index: usize) -> Result<Self> {
        let font = FontRef::from_index(&data, index).ok_or(FontError::InvalidFont)?;
        let metrics = Self::metrics_from_swash(&font);
        let (offset, key) = (font.offset, font.key);
        Ok(Self {
            data,
            index,
            offset,
            key,
            metrics,
        })
    }

    /// Create a font face from raw bytes owned by a `Vec<u8>`.
    pub fn from_vec(data: Vec<u8>, index: usize) -> Result<Self> {
        Self::from_bytes(Arc::from(data), index)
    }

    /// Create a font face from a font file on disk.
    pub fn from_path(path: impl AsRef<std::path::Path>, index: usize) -> Result<Self> {
        let data = std::fs::read(path)?;
        Self::from_vec(data, index)
    }

    /// Expose the raw font bytes for integration with other libraries
    /// that take ownership of the font data (e.g. harfbuzz_rs).
    pub fn as_bytes(&self) -> Arc<[u8]> {
        self.data.clone()
    }

    /// Return the face index used when loading this font.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Return a transient `FontRef` for interacting with swash APIs.
    fn as_swash_ref(&self) -> FontRef<'_> {
        FontRef {
            data: &self.data,
            offset: self.offset,
            key: self.key,
        }
    }

    fn metrics_from_swash(font: &FontRef<'_>) -> FontMetrics {
        // Use default (no variation) coordinates.
        let Metrics {
            units_per_em,
            ascent,
            descent,
            leading,
            cap_height,
            x_height,
            ..
        } = font.metrics(&[]);

        FontMetrics {
            ascent,
            descent,
            line_gap: leading,
            units_per_em,
            cap_height: Some(cap_height),
            x_height: Some(x_height),
        }
    }

    /// Font metrics in font units.
    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    /// Font metrics scaled to the requested pixel size (px per em).
    pub fn scaled_metrics(&self, font_size: f32) -> ScaledFontMetrics {
        self.metrics.scale_to_pixels(font_size)
    }

    /// Font metrics for a point size at a given DPI.
    pub fn scaled_metrics_from_points(&self, font_size_pt: f32, dpi: f32) -> ScaledFontMetrics {
        self.metrics.scale_from_points(font_size_pt, dpi)
    }

    /// Convert a glyph id to a scaled outline at the specified size.
    pub fn glyph_outline(&self, glyph_id: GlyphId, font_size: f32) -> Option<Outline> {
        let mut context = ScaleContext::new();
        let font = self.as_swash_ref();
        let mut scaler = context.builder(font).size(font_size).build();
        scaler.scale_outline(glyph_id)
    }

    /// Convert a glyph id to an alpha bitmap at the specified size.
    pub fn glyph_bitmap(&self, glyph_id: GlyphId, font_size: f32) -> Option<Image> {
        let mut context = ScaleContext::new();
        let font = self.as_swash_ref();
        let mut scaler = context.builder(font).size(font_size).build();
        scaler.scale_bitmap(glyph_id, StrikeWith::BestFit)
    }

    /// Convert a glyph id to a color bitmap at the specified size, if available.
    pub fn glyph_color_bitmap(&self, glyph_id: GlyphId, font_size: f32) -> Option<Image> {
        let mut context = ScaleContext::new();
        let font = self.as_swash_ref();
        let mut scaler = context.builder(font).size(font_size).build();
        scaler.scale_color_bitmap(glyph_id, StrikeWith::BestFit)
    }
}
