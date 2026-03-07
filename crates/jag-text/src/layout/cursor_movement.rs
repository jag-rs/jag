use unicode_segmentation::UnicodeSegmentation;

use super::line_breaker::{WordBoundaryKind, compute_word_boundaries};

/// Direction for cursor movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Unit of cursor movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementUnit {
    /// Move by one grapheme cluster.
    Character,
    /// Move by one word boundary.
    Word,
    /// Move to line start/end.
    Line,
    /// Move to document start/end.
    Document,
}

/// Helper functions for cursor movement operations.
pub struct CursorMovement;

impl CursorMovement {
    /// Move cursor left by one grapheme cluster.
    pub fn move_left_char(text: &str, byte_offset: usize) -> usize {
        if byte_offset == 0 {
            return 0;
        }

        // Find the previous grapheme boundary
        let mut prev_boundary = 0;
        for (idx, _) in text.grapheme_indices(true) {
            if idx >= byte_offset {
                return prev_boundary;
            }
            prev_boundary = idx;
        }

        prev_boundary
    }

    /// Move cursor right by one grapheme cluster.
    pub fn move_right_char(text: &str, byte_offset: usize) -> usize {
        if byte_offset >= text.len() {
            return text.len();
        }

        // Find the next grapheme boundary
        for (idx, _) in text.grapheme_indices(true) {
            if idx > byte_offset {
                return idx;
            }
        }

        text.len()
    }

    /// Move cursor left by one word boundary.
    ///
    /// This moves to the start of the current word if in the middle of a word,
    /// or to the start of the previous word if at a word boundary.
    pub fn move_left_word(text: &str, byte_offset: usize) -> usize {
        if byte_offset == 0 {
            return 0;
        }

        let boundaries = compute_word_boundaries(text);

        // Collect all word starts before the current position
        let mut word_starts: Vec<usize> = boundaries
            .iter()
            .filter(|b| b.kind == WordBoundaryKind::Word && b.range.start < byte_offset)
            .map(|b| b.range.start)
            .collect();

        // Return the last word start before current position
        word_starts.pop().unwrap_or(0)
    }

    /// Move cursor right by one word boundary.
    ///
    /// This moves to the end of the current word if in the middle of a word,
    /// or to the end of the next word if at a word boundary.
    pub fn move_right_word(text: &str, byte_offset: usize) -> usize {
        if byte_offset >= text.len() {
            return text.len();
        }

        let boundaries = compute_word_boundaries(text);

        // Find the next word boundary after current position
        let mut found_current = false;

        for boundary in boundaries.iter() {
            // If we're before or at the start of this word
            if byte_offset <= boundary.range.start {
                if boundary.kind == WordBoundaryKind::Word {
                    return boundary.range.end;
                }
            }

            // If we're inside this word, move to its end
            if boundary.range.contains(&byte_offset) {
                if boundary.kind == WordBoundaryKind::Word {
                    return boundary.range.end;
                }
                found_current = true;
            }

            // If we've passed the current position, find the next word
            if found_current && boundary.kind == WordBoundaryKind::Word {
                return boundary.range.end;
            }
        }

        text.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_move_left_char() {
        let text = "Hello 世界";

        // From middle of ASCII
        assert_eq!(CursorMovement::move_left_char(text, 5), 4);

        // From start
        assert_eq!(CursorMovement::move_left_char(text, 0), 0);

        // From after space
        assert_eq!(CursorMovement::move_left_char(text, 6), 5);

        // From after multi-byte character (世 is 3 bytes)
        assert_eq!(CursorMovement::move_left_char(text, 9), 6);
    }

    #[test]
    fn test_move_right_char() {
        let text = "Hello 世界";

        // From start
        assert_eq!(CursorMovement::move_right_char(text, 0), 1);

        // From middle of ASCII
        assert_eq!(CursorMovement::move_right_char(text, 4), 5);

        // From before multi-byte character
        assert_eq!(CursorMovement::move_right_char(text, 6), 9);

        // From end
        assert_eq!(
            CursorMovement::move_right_char(text, text.len()),
            text.len()
        );
    }

    #[test]
    fn test_move_left_word() {
        let text = "Hello, world! Test";

        // From middle of "world"
        assert_eq!(CursorMovement::move_left_word(text, 10), 7);

        // From start of "world"
        assert_eq!(CursorMovement::move_left_word(text, 7), 0);

        // From start
        assert_eq!(CursorMovement::move_left_word(text, 0), 0);

        // From middle of "Test"
        assert_eq!(CursorMovement::move_left_word(text, 16), 14);
    }

    #[test]
    fn test_move_right_word() {
        let text = "Hello, world! Test";

        // From start
        assert_eq!(CursorMovement::move_right_word(text, 0), 5);

        // From middle of "Hello"
        assert_eq!(CursorMovement::move_right_word(text, 2), 5);

        // From end of "Hello"
        assert_eq!(CursorMovement::move_right_word(text, 5), 12);

        // From end
        assert_eq!(
            CursorMovement::move_right_word(text, text.len()),
            text.len()
        );
    }

    #[test]
    fn test_emoji_movement() {
        let text = "Hello 👨‍👩‍👧‍👦 World";

        // The emoji is a ZWJ sequence that should be treated as one grapheme
        let emoji_start = 6;
        let emoji_end = text.find(" World").unwrap();

        // Moving right from before emoji should skip the entire emoji
        let next = CursorMovement::move_right_char(text, emoji_start);
        assert_eq!(next, emoji_end);

        // Moving left from after emoji should go to before emoji
        let prev = CursorMovement::move_left_char(text, emoji_end);
        assert_eq!(prev, emoji_start);
    }
}
