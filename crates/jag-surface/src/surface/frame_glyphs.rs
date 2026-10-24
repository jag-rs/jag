use super::JagSurface;
use std::sync::Arc;
type GlyphDraw = (
    [f32; 2],
    jag_draw::RasterizedGlyph,
    jag_draw::ColorLinPremul,
    i32,
    Option<jag_draw::Rect>,
);
impl JagSurface {
    pub(super) fn prepare_frame_glyphs(
        &self,
        mut glyph_draws: Vec<GlyphDraw>,
        text_provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
        text_draws: &[jag_draw::ExtractedTextDraw],
    ) -> Vec<GlyphDraw> {
        // Merge glyphs supplied explicitly via Canvas (draw_text_run/draw_text_direct)
        // with text runs extracted from the display list (e.g., hyperlinks) for
        // unified text rendering.
        if let Some(provider) = text_provider {
            // Use the same snapping strategy as direct text paths so small
            // text (e.g., 13–15px) lands cleanly on device pixels.
            let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
                self.dpi_scale
            } else {
                1.0
            };
            let snap = |v: f32| -> f32 { (v * sf).round() / sf };

            for text_draw in text_draws {
                let run = &text_draw.run;
                let [a, b, c, d, e, f] = text_draw.transform.m;

                // Transform the run origin (baseline-left) into world coordinates.
                let origin_x = a * run.pos[0] + c * run.pos[1] + e;
                let origin_y = b * run.pos[0] + d * run.pos[1] + f;

                // Infer uniform scale from the linear part of the transform so
                // text respects any explicit scaling in the display list.
                let sx = (a * a + b * b).sqrt();
                let sy = (c * c + d * d).sqrt();
                let mut s = if sx.is_finite() && sy.is_finite() {
                    if sx > 0.0 && sy > 0.0 {
                        (sx + sy) * 0.5
                    } else {
                        sx.max(sy).max(1.0)
                    }
                } else {
                    1.0
                };
                if !s.is_finite() || s <= 0.0 {
                    s = 1.0;
                }

                // Rasterize at *physical* pixel size (scaled by DPI) to match
                // the direct canvas text path. PassManager assumes glyph bitmaps
                // are at physical resolution and divides quad sizes by DPI.
                let logical_size = (run.size * s).max(1.0);
                let physical_size = (logical_size * sf).max(1.0);
                let run_for_provider = jag_draw::TextRun {
                    text: run.text.clone(),
                    pos: [0.0, 0.0],
                    size: physical_size,
                    logical_size,
                    color: run.color,
                    weight: run.weight,
                    style: run.style,
                    family: run.family.clone(),
                };

                // Rasterize glyphs for this run and push into glyph_draws.
                // Provider offsets are in *physical* pixels (proportional to physical_size).
                // Convert back into logical coordinates so PassManager's DPI scaling
                // keeps geometry and text aligned.
                let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &run_for_provider);
                for g in glyphs.iter() {
                    let mut origin = [origin_x + g.offset[0] / sf, origin_y + g.offset[1] / sf];
                    if logical_size <= 15.0 {
                        origin[0] = snap(origin[0]);
                        origin[1] = snap(origin[1]);
                    }
                    let (glyph, color) = text_draw.glyph_for_draw(g, origin, sf);
                    glyph_draws.push((origin, glyph, color, text_draw.z, text_draw.clip));
                }
            }
        }

        glyph_draws
    }
}
