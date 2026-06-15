use std::sync::Arc;

use jag_draw::{Brush, ColorLinPremul, FontStyle, RasterizedGlyph, TextProvider, TextRun};

use super::Canvas;
use super::helpers::clip_glyph_to_rect;

impl Canvas {
    /// Draw text directly by rasterizing immediately (simpler, bypasses display list).
    /// This is the recommended approach - it's simpler and more reliable than draw_text_run.
    pub fn draw_text_direct(
        &mut self,
        origin: [f32; 2],
        text: &str,
        size_px: f32,
        color: ColorLinPremul,
        provider: &dyn TextProvider,
        z: i32,
    ) {
        self.draw_text_direct_styled(
            origin,
            text,
            size_px,
            400.0,
            FontStyle::Normal,
            None,
            color,
            provider,
            z,
        );
    }

    /// Draw text directly with explicit font styling, bypassing the display list.
    pub fn draw_text_direct_styled(
        &mut self,
        origin: [f32; 2],
        text: &str,
        size_px: f32,
        weight: f32,
        style: FontStyle,
        family: Option<&str>,
        color: ColorLinPremul,
        provider: &dyn TextProvider,
        z: i32,
    ) {
        // Apply current transform to origin (handles zone positioning)
        let transform = self.painter.current_transform();
        let [a, b, c, d, e, f] = transform.m;
        let transformed_origin = [
            a * origin[0] + c * origin[1] + e,
            b * origin[0] + d * origin[1] + f,
        ];

        // Current effective clip rect in device coordinates, if any.
        let current_clip = self.clip_stack.last().cloned().unwrap_or(None);

        // DPI scale for converting logical to device coordinates
        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };

        // Rasterize at *physical* pixel size so small styled text matches the
        // same provider path used by display-list text.
        let run = TextRun {
            text: text.to_string(),
            pos: [0.0, 0.0],
            size: (size_px * sf).max(1.0),
            logical_size: size_px,
            color,
            weight,
            style,
            family: family.map(str::to_string),
        };

        // Rasterize glyphs, using the shared cache to avoid
        // re-rasterizing identical text every frame.
        let glyphs = jag_draw::rasterize_run_cached(provider, &run);

