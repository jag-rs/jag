use core::ops::Range;
use std::borrow::Cow;

use unicode_bidi::{BidiInfo, Level, LevelRun};

use crate::bidi::BaseDirection;

/// A run of text with a single BiDi embedding level, in visual order.
#[derive(Debug, Clone)]
pub struct BidiRun {
    /// Byte range in the original text.
    pub range: Range<usize>,
    /// Embedding level for this run.
    pub level: u8,
}

/// Reorder a single line of text into visual order according to the
/// Unicode BiDi algorithm (UAX-9).
///
/// `line` is a range of byte indices within `text` corresponding to
/// one line (typically within a single paragraph).
pub fn reorder_line<'a>(
    text: &'a str,
    base_dir: BaseDirection,
    line: Range<usize>,
) -> Cow<'a, str> {
    let info = BidiInfo::new(text, base_dir.to_level());
    let para = find_paragraph(&info, &line);
    info.reorder_line(para, line)
}

/// Compute BiDi level runs for a line in visual order.
///
/// The returned runs are ranges in the original text and appear in
/// the order they should be rendered visually.
pub fn visual_runs(
    text: &str,
    base_dir: BaseDirection,
    line: Range<usize>,
) -> (Vec<Level>, Vec<BidiRun>) {
    let info = BidiInfo::new(text, base_dir.to_level());
    let para = find_paragraph(&info, &line);
    let (levels, runs) = info.visual_runs(para, line.clone());
    let bidi_runs = runs
        .into_iter()
        .map(|run: LevelRun| {
            let level = levels[run.start].number();
            BidiRun { range: run, level }
        })
        .collect();
    (levels, bidi_runs)
}

/// Build a visual-to-logical character index map for a given line.
///
/// The returned vector has length equal to the number of Unicode scalar
/// values (chars) in `text[line.clone()]`. For each visual index `v`,
/// `map[v]` gives the corresponding logical character index within that
/// line (0-based).
pub fn visual_index_map(text: &str, base_dir: BaseDirection, line: Range<usize>) -> Vec<usize> {
    let info = BidiInfo::new(text, base_dir.to_level());
    let para = find_paragraph(&info, &line);

    // Levels per character for the full text, incorporating the line
    // reordering effects (rule L1).
    let levels_per_char: Vec<Level> = info.reordered_levels_per_char(para, line.clone());

    // Collect character indices that lie within the line range.
    let mut line_char_indices = Vec::new();
    for (char_idx, (byte_idx, _ch)) in text.char_indices().enumerate() {
        if byte_idx >= line.start && byte_idx < line.end {
            line_char_indices.push(char_idx);
        }
    }

    // Extract levels for just the characters in this line in logical order.
    let mut line_levels: Vec<Level> = Vec::with_capacity(line_char_indices.len());
    for char_idx in &line_char_indices {
        line_levels.push(levels_per_char[*char_idx]);
    }

    // Compute visual order mapping for these characters.
    BidiInfo::reorder_visual(&line_levels)
}

fn find_paragraph<'text>(
    info: &'text BidiInfo<'text>,
    line: &Range<usize>,
) -> &'text unicode_bidi::ParagraphInfo {
    info.paragraphs
        .iter()
        .find(|p| line.start >= p.range.start && line.end <= p.range.end)
        .expect("line range must lie within a single paragraph")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_mixed_text_line() {
        // Hebrew + Latin example similar to unicode-bidi docs.
        let text = concat!["א", "ב", "ג", "a", "b", "c"];
        let line = 0..text.len();
        let visual = reorder_line(text, BaseDirection::Auto, line);
        // Expect Latin rendered first in visual order for this sequence.
        assert!(visual.starts_with("abc"));
    }

    #[test]
    fn visual_runs_have_levels() {
        let text = concat!["א", "ב", "g", "h"];
        let line = 0..text.len();
        let (_levels, runs) = visual_runs(text, BaseDirection::Auto, line);
        assert!(!runs.is_empty());
    }

    #[test]
    fn visual_index_map_matches_expected_for_mixed_line() {
        // Mixed LTR + RTL in a single line with LTR base direction.
        let text = "abc אבג";
        // Single-line case.
        let line = 0..text.len();

        let char_count = text.chars().count();
        let map = visual_index_map(text, BaseDirection::Ltr, line);
        assert_eq!(map.len(), char_count);

        // Map must be a permutation of 0..char_count.
        let mut sorted = map.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..char_count).collect::<Vec<_>>());

        // For this specific string, we expect "abc " first, then the RTL run.
        // Logical indices: 0:'a',1:'b',2:'c',3:' ',4:'א',5:'ב',6:'ג'
        // Visual order:    0,1,2,3,6,5,4
        assert_eq!(map[0..4], [0, 1, 2, 3]);
        assert_eq!(map[4..], [6, 5, 4]);
    }
}
