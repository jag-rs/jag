use crate::layout::LineBox;

/// Prefix sum array for O(1) line/character lookups.
#[derive(Debug, Clone)]
pub struct PrefixSums {
    /// Cumulative character counts per line (at line start).
    char_offsets: Vec<usize>,
    /// Cumulative byte offsets per line (at line start).
    byte_offsets: Vec<usize>,
}

impl PrefixSums {
    /// Build prefix sums from the full text and its line boxes.
    pub fn new(text: &str, lines: &[LineBox]) -> Self {
        let mut char_offsets = Vec::with_capacity(lines.len());
        let mut byte_offsets = Vec::with_capacity(lines.len());
        let mut char_cursor = 0usize;

        for line in lines {
            char_offsets.push(char_cursor);
            byte_offsets.push(line.text_range.start);

            let slice = &text[line.text_range.clone()];
            char_cursor += slice.chars().count();
        }

        Self {
            char_offsets,
            byte_offsets,
        }
    }

    /// Find line index containing character offset (layout-wide).
    pub fn line_at_char(&self, char_offset: usize) -> Option<usize> {
        if self.char_offsets.is_empty() {
            return None;
        }
        match self.char_offsets.binary_search(&char_offset) {
            Ok(i) => Some(i),
            Err(i) => {
                if i > 0 {
                    Some(i - 1)
                } else {
                    None
                }
            }
        }
    }

    /// Get character offset at start of line.
    pub fn char_offset_at_line(&self, line_index: usize) -> Option<usize> {
        self.char_offsets.get(line_index).copied()
    }

    /// Get byte offset at start of line.
    pub fn byte_offset_at_line(&self, line_index: usize) -> Option<usize> {
        self.byte_offsets.get(line_index).copied()
    }

    /// Total number of characters covered by all lines.
    pub fn total_chars(&self, text: &str, lines: &[LineBox]) -> usize {
        if let Some(last) = lines.last() {
            let last_offset = self
                .char_offset_at_line(lines.len().saturating_sub(1))
                .unwrap_or(0);
            let tail = &text[last.text_range.clone()];
            last_offset + tail.chars().count()
        } else {
            0
        }
    }
}
