use core::ops::Range;

use harfrust::{
    Direction as HbDirection, FontRef as HbFontRef, Script as HbScript, ShaperData, ShaperInstance,
    Tag as HbTag, UnicodeBuffer as HbUnicodeBuffer,
};
use swash::GlyphId;

use crate::font::FontFace;

use super::{Direction, GlyphPosition, Script, ShapedRun};

/// Convert a 4-byte ASCII tag (e.g. `b"wght"`) to a harfrust `Tag`.
pub fn hb_tag_from_bytes(bytes: &[u8; 4]) -> HbTag {
    HbTag::new(bytes)
}

/// Simple text shaper for Phase 1.3 built on harfrust (pure-Rust HarfBuzz port).
///
/// For now this focuses on:
/// - Single-font runs
/// - Simple LTR text
/// - Kerning and ligatures via HarfBuzz semantics
pub struct TextShaper;

impl TextShaper {
    /// Shape a UTF-8 string using the given font and size, assuming simple
    /// left-to-right directionality and Latin script.
    pub fn shape_ltr(
        text: &str,
        text_range: Range<usize>,
        font: &FontFace,
        font_id: u32,
        font_size: f32,
    ) -> ShapedRun {
        Self::shape_ltr_with_variations(text, text_range, font, font_id, font_size, &[])
    }

    /// Shape text with an optional font-weight variation for variable fonts.
    pub fn shape_ltr_weighted(
        text: &str,
        text_range: Range<usize>,
        font: &FontFace,
        font_id: u32,
        font_size: f32,
        weight: Option<f32>,
    ) -> ShapedRun {
        let variations: Vec<(harfrust::Tag, f32)> = weight
            .map(|w| vec![(HbTag::new(b"wght"), w)])
            .unwrap_or_default();
        Self::shape_ltr_with_variations(text, text_range, font, font_id, font_size, &variations)
    }

    /// Shape text with arbitrary font variation settings.
    ///
    /// Each entry is `(tag, value)` — e.g. `(wght, 700.0)`, `(opsz, 17.0)`.
    /// For static fonts, variations are silently ignored by HarfBuzz.
    pub fn shape_ltr_with_variations(
        text: &str,
        text_range: Range<usize>,
        font: &FontFace,
        font_id: u32,
        font_size: f32,
        variations: &[(harfrust::Tag, f32)],
    ) -> ShapedRun {
        // Build a harfrust FontRef from the font bytes.
        let font_data = font.as_bytes();
        let font_ref = HbFontRef::from_index(&font_data, font.index() as u32)
            .expect("valid font data for harfrust");

        // Shaper configuration — apply font variations for variable fonts.
        let data = ShaperData::new(&font_ref);
        let hb_variations: Vec<harfrust::Variation> = variations
            .iter()
            .map(|&(tag, value)| harfrust::Variation { tag, value })
            .collect();
        let instance = ShaperInstance::from_variations(&font_ref, hb_variations.iter().copied());
        let shaper = data
            .shaper(&font_ref)
            .instance(Some(&instance))
            .point_size(None)
            .build();

        // Build Unicode buffer; this will handle complex scripts, combining
        // marks, ligatures, etc., though Phase 1.3 focuses on simple LTR usage.
        let mut buffer = HbUnicodeBuffer::new();
        buffer.push_str(text);
        buffer.set_direction(HbDirection::LeftToRight);
        // Latin script for now; later phases can infer script from text.
        let latin_tag = HbTag::new(b"Latn");
        if let Some(script) = HbScript::from_iso15924_tag(latin_tag) {
            buffer.set_script(script);
        }
        // Let harfrust fill in any remaining segment properties.
        buffer.guess_segment_properties();

        let glyph_buffer = shaper.shape(buffer, &[]);
        let infos = glyph_buffer.glyph_infos();
        let positions = glyph_buffer.glyph_positions();

        let mut glyphs = Vec::with_capacity(infos.len());
        let mut glyph_positions = Vec::with_capacity(infos.len());
        let mut advances = Vec::with_capacity(infos.len());
        let mut clusters = Vec::with_capacity(infos.len());

        // harfrust uses design units; convert to pixels using the font's
        // units-per-em and requested size.
        let metrics = font.metrics();
        let scale = if metrics.units_per_em != 0 {
            font_size / metrics.units_per_em as f32
        } else {
            1.0
        };

        let mut pen_x: f32 = 0.0;
        let mut width: f32 = 0.0;

        for (info, pos) in infos.iter().zip(positions.iter()) {
            let gid = info.glyph_id as GlyphId;
            let x_advance = pos.x_advance as f32 * scale;
            let x_offset = pos.x_offset as f32 * scale;
            let y_offset = -(pos.y_offset as f32) * scale;

            glyphs.push(gid);
            glyph_positions.push(GlyphPosition {
                x_offset: pen_x + x_offset,
                y_offset,
            });
            advances.push(x_advance);
            // Cluster is byte offset within the text
            clusters.push(info.cluster);

            pen_x += x_advance;
            width = pen_x;
        }

        ShapedRun {
            text_range,
            font_id,
            font_size,
            glyphs,
            positions: glyph_positions,
            advances,
            clusters,
            width,
            x_offset: 0.0,
            bidi_level: 0,
            direction: Direction::LeftToRight,
            script: Script::Latin,
        }
    }
}
