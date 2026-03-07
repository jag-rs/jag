use core::ops::Range;

use unicode_linebreak::{BreakOpportunity, linebreaks};
use unicode_segmentation::UnicodeSegmentation;

/// Kind of line break at a given position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineBreakKind {
    /// Required line break (e.g., explicit newline).
    Mandatory,
    /// Optional line break opportunity.
    Opportunity,
}

/// A line break opportunity in the text.
#[derive(Debug, Clone, Copy)]
pub struct LineBreak {
    /// Byte offset *after* the break.
    pub offset: usize,
    /// Break kind (mandatory vs optional).
    pub kind: LineBreakKind,
}

/// Kind of word boundary at a given range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordBoundaryKind {
    /// A run of word characters.
    Word,
    /// Non-word run (whitespace, punctuation, etc.).
    NonWord,
}

/// A word or non-word segment in the text.
#[derive(Debug, Clone)]
pub struct WordBoundary {
    pub range: Range<usize>,
    pub kind: WordBoundaryKind,
}

/// Compute all line break opportunities in the given text using UAX-14
/// via the `unicode-linebreak` crate.
///
/// This includes both optional and mandatory breaks, and treats the
/// end-of-text as a mandatory break.
pub fn compute_line_breaks(text: &str) -> Vec<LineBreak> {
    linebreaks(text)
        .map(|(offset, opp)| LineBreak {
            offset,
            kind: match opp {
                BreakOpportunity::Mandatory => LineBreakKind::Mandatory,
                BreakOpportunity::Allowed => LineBreakKind::Opportunity,
            },
        })
        .collect()
}

/// Enumerate word and non-word segments for the given text.
///
/// This uses `unicode-segmentation`'s word boundary logic under the
/// hood, which follows Unicode Text Segmentation (roughly UAX-29).
pub fn compute_word_boundaries(text: &str) -> Vec<WordBoundary> {
    let mut result = Vec::new();
    let mut byte_offset = 0;

    for segment in text.split_word_bounds() {
        let start = byte_offset;
        let end = start + segment.len();

        let kind = if segment.chars().any(|c| c.is_alphanumeric()) {
            WordBoundaryKind::Word
        } else {
            WordBoundaryKind::NonWord
        };

        result.push(WordBoundary {
            range: start..end,
            kind,
        });

        byte_offset = end;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_breaks_basic_with_newline() {
        let text = "a b \nc";
        let breaks = compute_line_breaks(text);
        // There should be at least one mandatory break at or after the newline.
        assert!(breaks.iter().any(|b| b.kind == LineBreakKind::Mandatory));
    }

    #[test]
    fn word_boundaries_simple() {
        let text = "Hello, world!";
        let words: Vec<_> = compute_word_boundaries(text)
            .into_iter()
            .filter(|w| w.kind == WordBoundaryKind::Word)
            .collect();
        assert_eq!(words.len(), 2);
        assert_eq!(&text[words[0].range.clone()], "Hello");
        assert_eq!(&text[words[1].range.clone()], "world");
    }
}
