use jag_draw::{Brush, ColorLinPremul, FontStyle, RasterizedGlyph, TextRun};

use super::Canvas;
use super::helpers::clip_glyph_to_rect;

impl Canvas {
    /// Draw text using direct rasterization (recommended).
    ///
    /// This method rasterizes glyphs immediately using the text provider,
    /// bypassing complex display list paths. This is simpler and more
    /// reliable than deferred rendering.
    ///
    /// # Performance
    /// - Glyphs are shaped and rasterized on each call
    /// - Use [`TextLayoutCache`](jag_draw::TextLayoutCache) to cache wrapping computations
    /// - Debounce resize events to avoid excessive rasterization
    ///
    /// # Transform Stack
    /// The current transform is applied to position text correctly
    /// within zones (viewport, toolbar, etc.).
    ///
    /// # DPI Scaling
    /// Both position and size are automatically scaled by `self.dpi_scale`.
    ///
    /// # Example
    /// ```no_run
    /// # use jag_surface::Canvas;
    /// # use jag_draw::ColorLinPremul;
    /// # let mut canvas: Canvas = todo!();
    /// canvas.draw_text_run(
    ///     [10.0, 20.0],
    ///     "Hello, world!".to_string(),
    ///     16.0,
    ///     ColorLinPremul::rgba(255, 255, 255, 255),
    ///     10,  // z-index
    /// );
    /// ```
    pub fn draw_text_run(
        &mut self,
        origin: [f32; 2],
        text: String,
        size_px: f32,
        color: ColorLinPremul,
        z: i32,
    ) {
        // Backwards-compatible wrapper: treat as normal weight text.
        self.draw_text_run_weighted(origin, text, size_px, 400.0, color, z);
    }

    /// Draw a text run with an explicit font weight.
    ///
    /// `weight` should follow CSS semantics (100–900; 400 = normal, 700 = bold).
    pub fn draw_text_run_weighted(
        &mut self,
        origin: [f32; 2],
        text: String,
        size_px: f32,
        weight: f32,
        color: ColorLinPremul,
        z: i32,
    ) {
        self.draw_text_run_styled(
            origin,
            text,
            size_px,
            weight,
            FontStyle::Normal,
            None,
            color,
            z,
        );
    }

    /// Draw a text run with full styling options.
    ///
    /// `weight` should follow CSS semantics (100–900; 400 = normal, 700 = bold).
    /// `style` specifies normal, italic, or oblique rendering.
    /// `family` optionally overrides the font family.
    pub fn draw_text_run_styled(
        &mut self,
        origin: [f32; 2],
        text: String,
        size_px: f32,
        weight: f32,
        style: FontStyle,
        family: Option<String>,
        color: ColorLinPremul,
        z: i32,
    ) {
        // If we have a provider and we're not inside an opacity group,
        // rasterize immediately (simple, reliable).
        // Inside opacity groups we route through display-list text so the
        // whole subtree can be composited once with group alpha.
        if let Some(ref provider) = self.text_provider
            && !self.painter.has_active_effect()
        {
            // Apply current transform to origin (handles zone positioning)
            let transform = self.painter.current_transform();
            let [a, b, c, d, e, f] = transform.m;
            let transformed_origin = [
                a * origin[0] + c * origin[1] + e,
                b * origin[0] + d * origin[1] + f,
            ];

            // DPI scale for converting logical to device coordinates
            let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
                self.dpi_scale
            } else {
                1.0
            };

            // Rasterize at *physical* pixel size so glyph bitmaps map 1:1 to the
            // backbuffer. We still keep layout in logical pixels and convert
            // offsets back into logical units below.
            // Carry font weight through so providers (e.g., JagTextProvider) can approximate
            // bold/semibold rendering when available.
            let run = TextRun {
                text,
                pos: [0.0, 0.0],
                size: (size_px * sf).max(1.0),
                logical_size: size_px,
                color,
                weight,
                style,
                family,
            };

            // Rasterize glyphs, using a shared cache to avoid
            // re-rasterizing identical text every frame.
            let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &run);
            // Current effective clip rect in device coordinates, if any.
            let current_clip = self.clip_stack.last().cloned().unwrap_or(None);

            for g in glyphs.iter() {
                // Provider offsets are in *physical* pixels (due to size scaling above).
                // Convert back into logical coordinates so PassManager's logical DPI
                // scale keeps geometry/text aligned.
                let mut glyph_origin_logical = [
                    transformed_origin[0] + g.offset[0] / sf,
                    transformed_origin[1] + g.offset[1] / sf,
                ];

                // Snap small text so the resulting *physical* origin lands on whole pixels.
                if size_px <= 15.0 {
                    glyph_origin_logical[0] = (glyph_origin_logical[0] * sf).round() / sf;
                    glyph_origin_logical[1] = (glyph_origin_logical[1] * sf).round() / sf;
                }

                // For clipping, convert to device pixels using the scaled logical origin.
                let glyph_origin_device =
                    [glyph_origin_logical[0] * sf, glyph_origin_logical[1] * sf];

                if let Some(clip) = current_clip {
                    // Clip glyph to the current rect in device coordinates.
                    if let Some((clipped_mask, clipped_origin_device)) =
                        clip_glyph_to_rect(&g.mask, glyph_origin_device, clip)
                    {
                        let clipped = RasterizedGlyph {
                            offset: [0.0, 0.0],
                            mask: clipped_mask,
                        };
                        // Convert clipped origin back to logical coordinates
                        let mut clipped_origin_logical =
                            [clipped_origin_device[0] / sf, clipped_origin_device[1] / sf];
                        if size_px <= 15.0 {
                            clipped_origin_logical[0] =
                                (clipped_origin_logical[0] * sf).round() / sf;
                            clipped_origin_logical[1] =
                                (clipped_origin_logical[1] * sf).round() / sf;
                        }
                        self.glyph_draws
                            .push((clipped_origin_logical, clipped, color, z, None));
                    }
                } else {
                    self.glyph_draws
                        .push((glyph_origin_logical, g.clone(), color, z, None));
                }
            }
        } else {
            // Fallback: use display list path (complex, but kept for compatibility)
            self.painter.text(
                TextRun {
                    text,
                    pos: origin,
                    size: size_px,
                    logical_size: size_px,
                    color,
                    weight,
                    style,
                    family,
                },
                z,
            );
        }
    }

    /// Draw text whose glyphs are painted with `brush` (CSS
    /// `background-clip: text`).
    ///
    /// `brush` is in the same local space as `origin`. The run goes through
    /// the display list so the fill follows scroll, opacity, and filter layers.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_run_gradient(
        &mut self,
        origin: [f32; 2],
        text: String,
        size_px: f32,
        weight: f32,
        style: FontStyle,
        family: Option<String>,
        brush: &Brush,
        z: i32,
    ) {
        self.painter.text_with_fill(
            TextRun {
                text,
                pos: origin,
                size: size_px,
                logical_size: size_px,
                color: ColorLinPremul::default(),
                weight,
                style,
                family,
            },
            brush.clone(),
            z,
        );
    }
}
