/// Font-level metrics in font units.
#[derive(Debug, Clone, Copy)]
pub struct FontMetrics {
    /// Ascent above baseline (positive).
    pub ascent: f32,
    /// Descent below baseline (positive).
    pub descent: f32,
    /// Line gap (leading).
    pub line_gap: f32,
    /// Units per em.
    pub units_per_em: u16,
    /// Cap height (optional).
    pub cap_height: Option<f32>,
    /// X-height (optional).
    pub x_height: Option<f32>,
}

impl FontMetrics {
    /// Calculate line height (ascent + descent + line_gap).
    pub fn line_height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }

    /// Scale metrics to pixel size, where `font_size` is in logical pixels
    /// (px per em).
    pub fn scale_to_pixels(&self, font_size: f32) -> ScaledFontMetrics {
        let scale = if self.units_per_em != 0 {
            font_size / self.units_per_em as f32
        } else {
            1.0
        };
        ScaledFontMetrics {
            ascent: self.ascent * scale,
            descent: self.descent * scale,
            line_gap: self.line_gap * scale,
            font_size,
        }
    }

    /// Scale metrics for a font size specified in points at a given DPI.
    ///
    /// This is a convenience for point-based typography workflows:
    /// `px = pt * dpi / 72.0`.
    pub fn scale_from_points(&self, font_size_pt: f32, dpi: f32) -> ScaledFontMetrics {
        let px = font_size_pt * dpi / 72.0;
        self.scale_to_pixels(px)
    }
}

/// Scaled font metrics in pixels.
#[derive(Debug, Clone, Copy)]
pub struct ScaledFontMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub font_size: f32,
}
