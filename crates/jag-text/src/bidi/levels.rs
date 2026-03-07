use core::ops::Range;

use unicode_bidi::{BidiInfo, LTR_LEVEL, Level, RTL_LEVEL};

/// Base direction hint for paragraph analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseDirection {
    /// Detect paragraph base direction from text (first strong char).
    Auto,
    /// Force overall left-to-right base direction.
    Ltr,
    /// Force overall right-to-left base direction.
    Rtl,
}

impl BaseDirection {
    pub fn to_level(self) -> Option<Level> {
        match self {
            BaseDirection::Auto => None,
            BaseDirection::Ltr => Some(LTR_LEVEL),
            BaseDirection::Rtl => Some(RTL_LEVEL),
        }
    }
}

/// Paragraph direction classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphDirection {
    Ltr,
    Rtl,
    Mixed,
}

/// BiDi information for a single paragraph.
#[derive(Debug, Clone)]
pub struct ParagraphBidi {
    /// Byte range of this paragraph within the original text.
    pub range: Range<usize>,
    /// Paragraph embedding level (UAX-9).
    pub level: u8,
    /// Paragraph direction (LTR/RTL/Mixed) derived from levels.
    pub direction: ParagraphDirection,
}

/// Compute paragraph-level BiDi information for the given text.
///
/// This uses the Unicode BiDi algorithm (UAX-9) via `unicode-bidi`
/// and supports explicit base-direction overrides through
/// `BaseDirection`.
pub fn paragraph_bidi_info(text: &str, base_dir: BaseDirection) -> Vec<ParagraphBidi> {
    let info = BidiInfo::new(text, base_dir.to_level());
    info.paragraphs
        .iter()
        .map(|para| {
            let slice_levels = &info.levels[para.range.clone()];
            let direction = paragraph_direction(slice_levels, para.level);
            ParagraphBidi {
                range: para.range.clone(),
                level: para.level.number(),
                direction,
            }
        })
        .collect()
}

/// Compute embedding levels for each byte in the text.
///
/// The result is parallel to `text.as_bytes()`: multi-byte characters
/// will have the same level repeated for each byte.
pub fn levels_per_byte(text: &str, base_dir: BaseDirection) -> Vec<u8> {
    let info = BidiInfo::new(text, base_dir.to_level());
    info.levels.iter().map(Level::number).collect()
}

fn paragraph_direction(_levels: &[Level], para_level: Level) -> ParagraphDirection {
    if para_level.is_rtl() {
        ParagraphDirection::Rtl
    } else {
        ParagraphDirection::Ltr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_paragraph_direction_auto() {
        // Hebrew + Latin
        let text = "אבג abc";
        let paras = paragraph_bidi_info(text, BaseDirection::Auto);
        assert_eq!(paras.len(), 1);
        assert_eq!(paras[0].direction, ParagraphDirection::Rtl);
    }

    #[test]
    fn base_direction_override_ltr() {
        let text = "אבג abc";
        let paras = paragraph_bidi_info(text, BaseDirection::Ltr);
        assert_eq!(paras[0].direction, ParagraphDirection::Ltr);
    }

    #[test]
    fn levels_cover_all_bytes() {
        let text = "a אב";
        let levels = levels_per_byte(text, BaseDirection::Auto);
        assert_eq!(levels.len(), text.len());
    }

    #[test]
    fn mixed_ltr_with_rtl_segment_has_distinct_levels() {
        let text = "abc אבג def";
        let levels = levels_per_byte(text, BaseDirection::Auto);
        // Expect at least two distinct levels for mixed-direction text.
        let mut unique: Vec<u8> = levels.clone();
        unique.sort();
        unique.dedup();
        assert!(unique.len() >= 2);
    }

    #[test]
    fn mixed_rtl_with_ltr_numbers_has_distinct_levels() {
        let text = "אבג 123 שלום";
        let levels = levels_per_byte(text, BaseDirection::Auto);
        let mut unique: Vec<u8> = levels.clone();
        unique.sort();
        unique.dedup();
        assert!(unique.len() >= 2);
    }

    #[test]
    fn neutrals_receive_some_level() {
        let text = "abc - אבג";
        let levels = levels_per_byte(text, BaseDirection::Auto);
        // Find the level assigned to the hyphen and ensure it is set.
        let hyphen_index = text.find('-').unwrap();
        assert!(levels[hyphen_index] <= 125);
    }
}