        for g in glyphs.iter() {
            // Provider offsets are in physical pixels; convert back into logical
            // coordinates so PassManager's DPI scaling remains the single source
            // of truth for mapping to device pixels.
            let mut glyph_origin_logical = [
                transformed_origin[0] + g.offset[0] / sf,
                transformed_origin[1] + g.offset[1] / sf,
            ];

            if size_px <= 15.0 {
                glyph_origin_logical[0] = (glyph_origin_logical[0] * sf).round() / sf;
                glyph_origin_logical[1] = (glyph_origin_logical[1] * sf).round() / sf;
            }

            // For clipping, convert to device pixels
            let glyph_origin_device = [glyph_origin_logical[0] * sf, glyph_origin_logical[1] * sf];

            if let Some(clip) = current_clip {
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
                        clipped_origin_logical[0] = (clipped_origin_logical[0] * sf).round() / sf;
                        clipped_origin_logical[1] = (clipped_origin_logical[1] * sf).round() / sf;
                    }
                    self.glyph_draws
                        .push((clipped_origin_logical, clipped, color, z, None));
                }
            } else {
                self.glyph_draws
                    .push((glyph_origin_logical, g.clone(), color, z, None));
            }
        }
    }

    /// Provide a text provider used for high-level text runs in this frame.
    pub fn set_text_provider(&mut self, provider: Arc<dyn TextProvider + Send + Sync>) {
        self.text_provider = Some(provider);
    }

    pub fn text_provider(&self) -> Option<&Arc<dyn TextProvider + Send + Sync>> {
        self.text_provider.as_ref()
    }

    pub fn dpi_scale(&self) -> f32 {
        if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        }
    }

    /// Measure the width of a text run in logical pixels using the active text provider.
    ///
    /// This is intended for layout/centering code that needs a more accurate width than
    /// simple character-count heuristics. When no provider is set, falls back to
    /// `font_size * 0.55 * text.len()` to match legacy behavior.
    pub fn measure_text_width(&self, text: &str, size_px: f32) -> f32 {
        if let Some(provider) = self.text_provider() {
            if let Some(shaped) = provider.shape_paragraph(text, size_px) {
                let total: f32 = shaped.glyphs.iter().map(|g| g.x_advance).sum();
                // Clamp to non-negative to avoid surprising negatives in rare cases.
                return total.max(0.0);
            }
        }
        // Fallback heuristic consistent with text_measure.rs and legacy elements.
        text.chars().count() as f32 * size_px * 0.55
    }

    /// Measure the width of a styled text run in logical pixels.
    ///
    /// Unlike `measure_text_width`, this accounts for font weight, style, and
    /// family so that measurements match `draw_text_run_styled` rendering.
    pub fn measure_text_width_styled(
        &self,
        text: &str,
        size_px: f32,
        weight: f32,
        style: FontStyle,
        family: Option<&str>,
    ) -> f32 {
        if let Some(provider) = self.text_provider() {
            // Measure at the same *physical* pixel size that draw_text_run_styled
            // uses for rasterization so that variable-font axes (especially
            // optical size / opsz) produce identical advance widths.  Scale the
            // result back to logical pixels afterward.
            let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
                self.dpi_scale
            } else {
                1.0
            };
            let run = TextRun {
                text: text.to_string(),
                pos: [0.0, 0.0],
                size: (size_px * sf).max(1.0),
                logical_size: size_px,
                color: ColorLinPremul::from_srgba_u8([0, 0, 0, 0]),
                weight,
                style,
                family: family.map(String::from),
            };
            provider.measure_run(&run) / sf
        } else {
            text.chars().count() as f32 * size_px * 0.55
        }
    }

    /// Draw pre-rasterized glyph masks at the given origin tinted with the color.
    pub fn draw_text_glyphs(
        &mut self,
        origin: [f32; 2],
        glyphs: &[RasterizedGlyph],
        color: ColorLinPremul,
        z: i32,
    ) {
        for g in glyphs.iter().cloned() {
            self.glyph_draws.push((origin, g, color, z, None));
        }
    }

    /// Draw a hyperlink with text, optional underline, and URL target.
    ///
    /// # Example
    /// ```no_run
    /// # use jag_surface::Canvas;
    /// # use jag_draw::{ColorLinPremul, Hyperlink};
    /// # let mut canvas: Canvas = todo!();
    /// let link = Hyperlink {
    ///     text: "Click me".to_string(),
    ///     pos: [10.0, 20.0],
    ///     size: 16.0,
    ///     color: ColorLinPremul::from_srgba_u8([0, 122, 255, 255]),
    ///     url: "https://example.com".to_string(),
    ///     weight: 400.0,
    ///     measured_width: None,
    ///     underline: true,
    ///     underline_color: None,
    ///     family: None,
    ///     style: jag_draw::FontStyle::Normal,
    /// };
    /// canvas.draw_hyperlink(link, 10);
    /// ```
    pub fn draw_hyperlink(&mut self, hyperlink: jag_draw::Hyperlink, z: i32) {
        // When a text provider is available and we're not inside an opacity
        // group, render the text and underline through canvas methods so they
        // respect the clip_stack (per-glyph clipping for text, rect clipping
        // for the underline).  A stripped-down DrawHyperlink is still emitted
        // into the display list so hit testing continues to work.
        if self.text_provider.is_some() && !self.painter.has_active_opacity() {
            // --- visual: text via per-glyph clipped path ---
            self.draw_text_run_styled(
                hyperlink.pos,
                hyperlink.text.clone(),
                hyperlink.size,
                hyperlink.weight,
                hyperlink.style,
                hyperlink.family.clone(),
                hyperlink.color,
                z,
            );

            // --- visual: underline via clip-aware fill_rect ---
            if hyperlink.underline {
                let underline_color = hyperlink.underline_color.unwrap_or(hyperlink.color);
                let (underline_x, text_width) =
                    if let Some(w) = hyperlink.measured_width.map(|v| v.max(0.0)) {
                        (hyperlink.pos[0], w)
                    } else {
                        let trimmed = hyperlink.text.trim_end();
                        let char_count = trimmed.chars().count() as f32;
                        let weight_boost = ((hyperlink.weight - 400.0).max(0.0) / 500.0) * 0.08;
                        let char_width = hyperlink.size * (0.50 + weight_boost);
                        let mut width = char_count * char_width;
                        let inset = hyperlink.size * 0.10;
                        if width > inset * 2.0 {
                            width -= inset * 2.0;
                        }
                        (hyperlink.pos[0] + inset, width)
                    };

                let underline_thickness = (hyperlink.size * 0.08).max(1.0);
                let underline_offset = hyperlink.size * 0.10;
                self.fill_rect(
                    underline_x,
                    hyperlink.pos[1] + underline_offset,
                    text_width,
                    underline_thickness,
                    Brush::Solid(underline_color),
                    z,
                );
            }

            // --- hit testing only: emit DrawHyperlink with no visual payload ---
            let mut hit_only = hyperlink;
            // Ensure measured_width is set so hit testing doesn't rely on
            // text length (which we're about to clear).
            if hit_only.measured_width.is_none() {
                hit_only.measured_width = Some(self.measure_text_width_styled(
                    &hit_only.text,
                    hit_only.size,
                    hit_only.weight,
                    hit_only.style,
                    hit_only.family.as_deref(),
                ));
            }
            hit_only.text = String::new();
            hit_only.underline = false;
            self.painter.hyperlink(hit_only, z);
        } else {
            // Fallback: no text provider or inside opacity group — use
            // display-list path (no clip, but correct compositing).
            self.painter.hyperlink(hyperlink, z);
        }
    }
}
