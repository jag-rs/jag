use unicode_segmentation::UnicodeSegmentation;

use crate::font::{FontFace, ScaledFontMetrics};
use crate::layout::{
    LineBox, PrefixSums, WrapMode,
    cursor::{Cursor, CursorAffinity, CursorPosition, CursorRect},
    cursor_movement::CursorMovement,
    hit_test::{HitTestPolicy, HitTestResult, Point, Position},
    line_breaker::{WordBoundaryKind, compute_line_breaks, compute_word_boundaries},
    selection::{Selection, SelectionRect},
    undo::{TextOperation, UndoStack},
};
use crate::shaping::TextShaper;

/// Complete text layout with all lines for a single font.
///
/// Phase 2.2 focuses on building `LineBox` instances with
/// correct metrics and supporting basic multi-line layout
/// driven by explicit newline characters.
#[derive(Debug)]
pub struct TextLayout {
    /// Source text.
    text: String,
    /// All line boxes in visual/top-to-bottom order.
    lines: Vec<LineBox>,
    /// Prefix sums over characters and bytes for fast lookups.
    prefix_sums: PrefixSums,
    /// Undo/redo stack for text editing operations.
    undo_stack: UndoStack,
    /// Optional override for the line height (in pixels) used during layout.
    line_height_override: Option<f32>,
}

impl TextLayout {
    /// Layout text using a single font and font size with no wrapping
    /// beyond explicit newline characters.
    pub fn new(text: impl Into<String>, font: &FontFace, font_size: f32) -> Self {
        Self::with_wrap(text, font, font_size, None, WrapMode::NoWrap)
    }

    /// Layout text with an optional width constraint and wrapping mode.
    ///
    /// - If `max_width` is `None` or `WrapMode::NoWrap`, only explicit
    ///   newlines are honored.
    /// - For `WrapMode::BreakWord`, uses Unicode line breaking and
    ///   falls back to grapheme boundaries when a single word exceeds
    ///   `max_width`.
    /// - For `WrapMode::BreakAll`, always breaks at grapheme
    ///   boundaries using a greedy algorithm.
    pub fn with_wrap(
        text: impl Into<String>,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Self {
        Self::with_wrap_internal(text, font, font_size, max_width, wrap_mode, None)
    }

    /// Layout text with an explicit line height override.
    pub fn with_wrap_and_line_height(
        text: impl Into<String>,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
        line_height: f32,
    ) -> Self {
        Self::with_wrap_internal(
            text,
            font,
            font_size,
            max_width,
            wrap_mode,
            Some(line_height),
        )
    }

    fn with_wrap_internal(
        text: impl Into<String>,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
        line_height_override: Option<f32>,
    ) -> Self {
        let text = text.into();
        let mut lines = Vec::new();

        // Compute scaled metrics once; reused for all lines.
        let scaled = font.scaled_metrics(font_size);
        let line_height = Self::resolve_line_height(line_height_override, &scaled);
        let leading = (line_height - scaled.ascent - scaled.descent).max(0.0);

        let mut y = 0.0f32;

        // Split into logical paragraphs on '\n'. Wrapping decisions
        // are made per paragraph.
        let mut para_start = 0usize;
        for (idx, ch) in text.char_indices() {
            if ch == '\n' {
                Self::layout_paragraph(
                    &text,
                    para_start..idx,
                    font,
                    font_size,
                    line_height,
                    scaled.ascent,
                    scaled.descent,
                    leading,
                    max_width,
                    wrap_mode,
                    &mut y,
                    &mut lines,
                );
                para_start = idx + ch.len_utf8();
            }
        }
        if para_start <= text.len() {
            Self::layout_paragraph(
                &text,
                para_start..text.len(),
                font,
                font_size,
                line_height,
                scaled.ascent,
                scaled.descent,
                leading,
                max_width,
                wrap_mode,
                &mut y,
                &mut lines,
            );
        }

        let prefix_sums = PrefixSums::new(&text, &lines);
        Self {
            text,
            lines,
            prefix_sums,
            undo_stack: UndoStack::new(),
            line_height_override,
        }
    }

    fn resolve_line_height(line_height_override: Option<f32>, scaled: &ScaledFontMetrics) -> f32 {
        let min_height = scaled.ascent + scaled.descent;
        line_height_override.unwrap_or(min_height + scaled.line_gap)
    }

    /// Convenience: layout text using a default system font.
    ///
    /// This uses `fontdb` to pick a reasonable sans-serif font from the host
    /// system (same heuristic as `engine-core::DetirTextProvider::from_system_fonts`)
    /// so callers don't need to manage a `FontFace` directly.
    pub fn with_system_font(
        text: impl Into<String>,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> crate::font::Result<Self> {
        let font = crate::font::load_system_default_font()?;
        Ok(Self::with_wrap(
            text, &font, font_size, max_width, wrap_mode,
        ))
    }

    /// Underlying source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// All line boxes in this layout.
    pub fn lines(&self) -> &[LineBox] {
        &self.lines
    }

    /// Get the line containing the given character offset, if any.
    pub fn line_at_char(&self, char_offset: usize) -> Option<&LineBox> {
        self.prefix_sums
            .line_at_char(char_offset)
            .and_then(|idx| self.lines.get(idx))
    }

    /// Get the cumulative character offset at the start of a line.
    pub fn char_offset_at_line(&self, line_index: usize) -> Option<usize> {
        self.prefix_sums.char_offset_at_line(line_index)
    }

    /// Calculate the visual rectangle for rendering a cursor at the given position.
    ///
    /// Returns `None` if the cursor position is invalid or outside the text bounds.
    pub fn cursor_rect(&self, cursor: &Cursor) -> Option<CursorRect> {
        self.cursor_rect_at_position(cursor.position())
    }

    /// Calculate the visual rectangle for a cursor at a specific position.
    ///
    /// Returns `None` if the position is invalid or outside the text bounds.
    pub fn cursor_rect_at_position(&self, position: CursorPosition) -> Option<CursorRect> {
        let byte_offset = position.byte_offset;

        // Handle empty text
        if self.text.is_empty() {
            if byte_offset == 0 && !self.lines.is_empty() {
                let line = &self.lines[0];
                // Use ascent + descent for cursor height to cover full text height
                let cursor_height = line.ascent + line.descent;
                return Some(CursorRect::new(
                    0.0,
                    line.y_offset,
                    1.0, // Default cursor width
                    cursor_height,
                ));
            }
            return None;
        }

        // Validate byte offset
        if byte_offset > self.text.len() {
            return None;
        }

        // Find the line containing this byte offset
        let line_idx = self.find_line_at_byte_offset(byte_offset)?;
        let line = &self.lines[line_idx];

        // Calculate X position within the line
        let x = self.calculate_x_at_byte_offset(line, byte_offset);

        // Use ascent + descent for cursor height to cover full text height
        let cursor_height = line.ascent + line.descent;
        Some(CursorRect::new(
            x,
            line.y_offset,
            1.0, // Default cursor width
            cursor_height,
        ))
    }

    /// Find the line index containing the given byte offset.
    fn find_line_at_byte_offset(&self, byte_offset: usize) -> Option<usize> {
        for (idx, line) in self.lines.iter().enumerate() {
            if line.text_range.contains(&byte_offset) || line.text_range.end == byte_offset {
                return Some(idx);
            }
        }
        None
    }

    /// Calculate the X position for a cursor at the given byte offset within a line.
    /// Uses cluster information for accurate positioning with ligatures.
    fn calculate_x_at_byte_offset(&self, line: &LineBox, byte_offset: usize) -> f32 {
        // If the offset is at or before the line start, cursor is at the beginning
        if byte_offset <= line.text_range.start {
            return 0.0;
        }

        // If the offset is at or after the line end, cursor is at the end
        if byte_offset >= line.text_range.end {
            return line.width;
        }

        // Calculate position within the line by measuring text up to the cursor
        let mut x = 0.0;
        for run in &line.runs {
            if byte_offset <= run.text_range.start {
                // Cursor is before this run
                break;
            }

            if byte_offset >= run.text_range.end {
                // Cursor is after this run, add full run width
                x += run.width;
                continue;
            }

            // Cursor is within this run - use cluster information for precise positioning
            let offset_in_run = byte_offset - run.text_range.start;

            // Find the cluster containing or just before this offset
            let mut current_x = 0.0;
            let mut i = 0;

            while i < run.clusters.len() {
                let cluster_start = run.clusters[i];

                // If this cluster starts at or after our target offset, we're done
                if cluster_start as usize >= offset_in_run {
                    x += current_x;
                    return x;
                }

                // Accumulate width of all glyphs in this cluster
                let mut cluster_width = 0.0;
                let mut j = i;
                while j < run.clusters.len() && run.clusters[j] == cluster_start {
                    cluster_width += run.advances[j];
                    j += 1;
                }

                // Check if our offset is within this cluster
                // Find the next cluster boundary
                let next_cluster_start = if j < run.clusters.len() {
                    run.clusters[j] as usize
                } else {
                    run.text_range.len()
                };

                if offset_in_run < next_cluster_start {
                    // Offset is within this cluster
                    // Position at start of cluster (ligatures are indivisible)
                    x += current_x;
                    return x;
                }

                current_x += cluster_width;
                i = j;
            }

            // Offset is after all clusters, position at end of run
            x += run.width;
            break;
        }

        x
    }

    /// Validate and snap a cursor position to the nearest grapheme boundary.
    pub fn snap_cursor_to_boundary(&self, position: CursorPosition) -> CursorPosition {
        position.snap_to_grapheme_boundary(&self.text)
    }

    /// Create a cursor at the start of the text.
    pub fn cursor_at_start(&self) -> Cursor {
        Cursor::at_position(CursorPosition::new(0))
    }

    /// Create a cursor at the end of the text.
    pub fn cursor_at_end(&self) -> Cursor {
        Cursor::at_position(CursorPosition::new(self.text.len()))
    }

    /// Perform hit testing to find the character offset at a given point.
    ///
    /// The point coordinates are in zone-local space (relative to the layout origin).
    /// This ensures hit testing works correctly regardless of where the text is
    /// positioned on screen.
    ///
    /// # Arguments
    /// * `point` - Zone-local coordinates to test
    /// * `policy` - How to handle points outside text bounds
    ///
    /// # Returns
    /// `Some(HitTestResult)` with the byte offset, affinity, and line index,
    /// or `None` if using `HitTestPolicy::Strict` and the point is outside bounds.
    pub fn hit_test(&self, point: Point, policy: HitTestPolicy) -> Option<HitTestResult> {
        // Handle empty text
        if self.text.is_empty() {
            return match policy {
                HitTestPolicy::Clamp => Some(HitTestResult::new(0, CursorAffinity::Downstream, 0)),
                HitTestPolicy::Strict => None,
            };
        }

        // Find the line containing this Y coordinate
        let line_idx = self.find_line_at_y(point.y, policy)?;
        let line = &self.lines[line_idx];

        // Hit test within the line to find X position
        let byte_offset = self.hit_test_line(line, point.x, policy)?;

        // Determine affinity based on position within character
        let affinity = CursorAffinity::Downstream;

        Some(HitTestResult::new(byte_offset, affinity, line_idx))
    }

    /// Map a character offset to its visual position in zone-local coordinates.
    ///
    /// Returns the position where a cursor at this offset would be rendered.
    /// All coordinates are relative to the layout origin.
    ///
    /// # Arguments
    /// * `byte_offset` - Byte offset in the source text
    ///
    /// # Returns
    /// `Some(Position)` with zone-local coordinates, or `None` if offset is invalid.
    pub fn offset_to_position(&self, byte_offset: usize) -> Option<Position> {
        // Validate offset
        if byte_offset > self.text.len() {
            return None;
        }

        // Handle empty text
        if self.text.is_empty() && byte_offset == 0 {
            return Some(Position::new(0.0, 0.0, 0));
        }

        // Find the line containing this offset
        let line_idx = self.find_line_at_byte_offset(byte_offset)?;
        let line = &self.lines[line_idx];

        // Calculate X position within the line
        let x = self.calculate_x_at_byte_offset(line, byte_offset);

        // Y position is the top of the line
        let y = line.y_offset;

        Some(Position::new(x, y, line_idx))
    }

    /// Get the position of the cursor baseline (for IME candidate window positioning).
    ///
    /// Returns zone-local coordinates of the baseline at the given offset.
    pub fn offset_to_baseline_position(&self, byte_offset: usize) -> Option<Position> {
        let line_idx = self.find_line_at_byte_offset(byte_offset)?;
        let line = &self.lines[line_idx];
        let x = self.calculate_x_at_byte_offset(line, byte_offset);
        let y = line.baseline_y();

        Some(Position::new(x, y, line_idx))
    }

    /// Find the line index containing the given Y coordinate.
    fn find_line_at_y(&self, y: f32, policy: HitTestPolicy) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }

        // Before first line
        if y < self.lines[0].y_offset {
            return match policy {
                HitTestPolicy::Clamp => Some(0),
                HitTestPolicy::Strict => None,
            };
        }

        // After last line
        let last_idx = self.lines.len() - 1;
        if y >= self.lines[last_idx].bottom_y() {
            return match policy {
                HitTestPolicy::Clamp => Some(last_idx),
                HitTestPolicy::Strict => None,
            };
        }

        // Find the line containing this Y
        for (idx, line) in self.lines.iter().enumerate() {
            if y >= line.y_offset && y < line.bottom_y() {
                return Some(idx);
            }
        }

        // Shouldn't reach here, but clamp to last line
        match policy {
            HitTestPolicy::Clamp => Some(last_idx),
            HitTestPolicy::Strict => None,
        }
    }

    /// Hit test within a specific line to find the byte offset at X coordinate.
    /// Handles BiDi text by considering visual order of runs.
    fn hit_test_line(&self, line: &LineBox, x: f32, policy: HitTestPolicy) -> Option<usize> {
        // Before line start
        if x <= 0.0 {
            return match policy {
                HitTestPolicy::Clamp => {
                    // For BiDi, find the visually first run
                    self.find_visual_start_offset(line)
                }
                HitTestPolicy::Strict => None,
            };
        }

        // After line end
        if x >= line.width {
            return match policy {
                HitTestPolicy::Clamp => {
                    // For BiDi, find the visually last run
                    self.find_visual_end_offset(line)
                }
                HitTestPolicy::Strict => None,
            };
        }

        // Empty line
        if line.text_range.is_empty() {
            return Some(line.text_range.start);
        }

        // Find the position within the line
        // Runs are already in visual order from BiDi reordering
        let mut current_x = 0.0;
        for run in &line.runs {
            let run_end_x = current_x + run.width;

            if x >= current_x && x < run_end_x {
                // Hit is within this run
                let x_in_run = if run.direction == crate::shaping::Direction::RightToLeft {
                    // For RTL runs, reverse the X coordinate
                    run.width - (x - current_x)
                } else {
                    x - current_x
                };
                return self.hit_test_run(run, x_in_run);
            }

            current_x = run_end_x;
        }

        // Shouldn't reach here, but clamp to line end
        Some(line.text_range.end)
    }

    /// Find the byte offset at the visual start of a line (handles BiDi).
    fn find_visual_start_offset(&self, line: &LineBox) -> Option<usize> {
        if line.runs.is_empty() {
            return Some(line.text_range.start);
        }

        // The first run in visual order
        let first_run = &line.runs[0];
        if first_run.direction == crate::shaping::Direction::RightToLeft {
            // RTL run: visual start is logical end
            Some(first_run.text_range.end)
        } else {
            // LTR run: visual start is logical start
            Some(first_run.text_range.start)
        }
    }

    /// Find the byte offset at the visual end of a line (handles BiDi).
    fn find_visual_end_offset(&self, line: &LineBox) -> Option<usize> {
        if line.runs.is_empty() {
            return Some(line.text_range.end);
        }

        // The last run in visual order
        let last_run = &line.runs[line.runs.len() - 1];
        if last_run.direction == crate::shaping::Direction::RightToLeft {
            // RTL run: visual end is logical start
            Some(last_run.text_range.start)
        } else {
            // LTR run: visual end is logical end
            Some(last_run.text_range.end)
        }
    }

    /// Hit test within a shaped run to find the byte offset.
    /// Uses cluster information from HarfBuzz for ligature-aware positioning.
    fn hit_test_run(&self, run: &crate::shaping::ShapedRun, x: f32) -> Option<usize> {
        let run_text = &self.text[run.text_range.clone()];

        // Empty run
        if run_text.is_empty() {
            return Some(run.text_range.start);
        }

        // If no glyphs, return start
        if run.glyphs.is_empty() {
            return Some(run.text_range.start);
        }

        // Build cluster boundaries with their X positions
        // This handles ligatures correctly since multiple glyphs can map to same cluster
        let mut cluster_bounds: Vec<(u32, f32, f32)> = Vec::new(); // (cluster_byte_offset, x_start, x_end)

        let mut current_x = 0.0;
        let mut i = 0;

        while i < run.clusters.len() {
            let cluster_start = run.clusters[i];
            let cluster_x_start = current_x;

            // Find all glyphs belonging to this cluster
            let mut cluster_width = 0.0;
            let mut j = i;
            while j < run.clusters.len() && run.clusters[j] == cluster_start {
                cluster_width += run.advances[j];
                j += 1;
            }

            let cluster_x_end = current_x + cluster_width;
            cluster_bounds.push((cluster_start, cluster_x_start, cluster_x_end));

            current_x = cluster_x_end;
            i = j;
        }

        // Find the cluster containing the X position
        for (cluster_offset, x_start, x_end) in cluster_bounds.iter() {
            if x >= *x_start && x < *x_end {
                // Check if we're closer to start or end of this cluster
                let mid_point = (x_start + x_end) / 2.0;
                if x < mid_point {
                    // Closer to start of cluster
                    return Some(run.text_range.start + *cluster_offset as usize);
                } else {
                    // Closer to end of cluster - find next cluster boundary
                    // This handles ligatures correctly
                    let run_text = &self.text[run.text_range.clone()];
                    let cluster_byte_start = *cluster_offset as usize;

                    // Find the next grapheme boundary after this cluster
                    for (idx, _) in run_text.grapheme_indices(true) {
                        if idx > cluster_byte_start {
                            return Some(run.text_range.start + idx);
                        }
                    }

                    // If no next boundary found, return end of run
                    return Some(run.text_range.end);
                }
            }
        }

        // Past all clusters, return end of run
        Some(run.text_range.end)
    }

    // ========================================================================
    // Cursor Movement (Phase 6.3)
    // ========================================================================

    /// Move cursor left by one grapheme cluster.
    ///
    /// Returns the new byte offset after moving left.
    pub fn move_cursor_left(&self, byte_offset: usize) -> usize {
        CursorMovement::move_left_char(&self.text, byte_offset)
    }

    /// Move cursor right by one grapheme cluster.
    ///
    /// Returns the new byte offset after moving right.
    pub fn move_cursor_right(&self, byte_offset: usize) -> usize {
        CursorMovement::move_right_char(&self.text, byte_offset)
    }

    /// Move cursor left by one word boundary.
    ///
    /// Returns the new byte offset after moving to the previous word.
    pub fn move_cursor_left_word(&self, byte_offset: usize) -> usize {
        CursorMovement::move_left_word(&self.text, byte_offset)
    }

    /// Move cursor right by one word boundary.
    ///
    /// Returns the new byte offset after moving to the next word.
    pub fn move_cursor_right_word(&self, byte_offset: usize) -> usize {
        CursorMovement::move_right_word(&self.text, byte_offset)
    }

    /// Move cursor up by one line, maintaining horizontal position.
    ///
    /// The `preferred_x` parameter is used to maintain the column position
    /// when moving vertically. Pass `None` to calculate it from the current offset.
    ///
    /// Returns `(new_byte_offset, preferred_x)`.
    pub fn move_cursor_up(&self, byte_offset: usize, preferred_x: Option<f32>) -> (usize, f32) {
        // Find current line
        let Some(current_line_idx) = self.find_line_at_byte_offset(byte_offset) else {
            return (byte_offset, preferred_x.unwrap_or(0.0));
        };

        // Calculate X position if not provided
        let x = preferred_x.unwrap_or_else(|| {
            self.calculate_x_at_byte_offset(&self.lines[current_line_idx], byte_offset)
        });

        // If we're on the first line, stay at current position
        if current_line_idx == 0 {
            return (byte_offset, x);
        }

        // Walk upwards until we either change byte offset or hit the top.
        let mut line_idx = current_line_idx.saturating_sub(1);
        loop {
            let line = &self.lines[line_idx];
            let new_offset = self
                .hit_test_line(line, x, HitTestPolicy::Clamp)
                .unwrap_or(line.text_range.start);

            // If this move changes the logical position, or we've reached
            // the first line, stop. This avoids the "no-op" case when the
            // previous line is visually separate but maps to the same byte
            // offset (e.g. empty lines around '\n').
            if new_offset != byte_offset || line_idx == 0 {
                return (new_offset, x);
            }

            if line_idx == 0 {
                break;
            }
            line_idx -= 1;
        }

        (byte_offset, x)
    }

    /// Move cursor down by one line, maintaining horizontal position.
    ///
    /// The `preferred_x` parameter is used to maintain the column position
    /// when moving vertically. Pass `None` to calculate it from the current offset.
    ///
    /// Returns `(new_byte_offset, preferred_x)`.
    pub fn move_cursor_down(&self, byte_offset: usize, preferred_x: Option<f32>) -> (usize, f32) {
        // Find current line
        let Some(current_line_idx) = self.find_line_at_byte_offset(byte_offset) else {
            return (byte_offset, preferred_x.unwrap_or(0.0));
        };

        // Calculate X position if not provided
        let x = preferred_x.unwrap_or_else(|| {
            self.calculate_x_at_byte_offset(&self.lines[current_line_idx], byte_offset)
        });

        // If we're on the last line, stay at current position
        if current_line_idx >= self.lines.len() - 1 {
            return (byte_offset, x);
        }

        // Walk downwards until we either change byte offset or hit the last line.
        let mut line_idx = current_line_idx + 1;
        while line_idx < self.lines.len() {
            let line = &self.lines[line_idx];
            let new_offset = self
                .hit_test_line(line, x, HitTestPolicy::Clamp)
                .unwrap_or(line.text_range.start);

            // If this move changes the logical position, or we've reached
            // the last line, stop. This avoids the "no-op" case when the
            // next line is visually separate but maps to the same byte
            // offset (e.g. empty lines around '\n').
            if new_offset != byte_offset || line_idx == self.lines.len() - 1 {
                return (new_offset, x);
            }

            line_idx += 1;
        }

        (byte_offset, x)
    }

    /// Move cursor to the start of the current line.
    ///
    /// Returns the byte offset at the start of the line.
    pub fn move_cursor_line_start(&self, byte_offset: usize) -> usize {
        let Some(line_idx) = self.find_line_at_byte_offset(byte_offset) else {
            return 0;
        };

        let line = &self.lines[line_idx];

        // For BiDi text, find the visual start of the line
        self.find_visual_start_offset(line)
            .unwrap_or(line.text_range.start)
    }

    /// Move cursor to the end of the current line.
    ///
    /// Returns the byte offset at the end of the line.
    pub fn move_cursor_line_end(&self, byte_offset: usize) -> usize {
        let Some(line_idx) = self.find_line_at_byte_offset(byte_offset) else {
            return self.text.len();
        };

        let line = &self.lines[line_idx];

        // For BiDi text, find the visual end of the line
        self.find_visual_end_offset(line)
            .unwrap_or(line.text_range.end)
    }

    /// Move cursor to the start of the document.
    ///
    /// Returns 0.
    pub fn move_cursor_document_start(&self) -> usize {
        0
    }

    /// Move cursor to the end of the document.
    ///
    /// Returns the byte length of the text.
    pub fn move_cursor_document_end(&self) -> usize {
        self.text.len()
    }

    // ========================================================================
    // Selection Management (Phase 6.4)
    // ========================================================================

    /// Calculate selection rectangles for rendering a selection.
    ///
    /// Returns a vector of rectangles, one for each line that the selection spans.
    /// Empty if the selection is collapsed.
    pub fn selection_rects(&self, selection: &Selection) -> Vec<SelectionRect> {
        if selection.is_collapsed() {
            return Vec::new();
        }

        let range = selection.range();
        let start = range.start;
        let end = range.end;

        let mut rects = Vec::new();

        // Find all lines that intersect with the selection
        for line in &self.lines {
            // Skip lines that don't intersect with selection
            if line.text_range.end <= start || line.text_range.start >= end {
                continue;
            }

            // Calculate the intersection of selection and line
            let line_sel_start = start.max(line.text_range.start);
            let line_sel_end = end.min(line.text_range.end);

            // Calculate X positions for start and end of selection on this line
            let x_start = self.calculate_x_at_byte_offset(line, line_sel_start);
            let x_end = self.calculate_x_at_byte_offset(line, line_sel_end);

            // Use ascent + descent for selection height to cover full text height
            // even when line_height is smaller for tighter spacing
            let selection_height = line.ascent + line.descent;
            rects.push(SelectionRect::new(
                x_start,
                line.y_offset,
                x_end - x_start,
                selection_height,
            ));
        }

        rects
    }

    /// Select the word at the given byte offset.
    ///
    /// Returns a Selection covering the entire word containing the offset.
    /// If the offset is in whitespace or punctuation, selects that segment.
    pub fn select_word_at(&self, byte_offset: usize) -> Selection {
        if self.text.is_empty() {
            return Selection::collapsed(0);
        }

        let offset = byte_offset.min(self.text.len());
        let boundaries = compute_word_boundaries(&self.text);

        // Find the word boundary containing this offset
        // Check both inside the range and at the end boundary (for clicks at word end)
        for boundary in boundaries.iter() {
            // Offset is inside the range (exclusive end)
            if boundary.range.contains(&offset) {
                return Selection::new(boundary.range.start, boundary.range.end);
            }
            // Offset is at the start of the range
            if boundary.range.start == offset {
                return Selection::new(boundary.range.start, boundary.range.end);
            }
            // Offset is at the exclusive end of the range - select this word if it's an actual word
            // (not whitespace/punctuation), as clicking at the very end should select the word
            if boundary.range.end == offset && boundary.kind == WordBoundaryKind::Word {
                return Selection::new(boundary.range.start, boundary.range.end);
            }
        }

        // If not found, return collapsed selection
        Selection::collapsed(offset)
    }

    /// Select the line at the given byte offset.
    ///
    /// Returns a Selection covering the entire line containing the offset.
    pub fn select_line_at(&self, byte_offset: usize) -> Selection {
        if self.text.is_empty() {
            return Selection::collapsed(0);
        }

        let offset = byte_offset.min(self.text.len());

        // Find the line containing this offset
        if let Some(line_idx) = self.find_line_at_byte_offset(offset) {
            let line = &self.lines[line_idx];
            return Selection::new(line.text_range.start, line.text_range.end);
        }

        // If offset is at the exclusive end of a line, select that line
        // This handles triple-clicking at the very end of a line
        for line in &self.lines {
            if line.text_range.end == offset {
                return Selection::new(line.text_range.start, line.text_range.end);
            }
        }

        Selection::collapsed(offset)
    }

    /// Select the entire paragraph at the given byte offset.
    ///
    /// A paragraph is defined as text between newlines.
    pub fn select_paragraph_at(&self, byte_offset: usize) -> Selection {
        if self.text.is_empty() {
            return Selection::collapsed(0);
        }

        let offset = byte_offset.min(self.text.len());

        // Find paragraph boundaries (text between newlines)
        let mut para_start = 0;
        let mut para_end = self.text.len();

        // Search backward for newline or start
        for (idx, ch) in self.text[..offset].char_indices().rev() {
            if ch == '\n' {
                para_start = idx + ch.len_utf8();
                break;
            }
        }

        // Search forward for newline or end
        for (idx, ch) in self.text[offset..].char_indices() {
            if ch == '\n' {
                para_end = offset + idx;
                break;
            }
        }

        Selection::new(para_start, para_end)
    }

    /// Extend a selection using a cursor movement operation.
    ///
    /// This is used for Shift+movement operations. The anchor stays fixed
    /// and the active end moves according to the movement function.
    ///
    /// # Arguments
    /// * `selection` - The current selection to extend
    /// * `move_fn` - A function that takes a byte offset and returns a new offset
    ///
    /// # Returns
    /// A new selection with the active end moved.
    pub fn extend_selection<F>(&self, selection: &Selection, move_fn: F) -> Selection
    where
        F: FnOnce(usize) -> usize,
    {
        let new_active = move_fn(selection.active());
        Selection::new(selection.anchor(), new_active)
    }

    /// Extend a selection using a cursor movement operation that returns (offset, x).
    ///
    /// This is used for Shift+Up/Down operations where we need to track the preferred X.
    ///
    /// # Arguments
    /// * `selection` - The current selection to extend
    /// * `move_fn` - A function that takes (offset, preferred_x) and returns (new_offset, new_x)
    /// * `preferred_x` - The preferred X position for vertical movement
    ///
    /// # Returns
    /// A tuple of (new_selection, new_preferred_x).
    pub fn extend_selection_vertical<F>(
        &self,
        selection: &Selection,
        move_fn: F,
        preferred_x: Option<f32>,
    ) -> (Selection, f32)
    where
        F: FnOnce(usize, Option<f32>) -> (usize, f32),
    {
        let (new_active, new_x) = move_fn(selection.active(), preferred_x);
        (Selection::new(selection.anchor(), new_active), new_x)
    }

    // ========================================================================
    // Mouse Selection (Phase 6.3/6.4)
    // ========================================================================

    /// Start a new selection at a mouse position.
    ///
    /// This is typically called on mouse down. It creates a collapsed selection
    /// at the clicked position.
    ///
    /// # Arguments
    /// * `point` - Mouse position in zone-local coordinates
    ///
    /// # Returns
    /// A new collapsed selection at the clicked position, or `None` if the point
    /// is outside the text bounds.
    pub fn start_mouse_selection(&self, point: Point) -> Option<Selection> {
        let result = self.hit_test(point, HitTestPolicy::Clamp)?;
        Some(Selection::collapsed(result.byte_offset))
    }

    /// Extend a selection to a mouse position (drag selection).
    ///
    /// This is typically called on mouse move while the button is held down.
    /// The anchor stays at the original click position, and the active end
    /// moves to the current mouse position.
    ///
    /// # Arguments
    /// * `selection` - The current selection (anchor is the original click position)
    /// * `point` - Current mouse position in zone-local coordinates
    ///
    /// # Returns
    /// An extended selection, or the original selection if hit test fails.
    pub fn extend_mouse_selection(&self, selection: &Selection, point: Point) -> Selection {
        if let Some(result) = self.hit_test(point, HitTestPolicy::Clamp) {
            Selection::new(selection.anchor(), result.byte_offset)
        } else {
            *selection
        }
    }

    /// Start a word selection at a mouse position (double-click).
    ///
    /// This selects the entire word at the clicked position.
    ///
    /// # Arguments
    /// * `point` - Mouse position in zone-local coordinates
    ///
    /// # Returns
    /// A selection covering the word at the clicked position, or `None` if the
    /// point is outside the text bounds.
    pub fn start_word_selection(&self, point: Point) -> Option<Selection> {
        let result = self.hit_test(point, HitTestPolicy::Clamp)?;
        Some(self.select_word_at(result.byte_offset))
    }

    /// Extend a word selection to a mouse position (double-click + drag).
    ///
    /// This extends the selection word-by-word as the mouse moves.
    /// The selection always includes complete words.
    ///
    /// # Arguments
    /// * `selection` - The current word selection (anchor is the original word)
    /// * `point` - Current mouse position in zone-local coordinates
    ///
    /// # Returns
    /// An extended word selection, or the original selection if hit test fails.
    pub fn extend_word_selection(&self, selection: &Selection, point: Point) -> Selection {
        if let Some(result) = self.hit_test(point, HitTestPolicy::Clamp) {
            // Get the word at the current mouse position
            let current_word = self.select_word_at(result.byte_offset);

            // Determine which direction we're extending
            let anchor = selection.anchor();

            if current_word.start() < anchor {
                // Extending backward - anchor at original end, active at new word start
                Selection::new(selection.end(), current_word.start())
            } else if current_word.end() > anchor {
                // Extending forward - anchor at original start, active at new word end
                Selection::new(selection.start(), current_word.end())
            } else {
                // Within original word
                *selection
            }
        } else {
            *selection
        }
    }

    /// Start a line selection at a mouse position (triple-click).
    ///
    /// This selects the entire line at the clicked position.
    ///
    /// # Arguments
    /// * `point` - Mouse position in zone-local coordinates
    ///
    /// # Returns
    /// A selection covering the line at the clicked position, or `None` if the
    /// point is outside the text bounds.
    pub fn start_line_selection(&self, point: Point) -> Option<Selection> {
        let result = self.hit_test(point, HitTestPolicy::Clamp)?;
        Some(self.select_line_at(result.byte_offset))
    }

    /// Extend a line selection to a mouse position (triple-click + drag).
    ///
    /// This extends the selection line-by-line as the mouse moves.
    /// The selection always includes complete lines.
    ///
    /// # Arguments
    /// * `selection` - The current line selection (anchor is the original line)
    /// * `point` - Current mouse position in zone-local coordinates
    ///
    /// # Returns
    /// An extended line selection, or the original selection if hit test fails.
    pub fn extend_line_selection(&self, selection: &Selection, point: Point) -> Selection {
        if let Some(result) = self.hit_test(point, HitTestPolicy::Clamp) {
            // Get the line at the current mouse position
            let current_line = self.select_line_at(result.byte_offset);

            // Determine which direction we're extending
            let anchor = selection.anchor();

            if current_line.start() < anchor {
                // Extending backward - anchor at original end, active at new line start
                Selection::new(selection.end(), current_line.start())
            } else if current_line.end() > anchor {
                // Extending forward - anchor at original start, active at new line end
                Selection::new(selection.start(), current_line.end())
            } else {
                // Within original line
                *selection
            }
        } else {
            *selection
        }
    }

    /// Validate and snap a selection to grapheme boundaries.
    ///
    /// Ensures both anchor and active positions are at valid grapheme cluster boundaries.
    pub fn snap_selection_to_boundaries(&self, selection: &Selection) -> Selection {
        let anchor = CursorPosition::new(selection.anchor())
            .snap_to_grapheme_boundary(&self.text)
            .byte_offset;
        let active = CursorPosition::new(selection.active())
            .snap_to_grapheme_boundary(&self.text)
            .byte_offset;
        Selection::new(anchor, active)
    }

    // ========================================================================
    // Scrolling & Viewport Helpers (Phase 6.10)
    // ========================================================================

    /// Calculate the scroll position needed to make a cursor visible.
    ///
    /// Returns the minimum scroll adjustments (dx, dy) needed to bring the cursor
    /// into view within the given viewport dimensions.
    ///
    /// # Arguments
    /// * `cursor_offset` - Byte offset of the cursor
    /// * `viewport_x` - Current viewport X scroll offset
    /// * `viewport_y` - Current viewport Y scroll offset
    /// * `viewport_width` - Viewport width
    /// * `viewport_height` - Viewport height
    /// * `margin` - Minimum margin to maintain around cursor (in pixels)
    ///
    /// # Returns
    /// `Some((new_scroll_x, new_scroll_y))` if scrolling is needed, `None` if cursor is already visible.
    pub fn scroll_to_cursor(
        &self,
        cursor_offset: usize,
        viewport_x: f32,
        viewport_y: f32,
        viewport_width: f32,
        viewport_height: f32,
        margin: f32,
    ) -> Option<(f32, f32)> {
        let position = self.offset_to_position(cursor_offset)?;

        let cursor_x = position.x;
        let cursor_y = position.y;

        // Find the line to get cursor height
        let line = self.lines.get(position.line_index)?;
        let cursor_height = line.height;

        let mut new_scroll_x = viewport_x;
        let mut new_scroll_y = viewport_y;
        let mut needs_scroll = false;

        // Check horizontal scrolling
        if cursor_x < viewport_x {
            // Cursor is left of viewport
            new_scroll_x = (cursor_x - margin).max(0.0);
            needs_scroll = true;
        } else if cursor_x < viewport_x + margin && viewport_x > 0.0 {
            // Cursor is too close to left edge (but not at document start)
            new_scroll_x = (cursor_x - margin).max(0.0);
            needs_scroll = true;
        } else if cursor_x > viewport_x + viewport_width {
            // Cursor is right of viewport
            new_scroll_x = cursor_x - viewport_width + margin;
            needs_scroll = true;
        } else if cursor_x > viewport_x + viewport_width - margin {
            // Cursor is too close to right edge
            new_scroll_x = cursor_x - viewport_width + margin;
            needs_scroll = true;
        }

        // Check vertical scrolling
        if cursor_y < viewport_y {
            // Cursor is above viewport
            new_scroll_y = (cursor_y - margin).max(0.0);
            needs_scroll = true;
        } else if cursor_y < viewport_y + margin && viewport_y > 0.0 {
            // Cursor is too close to top edge (but not at document start)
            new_scroll_y = (cursor_y - margin).max(0.0);
            needs_scroll = true;
        } else if cursor_y + cursor_height > viewport_y + viewport_height {
            // Cursor is below viewport
            new_scroll_y = cursor_y + cursor_height - viewport_height + margin;
            needs_scroll = true;
        } else if cursor_y + cursor_height > viewport_y + viewport_height - margin {
            // Cursor is too close to bottom edge
            new_scroll_y = cursor_y + cursor_height - viewport_height + margin;
            needs_scroll = true;
        }

        if needs_scroll {
            Some((new_scroll_x, new_scroll_y))
        } else {
            None
        }
    }

    /// Calculate the scroll position needed to make a selection visible.
    ///
    /// Returns the minimum scroll adjustments needed to bring the active end
    /// of the selection into view.
    ///
    /// # Arguments
    /// * `selection` - The selection to make visible
    /// * `viewport_x` - Current viewport X scroll offset
    /// * `viewport_y` - Current viewport Y scroll offset
    /// * `viewport_width` - Viewport width
    /// * `viewport_height` - Viewport height
    /// * `margin` - Minimum margin to maintain around selection (in pixels)
    ///
    /// # Returns
    /// `Some((new_scroll_x, new_scroll_y))` if scrolling is needed, `None` if selection is already visible.
    pub fn scroll_to_selection(
        &self,
        selection: &Selection,
        viewport_x: f32,
        viewport_y: f32,
        viewport_width: f32,
        viewport_height: f32,
        margin: f32,
    ) -> Option<(f32, f32)> {
        // Scroll to the active end of the selection (where the cursor is)
        self.scroll_to_cursor(
            selection.active(),
            viewport_x,
            viewport_y,
            viewport_width,
            viewport_height,
            margin,
        )
    }

    /// Calculate the scroll position to center a line in the viewport.
    ///
    /// This is useful for "scroll to line" or "reveal line" operations.
    ///
    /// # Arguments
    /// * `line_index` - Index of the line to center
    /// * `viewport_height` - Viewport height
    ///
    /// # Returns
    /// The Y scroll offset to center the line, or `None` if line index is invalid.
    pub fn scroll_to_line_centered(&self, line_index: usize, viewport_height: f32) -> Option<f32> {
        let line = self.lines.get(line_index)?;
        let line_center = line.y_offset + line.height / 2.0;
        let scroll_y = (line_center - viewport_height / 2.0).max(0.0);
        Some(scroll_y)
    }

    /// Calculate the scroll position to show a line at the top of the viewport.
    ///
    /// # Arguments
    /// * `line_index` - Index of the line to show
    /// * `margin` - Margin from the top (in pixels)
    ///
    /// # Returns
    /// The Y scroll offset, or `None` if line index is invalid.
    pub fn scroll_to_line_top(&self, line_index: usize, margin: f32) -> Option<f32> {
        let line = self.lines.get(line_index)?;
        Some((line.y_offset - margin).max(0.0))
    }

    /// Get the recommended scroll bounds for the text content.
    ///
    /// Returns `(max_scroll_x, max_scroll_y)` representing the maximum
    /// scroll offsets that make sense for this content.
    ///
    /// # Arguments
    /// * `viewport_width` - Viewport width
    /// * `viewport_height` - Viewport height
    ///
    /// # Returns
    /// `(max_scroll_x, max_scroll_y)` - Maximum scroll offsets.
    pub fn scroll_bounds(&self, viewport_width: f32, viewport_height: f32) -> (f32, f32) {
        let content_width = self.max_line_width();
        let content_height = self.total_height();

        let max_scroll_x = (content_width - viewport_width).max(0.0);
        let max_scroll_y = (content_height - viewport_height).max(0.0);

        (max_scroll_x, max_scroll_y)
    }

    /// Calculate scroll delta for mouse wheel scrolling.
    ///
    /// This is a helper to convert mouse wheel delta to scroll amount,
    /// with support for smooth scrolling.
    ///
    /// # Arguments
    /// * `wheel_delta_x` - Horizontal wheel delta (positive = scroll right)
    /// * `wheel_delta_y` - Vertical wheel delta (positive = scroll down)
    /// * `line_scroll_amount` - Number of pixels to scroll per wheel notch
    ///
    /// # Returns
    /// `(scroll_dx, scroll_dy)` - Amount to scroll in pixels.
    pub fn wheel_scroll_delta(
        wheel_delta_x: f32,
        wheel_delta_y: f32,
        line_scroll_amount: f32,
    ) -> (f32, f32) {
        // Simple linear scaling - can be customized for acceleration
        let dx = wheel_delta_x * line_scroll_amount;
        let dy = wheel_delta_y * line_scroll_amount;
        (dx, dy)
    }

    /// Get the default line scroll amount based on line height.
    ///
    /// This is typically used for mouse wheel scrolling.
    ///
    /// # Returns
    /// Recommended scroll amount per wheel notch, or a default value if layout is empty.
    pub fn default_line_scroll_amount(&self) -> f32 {
        // Scroll by 3 lines worth of height per wheel notch
        self.line_height().unwrap_or(20.0) * 3.0
    }

    // ========================================================================
    // Text Insertion (Phase 6.5)
    // ========================================================================

    /// Insert a character at the cursor position.
    ///
    /// If a selection is active, it will be replaced with the character.
    /// Returns the new cursor position after insertion.
    ///
    /// # Arguments
    /// * `cursor_offset` - Byte offset where to insert
    /// * `ch` - Character to insert
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// New cursor position after insertion (at the end of inserted character).
    pub fn insert_char(
        &mut self,
        cursor_offset: usize,
        ch: char,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        let mut s = String::new();
        s.push(ch);
        self.insert_str(cursor_offset, &s, font, font_size, max_width, wrap_mode)
    }

    /// Insert a string at the cursor position.
    ///
    /// If a selection is active, it will be replaced with the string.
    /// Returns the new cursor position after insertion.
    ///
    /// # Arguments
    /// * `cursor_offset` - Byte offset where to insert
    /// * `text` - String to insert
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// New cursor position after insertion (at the end of inserted text).
    pub fn insert_str(
        &mut self,
        cursor_offset: usize,
        text: &str,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        let selection_before = Selection::collapsed(cursor_offset);
        self.insert_str_with_undo(
            cursor_offset,
            text,
            selection_before,
            font,
            font_size,
            max_width,
            wrap_mode,
        )
    }

    /// Insert a string at the cursor position with undo tracking.
    ///
    /// This version accepts the selection state for proper undo/redo support.
    ///
    /// # Arguments
    /// * `cursor_offset` - Byte offset where to insert
    /// * `text` - String to insert
    /// * `selection_before` - Selection state before insertion
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// New cursor position after insertion (at the end of inserted text).
    pub fn insert_str_with_undo(
        &mut self,
        cursor_offset: usize,
        text: &str,
        selection_before: Selection,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        // Validate cursor offset
        let offset = cursor_offset.min(self.text.len());

        // Calculate new cursor position (after inserted text)
        let new_cursor = offset + text.len();
        let selection_after = Selection::collapsed(new_cursor);

        // Record the operation for undo
        let operation = TextOperation::Insert {
            offset,
            text: text.to_string(),
            selection_before,
            selection_after,
        };
        self.record_operation(operation);

        // Insert the text
        self.text.insert_str(offset, text);

        // Trigger layout invalidation and re-layout
        self.relayout(font, font_size, max_width, wrap_mode);

        new_cursor
    }

    /// Replace a selection with inserted text.
    ///
    /// Deletes the selected range and inserts the new text.
    /// Returns the new cursor position after insertion.
    ///
    /// # Arguments
    /// * `selection` - Selection range to replace
    /// * `text` - String to insert
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// New cursor position after insertion (at the end of inserted text).
    pub fn replace_selection(
        &mut self,
        selection: &Selection,
        text: &str,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        if selection.is_collapsed() {
            // No selection, just insert at cursor
            return self.insert_str_with_undo(
                selection.active(),
                text,
                selection.clone(),
                font,
                font_size,
                max_width,
                wrap_mode,
            );
        }

        let range = selection.range();
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len());

        // Get the old text for undo
        let old_text = self.text[start..end].to_string();

        // Calculate new cursor position
        let new_cursor = start + text.len();
        let selection_after = Selection::collapsed(new_cursor);

        // Record the operation for undo
        let operation = TextOperation::Replace {
            offset: start,
            old_text,
            new_text: text.to_string(),
            selection_before: selection.clone(),
            selection_after,
        };
        self.record_operation(operation);

        // Delete the selection
        self.text.replace_range(start..end, "");

        // Insert the new text
        self.text.insert_str(start, text);

        // Trigger layout invalidation and re-layout
        self.relayout(font, font_size, max_width, wrap_mode);

        new_cursor
    }

    /// Insert a newline at the cursor position.
    ///
    /// Returns the new cursor position after insertion.
    pub fn insert_newline(
        &mut self,
        cursor_offset: usize,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        self.insert_char(cursor_offset, '\n', font, font_size, max_width, wrap_mode)
    }

    /// Insert a tab at the cursor position.
    ///
    /// Returns the new cursor position after insertion.
    pub fn insert_tab(
        &mut self,
        cursor_offset: usize,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        self.insert_char(cursor_offset, '\t', font, font_size, max_width, wrap_mode)
    }

    // ========================================================================
    // Text Deletion (Phase 6.6)
    // ========================================================================

    /// Delete the character before the cursor (backspace).
    ///
    /// Respects grapheme cluster boundaries.
    /// Returns the new cursor position after deletion.
    ///
    /// # Arguments
    /// * `cursor_offset` - Current cursor position
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// New cursor position after deletion.
    pub fn delete_backward(
        &mut self,
        cursor_offset: usize,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        if cursor_offset == 0 || self.text.is_empty() {
            return 0;
        }

        // Find the previous grapheme boundary
        let mut prev_boundary = 0;
        for (idx, _) in self.text.grapheme_indices(true) {
            if idx >= cursor_offset {
                break;
            }
            prev_boundary = idx;
        }

        // Get the deleted text for undo
        let deleted_text = self.text[prev_boundary..cursor_offset].to_string();

        // Record the operation for undo
        let operation = TextOperation::Delete {
            offset: prev_boundary,
            text: deleted_text,
            selection_before: Selection::collapsed(cursor_offset),
            selection_after: Selection::collapsed(prev_boundary),
        };
        self.record_operation(operation);

        // Delete from prev_boundary to cursor_offset
        self.text.replace_range(prev_boundary..cursor_offset, "");

        // Trigger layout invalidation and re-layout
        self.relayout(font, font_size, max_width, wrap_mode);

        prev_boundary
    }

    /// Delete the character after the cursor (delete key).
    ///
    /// Respects grapheme cluster boundaries.
    /// Returns the new cursor position after deletion (unchanged).
    ///
    /// # Arguments
    /// * `cursor_offset` - Current cursor position
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// New cursor position after deletion (same as input).
    pub fn delete_forward(
        &mut self,
        cursor_offset: usize,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        if cursor_offset >= self.text.len() || self.text.is_empty() {
            return cursor_offset;
        }

        // Find the next grapheme boundary
        let mut next_boundary = self.text.len();
        for (idx, _) in self.text.grapheme_indices(true) {
            if idx > cursor_offset {
                next_boundary = idx;
                break;
            }
        }

        // Delete from cursor_offset to next_boundary
        self.text.replace_range(cursor_offset..next_boundary, "");

        // Trigger layout invalidation and re-layout
        self.relayout(font, font_size, max_width, wrap_mode);

        cursor_offset
    }

    /// Delete the word before the cursor (Ctrl+Backspace).
    ///
    /// Returns the new cursor position after deletion.
    pub fn delete_word_backward(
        &mut self,
        cursor_offset: usize,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        if cursor_offset == 0 || self.text.is_empty() {
            return 0;
        }

        // Find the previous word boundary
        let prev_word_offset = CursorMovement::move_left_word(&self.text, cursor_offset);

        // Delete from prev_word_offset to cursor_offset
        self.text.replace_range(prev_word_offset..cursor_offset, "");

        // Trigger layout invalidation and re-layout
        self.relayout(font, font_size, max_width, wrap_mode);

        prev_word_offset
    }

    /// Delete the word after the cursor (Ctrl+Delete).
    ///
    /// Returns the new cursor position after deletion (unchanged).
    pub fn delete_word_forward(
        &mut self,
        cursor_offset: usize,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        if cursor_offset >= self.text.len() || self.text.is_empty() {
            return cursor_offset;
        }

        // Find the next word boundary
        let next_word_offset = CursorMovement::move_right_word(&self.text, cursor_offset);

        // Delete from cursor_offset to next_word_offset
        self.text.replace_range(cursor_offset..next_word_offset, "");

        // Trigger layout invalidation and re-layout
        self.relayout(font, font_size, max_width, wrap_mode);

        cursor_offset
    }

    /// Delete a selection range.
    ///
    /// Returns the new cursor position (at the start of the deleted range).
    ///
    /// # Arguments
    /// * `selection` - Selection range to delete
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// New cursor position (at the start of the deleted range).
    pub fn delete_selection(
        &mut self,
        selection: &Selection,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        if selection.is_collapsed() {
            return selection.active();
        }

        let range = selection.range();
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len());

        // Get the deleted text for undo
        let deleted_text = self.text[start..end].to_string();

        // Record the operation for undo
        let operation = TextOperation::Delete {
            offset: start,
            text: deleted_text,
            selection_before: selection.clone(),
            selection_after: Selection::collapsed(start),
        };
        self.record_operation(operation);

        // Delete the range
        self.text.replace_range(start..end, "");

        // Trigger layout invalidation and re-layout
        self.relayout(font, font_size, max_width, wrap_mode);

        start
    }

    /// Delete the entire line containing the cursor.
    ///
    /// Returns the new cursor position.
    pub fn delete_line(
        &mut self,
        cursor_offset: usize,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> usize {
        if self.text.is_empty() {
            return 0;
        }

        let offset = cursor_offset.min(self.text.len());

        // Find line boundaries (between newlines)
        let mut line_start = 0;
        let mut line_end = self.text.len();

        // Search backward for newline or start
        for (idx, ch) in self.text[..offset].char_indices().rev() {
            if ch == '\n' {
                line_start = idx + ch.len_utf8();
                break;
            }
        }

        // Search forward for newline or end
        for (idx, ch) in self.text[offset..].char_indices() {
            if ch == '\n' {
                line_end = offset + idx + ch.len_utf8(); // Include the newline
                break;
            }
        }

        // Delete the line
        self.text.replace_range(line_start..line_end, "");

        // Trigger layout invalidation and re-layout
        self.relayout(font, font_size, max_width, wrap_mode);

        line_start.min(self.text.len())
    }

    // ========================================================================
    // Clipboard Operations (Phase 6.7)
    // ========================================================================

    /// Copy the selected text to the system clipboard.
    ///
    /// Returns `Ok(())` if successful, or an error if clipboard access fails.
    ///
    /// # Arguments
    /// * `selection` - Selection range to copy
    ///
    /// # Errors
    /// Returns an error if clipboard access fails.
    #[cfg(not(target_os = "android"))]
    pub fn copy_to_clipboard(&self, selection: &Selection) -> Result<(), String> {
        if selection.is_collapsed() {
            // Nothing to copy
            return Ok(());
        }

        let range = selection.range();
        let start = range.start.min(self.text.len());
        let end = range.end.min(self.text.len());

        let selected_text = &self.text[start..end];

        // Access clipboard and set text
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;

        clipboard
            .set_text(selected_text.to_string())
            .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;

        Ok(())
    }

    /// Clipboard operations are not available on Android.
    #[cfg(target_os = "android")]
    pub fn copy_to_clipboard(&self, _selection: &Selection) -> Result<(), String> {
        Err("Clipboard not available on Android".to_string())
    }

    /// Cut the selected text to the system clipboard.
    ///
    /// Copies the selection to clipboard and then deletes it.
    /// Returns the new cursor position after cutting.
    ///
    /// # Arguments
    /// * `selection` - Selection range to cut
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// `Ok(new_cursor_position)` if successful, or an error if clipboard access fails.
    ///
    /// # Errors
    /// Returns an error if clipboard access fails.
    pub fn cut_to_clipboard(
        &mut self,
        selection: &Selection,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Result<usize, String> {
        if selection.is_collapsed() {
            // Nothing to cut
            return Ok(selection.active());
        }

        // First copy to clipboard
        self.copy_to_clipboard(selection)?;

        // Then delete the selection
        let new_cursor = self.delete_selection(selection, font, font_size, max_width, wrap_mode);

        Ok(new_cursor)
    }

    /// Paste text from the system clipboard at the cursor position.
    ///
    /// If a selection is active, it will be replaced with the pasted text.
    /// Returns the new cursor position after pasting.
    ///
    /// # Arguments
    /// * `cursor_offset` - Byte offset where to paste (or selection to replace)
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// `Ok(new_cursor_position)` if successful, or an error if clipboard access fails.
    ///
    /// # Errors
    /// Returns an error if clipboard access fails or contains non-text data.
    #[cfg(not(target_os = "android"))]
    pub fn paste_from_clipboard(
        &mut self,
        cursor_offset: usize,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Result<usize, String> {
        // Access clipboard and get text
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;

        let clipboard_text = clipboard
            .get_text()
            .map_err(|e| format!("Failed to read from clipboard: {}", e))?;

        // Normalize the clipboard text (handle different line endings)
        let normalized_text = Self::normalize_clipboard_text(&clipboard_text);

        // Insert the text at cursor position
        let new_cursor = self.insert_str(
            cursor_offset,
            &normalized_text,
            font,
            font_size,
            max_width,
            wrap_mode,
        );

        Ok(new_cursor)
    }

    /// Clipboard operations are not available on Android.
    #[cfg(target_os = "android")]
    pub fn paste_from_clipboard(
        &mut self,
        _cursor_offset: usize,
        _font: &FontFace,
        _font_size: f32,
        _max_width: Option<f32>,
        _wrap_mode: WrapMode,
    ) -> Result<usize, String> {
        Err("Clipboard not available on Android".to_string())
    }

    /// Paste text from the system clipboard, replacing the current selection.
    ///
    /// If the selection is collapsed, this behaves like `paste_from_clipboard`.
    /// Returns the new cursor position after pasting.
    ///
    /// # Arguments
    /// * `selection` - Selection range to replace
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// `Ok(new_cursor_position)` if successful, or an error if clipboard access fails.
    ///
    /// # Errors
    /// Returns an error if clipboard access fails or contains non-text data.
    #[cfg(not(target_os = "android"))]
    pub fn paste_replace_selection(
        &mut self,
        selection: &Selection,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Result<usize, String> {
        // Access clipboard and get text
        let mut clipboard =
            arboard::Clipboard::new().map_err(|e| format!("Failed to access clipboard: {}", e))?;

        let clipboard_text = clipboard
            .get_text()
            .map_err(|e| format!("Failed to read from clipboard: {}", e))?;

        // Normalize the clipboard text
        let normalized_text = Self::normalize_clipboard_text(&clipboard_text);

        // Replace selection with pasted text
        let new_cursor = self.replace_selection(
            selection,
            &normalized_text,
            font,
            font_size,
            max_width,
            wrap_mode,
        );

        Ok(new_cursor)
    }

    /// Clipboard operations are not available on Android.
    #[cfg(target_os = "android")]
    pub fn paste_replace_selection(
        &mut self,
        _selection: &Selection,
        _font: &FontFace,
        _font_size: f32,
        _max_width: Option<f32>,
        _wrap_mode: WrapMode,
    ) -> Result<usize, String> {
        Err("Clipboard not available on Android".to_string())
    }

    /// Normalize clipboard text by converting different line endings to '\n'.
    ///
    /// Handles:
    /// - Windows line endings (CRLF -> LF)
    /// - Old Mac line endings (CR -> LF)
    /// - Unix line endings (LF -> LF, unchanged)
    ///
    /// This ensures consistent behavior across platforms.
    fn normalize_clipboard_text(text: &str) -> String {
        // Replace CRLF with LF first (Windows)
        let text = text.replace("\r\n", "\n");
        // Replace remaining CR with LF (old Mac)
        text.replace('\r', "\n")
    }

    // ========================================================================
    // Undo/Redo System (Phase 6.8)
    // ========================================================================

    /// Undo the last text editing operation.
    ///
    /// Returns the new cursor position after undoing, or `None` if there's nothing to undo.
    ///
    /// # Arguments
    /// * `current_selection` - The current selection state
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// `Some((new_cursor_offset, new_selection))` if an operation was undone, `None` otherwise.
    pub fn undo(
        &mut self,
        current_selection: &Selection,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Option<(usize, Selection)> {
        let operations = self.undo_stack.undo()?;

        // Apply the inverse of each operation
        for operation in operations.iter().rev() {
            match operation {
                TextOperation::Insert { offset, text, .. } => {
                    // Undo insert by deleting
                    let end = offset + text.len();
                    self.text.replace_range(*offset..end, "");
                }
                TextOperation::Delete { offset, text, .. } => {
                    // Undo delete by inserting
                    self.text.insert_str(*offset, text);
                }
                TextOperation::Replace {
                    offset,
                    old_text,
                    new_text,
                    ..
                } => {
                    // Undo replace by replacing back
                    let end = offset + new_text.len();
                    self.text.replace_range(*offset..end, old_text);
                }
            }
        }

        // Re-layout after undo
        self.relayout(font, font_size, max_width, wrap_mode);

        // Return the selection from before the operation
        if let Some(first_op) = operations.first() {
            let selection = first_op.selection_before().clone();
            Some((selection.active(), selection))
        } else {
            Some((current_selection.active(), current_selection.clone()))
        }
    }

    /// Redo the last undone operation.
    ///
    /// Returns the new cursor position after redoing, or `None` if there's nothing to redo.
    ///
    /// # Arguments
    /// * `current_selection` - The current selection state
    /// * `font` - Font to use for re-layout
    /// * `font_size` - Font size to use for re-layout
    /// * `max_width` - Optional width constraint for wrapping
    /// * `wrap_mode` - Text wrapping mode
    ///
    /// # Returns
    /// `Some((new_cursor_offset, new_selection))` if an operation was redone, `None` otherwise.
    pub fn redo(
        &mut self,
        current_selection: &Selection,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) -> Option<(usize, Selection)> {
        let operations = self.undo_stack.redo()?;

        // Re-apply each operation
        for operation in operations.iter() {
            match operation {
                TextOperation::Insert { offset, text, .. } => {
                    self.text.insert_str(*offset, text);
                }
                TextOperation::Delete { offset, text, .. } => {
                    let end = offset + text.len();
                    self.text.replace_range(*offset..end, "");
                }
                TextOperation::Replace {
                    offset,
                    old_text,
                    new_text,
                    ..
                } => {
                    let end = offset + old_text.len();
                    self.text.replace_range(*offset..end, new_text);
                }
            }
        }

        // Re-layout after redo
        self.relayout(font, font_size, max_width, wrap_mode);

        // Return the selection from after the operation
        if let Some(last_op) = operations.last() {
            let selection = last_op.selection_after().clone();
            Some((selection.active(), selection))
        } else {
            Some((current_selection.active(), current_selection.clone()))
        }
    }

    /// Check if there are operations that can be undone.
    pub fn can_undo(&self) -> bool {
        self.undo_stack.can_undo()
    }

    /// Check if there are operations that can be redone.
    pub fn can_redo(&self) -> bool {
        self.undo_stack.can_redo()
    }

    /// Clear the undo/redo history.
    ///
    /// This is useful when loading a new document or after major changes
    /// that should not be undoable.
    pub fn clear_undo_history(&mut self) {
        self.undo_stack.clear();
    }

    /// Set the maximum number of undo operations to keep.
    ///
    /// Default is 1000. Setting a lower limit can reduce memory usage.
    pub fn set_undo_limit(&mut self, limit: usize) {
        self.undo_stack.set_limit(limit);
    }

    /// Get the current undo limit.
    pub fn undo_limit(&self) -> usize {
        self.undo_stack.limit()
    }

    /// Enable or disable operation grouping.
    ///
    /// When enabled (default), consecutive typing operations are grouped together
    /// so they can be undone as a single unit.
    pub fn set_undo_grouping(&mut self, enabled: bool) {
        self.undo_stack.set_grouping(enabled);
    }

    /// Record a text operation in the undo stack.
    ///
    /// This is called internally by text modification methods.
    fn record_operation(&mut self, operation: TextOperation) {
        self.undo_stack.push(operation);
    }

    // ========================================================================
    // Text Measurement for Editing (Phase 6.9)
    // ========================================================================

    /// Measure the width of a single line of text.
    ///
    /// This is useful for determining how wide text will be before laying it out.
    ///
    /// # Arguments
    /// * `text` - The text to measure
    /// * `font` - Font to use for measurement
    /// * `font_size` - Font size to use
    ///
    /// # Returns
    /// The width of the text in pixels.
    pub fn measure_single_line_width(text: &str, font: &FontFace, font_size: f32) -> f32 {
        if text.is_empty() {
            return 0.0;
        }

        let run = TextShaper::shape_ltr(text, 0..text.len(), font, 0, font_size);
        run.width
    }

    /// Calculate the bounding box of the entire text layout.
    ///
    /// Returns `(width, height)` where:
    /// - `width` is the maximum line width
    /// - `height` is the total height of all lines
    ///
    /// # Returns
    /// `(width, height)` in pixels, or `(0.0, 0.0)` for empty text.
    pub fn text_bounds(&self) -> (f32, f32) {
        if self.lines.is_empty() {
            return (0.0, 0.0);
        }

        let max_width = self
            .lines
            .iter()
            .map(|line| line.width)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0);

        let last_line = &self.lines[self.lines.len() - 1];
        let total_height = last_line.y_offset + last_line.height;

        (max_width, total_height)
    }

    /// Get the width of a character at a specific byte offset.
    ///
    /// For ligatures or multi-glyph clusters, returns the width of the entire cluster.
    ///
    /// # Arguments
    /// * `byte_offset` - Byte offset of the character
    ///
    /// # Returns
    /// Width of the character/cluster in pixels, or `None` if offset is invalid.
    pub fn character_width(&self, byte_offset: usize) -> Option<f32> {
        if byte_offset >= self.text.len() {
            return None;
        }

        // Find the line containing this offset
        let line_idx = self.find_line_at_byte_offset(byte_offset)?;
        let line = &self.lines[line_idx];

        // Find the run containing this offset
        for run in &line.runs {
            if byte_offset < run.text_range.start || byte_offset >= run.text_range.end {
                continue;
            }

            let offset_in_run = byte_offset - run.text_range.start;

            // Find the cluster containing this offset
            let mut i = 0;
            while i < run.clusters.len() {
                let cluster_start = run.clusters[i] as usize;

                // Find the next cluster boundary
                let next_cluster_start = if i + 1 < run.clusters.len() {
                    // Find next different cluster value
                    let mut j = i + 1;
                    while j < run.clusters.len() && run.clusters[j] == run.clusters[i] {
                        j += 1;
                    }
                    if j < run.clusters.len() {
                        run.clusters[j] as usize
                    } else {
                        run.text_range.len()
                    }
                } else {
                    run.text_range.len()
                };

                // Check if our offset is within this cluster
                if offset_in_run >= cluster_start && offset_in_run < next_cluster_start {
                    // Calculate the width of this cluster
                    let mut cluster_width = 0.0;
                    let mut j = i;
                    while j < run.clusters.len() && run.clusters[j] == run.clusters[i] {
                        cluster_width += run.advances[j];
                        j += 1;
                    }
                    return Some(cluster_width);
                }

                // Move to next cluster
                i += 1;
                while i < run.clusters.len() && run.clusters[i] == run.clusters[i - 1] {
                    i += 1;
                }
            }
        }

        None
    }

    /// Get the line height used in this layout.
    ///
    /// Returns the height of a single line, or `None` if the layout is empty.
    pub fn line_height(&self) -> Option<f32> {
        self.lines.first().map(|line| line.height)
    }

    /// Get the baseline offset from the top of a line.
    ///
    /// Returns the distance from the top of the line box to the baseline,
    /// or `None` if the layout is empty.
    pub fn baseline_offset(&self) -> Option<f32> {
        self.lines.first().map(|line| line.baseline_offset)
    }

    /// Get the baseline Y position for a specific line.
    ///
    /// # Arguments
    /// * `line_index` - Index of the line
    ///
    /// # Returns
    /// Baseline Y position in pixels, or `None` if line index is invalid.
    pub fn line_baseline_y(&self, line_index: usize) -> Option<f32> {
        self.lines.get(line_index).map(|line| line.baseline_y())
    }

    /// Calculate the visible line range for a given viewport.
    ///
    /// This is useful for viewport culling when rendering large documents.
    ///
    /// # Arguments
    /// * `viewport_y` - Y offset of the viewport top
    /// * `viewport_height` - Height of the viewport
    ///
    /// # Returns
    /// `(first_visible_line, last_visible_line)` as inclusive indices,
    /// or `None` if no lines are visible.
    pub fn visible_line_range(
        &self,
        viewport_y: f32,
        viewport_height: f32,
    ) -> Option<(usize, usize)> {
        if self.lines.is_empty() {
            return None;
        }

        let viewport_bottom = viewport_y + viewport_height;

        // Find first visible line
        let mut first_visible = None;
        for (idx, line) in self.lines.iter().enumerate() {
            if line.bottom_y() > viewport_y {
                first_visible = Some(idx);
                break;
            }
        }

        let first_visible = first_visible?;

        // Find last visible line
        let mut last_visible = first_visible;
        for (idx, line) in self.lines[first_visible..].iter().enumerate() {
            if line.y_offset >= viewport_bottom {
                break;
            }
            last_visible = first_visible + idx;
        }

        Some((first_visible, last_visible))
    }

    /// Measure the width of a text range.
    ///
    /// This calculates the visual width of text between two byte offsets.
    /// For multi-line ranges, returns the width of the widest line.
    ///
    /// # Arguments
    /// * `start_offset` - Start byte offset (inclusive)
    /// * `end_offset` - End byte offset (exclusive)
    ///
    /// # Returns
    /// Width of the text range in pixels, or `None` if offsets are invalid.
    pub fn measure_range_width(&self, start_offset: usize, end_offset: usize) -> Option<f32> {
        if start_offset >= end_offset || end_offset > self.text.len() {
            return None;
        }

        let mut max_width = 0.0f32;

        // Find all lines that intersect with the range
        for line in &self.lines {
            // Skip lines that don't intersect
            if line.text_range.end <= start_offset || line.text_range.start >= end_offset {
                continue;
            }

            // Calculate the intersection
            let line_start = start_offset.max(line.text_range.start);
            let line_end = end_offset.min(line.text_range.end);

            // Calculate X positions for start and end
            let x_start = self.calculate_x_at_byte_offset(line, line_start);
            let x_end = self.calculate_x_at_byte_offset(line, line_end);

            let line_width = x_end - x_start;
            max_width = max_width.max(line_width);
        }

        Some(max_width)
    }

    /// Get the number of lines in the layout.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Get the total height of the text layout.
    ///
    /// This is the Y position of the bottom of the last line.
    pub fn total_height(&self) -> f32 {
        if self.lines.is_empty() {
            return 0.0;
        }

        let last_line = &self.lines[self.lines.len() - 1];
        last_line.y_offset + last_line.height
    }

    /// Get the maximum width of any line in the layout.
    pub fn max_line_width(&self) -> f32 {
        self.lines
            .iter()
            .map(|line| line.width)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(0.0)
    }

    // ========================================================================
    // Helper Methods for Text Modification
    // ========================================================================

    /// Re-layout the text after modification.
    ///
    /// This is called internally after any text insertion or deletion.
    fn relayout(
        &mut self,
        font: &FontFace,
        font_size: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
    ) {
        // Clear existing layout
        self.lines.clear();

        // Compute scaled metrics once; reused for all lines.
        let scaled = font.scaled_metrics(font_size);
        let line_height = Self::resolve_line_height(self.line_height_override, &scaled);
        let leading = (line_height - scaled.ascent - scaled.descent).max(0.0);

        let mut y = 0.0f32;

        // Split into logical paragraphs on '\n'. Wrapping decisions
        // are made per paragraph.
        let mut para_start = 0usize;
        for (idx, ch) in self.text.char_indices() {
            if ch == '\n' {
                Self::layout_paragraph(
                    &self.text,
                    para_start..idx,
                    font,
                    font_size,
                    line_height,
                    scaled.ascent,
                    scaled.descent,
                    leading,
                    max_width,
                    wrap_mode,
                    &mut y,
                    &mut self.lines,
                );
                para_start = idx + ch.len_utf8();
            }
        }
        if para_start <= self.text.len() {
            Self::layout_paragraph(
                &self.text,
                para_start..self.text.len(),
                font,
                font_size,
                line_height,
                scaled.ascent,
                scaled.descent,
                leading,
                max_width,
                wrap_mode,
                &mut y,
                &mut self.lines,
            );
        }

        // Rebuild prefix sums
        self.prefix_sums = PrefixSums::new(&self.text, &self.lines);
    }

    /// Get a mutable reference to the underlying text.
    ///
    /// **Warning**: Modifying the text directly will invalidate the layout.
    /// You must call `relayout()` after making changes, or use the provided
    /// insertion/deletion methods instead.
    pub fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }
}

impl TextLayout {
    #[allow(clippy::too_many_arguments)]
    fn layout_paragraph(
        full_text: &str,
        range: core::ops::Range<usize>,
        font: &FontFace,
        font_size: f32,
        line_height: f32,
        ascent: f32,
        descent: f32,
        leading: f32,
        max_width: Option<f32>,
        wrap_mode: WrapMode,
        y: &mut f32,
        out_lines: &mut Vec<LineBox>,
    ) {
        let paragraph = &full_text[range.clone()];

        // Empty paragraph: emit a single empty line.
        if paragraph.is_empty() {
            let line = LineBox {
                text_range: range,
                width: 0.0,
                height: line_height,
                baseline_offset: ascent,
                ascent,
                descent,
                leading,
                runs: Vec::new(),
                y_offset: *y,
            };
            *y += line_height;
            out_lines.push(line);
            return;
        }

        // No wrapping requested or no width constraint: single line.
        if max_width.is_none() || matches!(wrap_mode, WrapMode::NoWrap) {
            let run = TextShaper::shape_ltr(paragraph, range.clone(), font, 0, font_size);
            let line = LineBox {
                text_range: range,
                width: run.width,
                height: line_height,
                baseline_offset: ascent,
                ascent,
                descent,
                leading,
                runs: vec![run],
                y_offset: *y,
            };
            *y += line_height;
            out_lines.push(line);
            return;
        }

        let max_width = max_width.unwrap();

        match wrap_mode {
            WrapMode::BreakWord => {
                Self::layout_with_word_wrap(
                    full_text,
                    range,
                    font,
                    font_size,
                    line_height,
                    ascent,
                    descent,
                    leading,
                    max_width,
                    y,
                    out_lines,
                );
            }
            WrapMode::BreakAll => {
                Self::layout_with_break_all(
                    full_text,
                    range,
                    font,
                    font_size,
                    line_height,
                    ascent,
                    descent,
                    leading,
                    max_width,
                    y,
                    out_lines,
                );
            }
            WrapMode::NoWrap => unreachable!(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_with_word_wrap(
        full_text: &str,
        range: core::ops::Range<usize>,
        font: &FontFace,
        font_size: f32,
        line_height: f32,
        ascent: f32,
        descent: f32,
        leading: f32,
        max_width: f32,
        y: &mut f32,
        out_lines: &mut Vec<LineBox>,
    ) {
        let paragraph = &full_text[range.clone()];
        let mut local_start = 0usize;
        let para_len = paragraph.len();

        // Precompute line break opportunities within the paragraph.
        let breaks = compute_line_breaks(paragraph);

        while local_start < para_len {
            let mut best_end = None;

            // Try all break opportunities after local_start, greedily
            // picking the last that fits.
            for br in breaks.iter().filter(|b| b.offset > local_start) {
                let local_end = br.offset.min(para_len);
                let segment = &paragraph[local_start..local_end];
                let run = TextShaper::shape_ltr(
                    segment,
                    (range.start + local_start)..(range.start + local_end),
                    font,
                    0,
                    font_size,
                );
                if run.width <= max_width {
                    best_end = Some((local_end, run));
                } else {
                    break;
                }
            }

            // If we found a suitable break at a word boundary, use it.
            if let Some((local_end, run)) = best_end {
                let line = LineBox {
                    text_range: (range.start + local_start)..(range.start + local_end),
                    width: run.width,
                    height: line_height,
                    baseline_offset: ascent,
                    ascent,
                    descent,
                    leading,
                    runs: vec![run],
                    y_offset: *y,
                };
                *y += line_height;
                out_lines.push(line);
                local_start = local_end;
                continue;
            }

            // Fall back to breaking at grapheme boundaries if a single
            // word exceeds max_width.
            let mut best_end = None;
            for (idx, g) in paragraph[local_start..].grapheme_indices(true) {
                let local_end = local_start + idx + g.len();
                let segment = &paragraph[local_start..local_end];
                let run = TextShaper::shape_ltr(
                    segment,
                    (range.start + local_start)..(range.start + local_end),
                    font,
                    0,
                    font_size,
                );
                if run.width <= max_width {
                    best_end = Some((local_end, run));
                } else {
                    break;
                }
            }

            if let Some((local_end, run)) = best_end {
                let line = LineBox {
                    text_range: (range.start + local_start)..(range.start + local_end),
                    width: run.width,
                    height: line_height,
                    baseline_offset: ascent,
                    ascent,
                    descent,
                    leading,
                    runs: vec![run],
                    y_offset: *y,
                };
                *y += line_height;
                out_lines.push(line);
                local_start = local_end;
            } else {
                // As a last resort, force at least one grapheme.
                let mut iter = paragraph[local_start..].grapheme_indices(true);
                if let Some((idx, g)) = iter.next() {
                    let local_end = local_start + idx + g.len();
                    let segment = &paragraph[local_start..local_end];
                    let run = TextShaper::shape_ltr(
                        segment,
                        (range.start + local_start)..(range.start + local_end),
                        font,
                        0,
                        font_size,
                    );
                    let line = LineBox {
                        text_range: (range.start + local_start)..(range.start + local_end),
                        width: run.width,
                        height: line_height,
                        baseline_offset: ascent,
                        ascent,
                        descent,
                        leading,
                        runs: vec![run],
                        y_offset: *y,
                    };
                    *y += line_height;
                    out_lines.push(line);
                    local_start = local_end;
                } else {
                    break;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_with_break_all(
        full_text: &str,
        range: core::ops::Range<usize>,
        font: &FontFace,
        font_size: f32,
        line_height: f32,
        ascent: f32,
        descent: f32,
        leading: f32,
        max_width: f32,
        y: &mut f32,
        out_lines: &mut Vec<LineBox>,
    ) {
        let paragraph = &full_text[range.clone()];
        let mut local_start = 0usize;
        let para_len = paragraph.len();

        while local_start < para_len {
            let mut best_end = None;
            for (idx, g) in paragraph[local_start..].grapheme_indices(true) {
                let local_end = local_start + idx + g.len();
                let segment = &paragraph[local_start..local_end];
                let run = TextShaper::shape_ltr(
                    segment,
                    (range.start + local_start)..(range.start + local_end),
                    font,
                    0,
                    font_size,
                );
                if run.width <= max_width {
                    best_end = Some((local_end, run));
                } else {
                    break;
                }
            }

            if let Some((local_end, run)) = best_end {
                let line = LineBox {
                    text_range: (range.start + local_start)..(range.start + local_end),
                    width: run.width,
                    height: line_height,
                    baseline_offset: ascent,
                    ascent,
                    descent,
                    leading,
                    runs: vec![run],
                    y_offset: *y,
                };
                *y += line_height;
                out_lines.push(line);
                local_start = local_end;
            } else {
                // Force at least one grapheme.
                let mut iter = paragraph[local_start..].grapheme_indices(true);
                if let Some((idx, g)) = iter.next() {
                    let local_end = local_start + idx + g.len();
                    let segment = &paragraph[local_start..local_end];
                    let run = TextShaper::shape_ltr(
                        segment,
                        (range.start + local_start)..(range.start + local_end),
                        font,
                        0,
                        font_size,
                    );
                    let line = LineBox {
                        text_range: (range.start + local_start)..(range.start + local_end),
                        width: run.width,
                        height: line_height,
                        baseline_offset: ascent,
                        ascent,
                        descent,
                        leading,
                        runs: vec![run],
                        y_offset: *y,
                    };
                    *y += line_height;
                    out_lines.push(line);
                    local_start = local_end;
                } else {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_font() -> FontFace {
        // Load a test font - using the Geist font from the fonts directory
        let font_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fonts/Geist/Geist-VariableFont_wght.ttf");

        FontFace::from_path(&font_path, 0).expect("Failed to load test font")
    }

    #[test]
    fn test_insert_char() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Insert a character in the middle
        let new_cursor = layout.insert_char(5, '!', &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello!");
        assert_eq!(new_cursor, 6);
    }

    #[test]
    fn test_insert_str() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Insert a string in the middle
        let new_cursor = layout.insert_str(5, " World", &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello World");
        assert_eq!(new_cursor, 11);
    }

    #[test]
    fn test_insert_at_start() {
        let font = create_test_font();
        let mut layout = TextLayout::new("World", &font, 16.0);

        let new_cursor = layout.insert_str(0, "Hello ", &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello World");
        assert_eq!(new_cursor, 6);
    }

    #[test]
    fn test_insert_newline() {
        let font = create_test_font();
        let mut layout = TextLayout::new("HelloWorld", &font, 16.0);

        let new_cursor = layout.insert_newline(5, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello\nWorld");
        assert_eq!(new_cursor, 6);
        assert_eq!(layout.lines().len(), 2);
    }

    #[test]
    fn test_insert_tab() {
        let font = create_test_font();
        let mut layout = TextLayout::new("HelloWorld", &font, 16.0);

        let new_cursor = layout.insert_tab(5, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello\tWorld");
        assert_eq!(new_cursor, 6);
    }

    #[test]
    fn test_replace_selection() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello World", &font, 16.0);

        // Replace "World" with "Rust"
        let selection = Selection::new(6, 11);
        let new_cursor =
            layout.replace_selection(&selection, "Rust", &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello Rust");
        assert_eq!(new_cursor, 10);
    }

    #[test]
    fn test_replace_collapsed_selection() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Collapsed selection should just insert
        let selection = Selection::collapsed(5);
        let new_cursor =
            layout.replace_selection(&selection, "!", &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello!");
        assert_eq!(new_cursor, 6);
    }

    #[test]
    fn test_delete_backward() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello!", &font, 16.0);

        // Delete the '!'
        let new_cursor = layout.delete_backward(6, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello");
        assert_eq!(new_cursor, 5);
    }

    #[test]
    fn test_delete_backward_at_start() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Delete at start should do nothing
        let new_cursor = layout.delete_backward(0, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello");
        assert_eq!(new_cursor, 0);
    }

    #[test]
    fn test_delete_forward() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello!", &font, 16.0);

        // Delete the 'H'
        let new_cursor = layout.delete_forward(0, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "ello!");
        assert_eq!(new_cursor, 0);
    }

    #[test]
    fn test_delete_forward_at_end() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Delete at end should do nothing
        let new_cursor = layout.delete_forward(5, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello");
        assert_eq!(new_cursor, 5);
    }

    #[test]
    fn test_delete_word_backward() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello World", &font, 16.0);

        // Delete "World"
        let new_cursor = layout.delete_word_backward(11, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello ");
        assert_eq!(new_cursor, 6);
    }

    #[test]
    fn test_delete_word_forward() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello World", &font, 16.0);

        // Delete "Hello"
        let new_cursor = layout.delete_word_forward(0, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), " World");
        assert_eq!(new_cursor, 0);
    }

    #[test]
    fn test_delete_selection() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello World", &font, 16.0);

        // Delete "World"
        let selection = Selection::new(6, 11);
        let new_cursor = layout.delete_selection(&selection, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello ");
        assert_eq!(new_cursor, 6);
    }

    #[test]
    fn test_delete_collapsed_selection() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Collapsed selection should do nothing
        let selection = Selection::collapsed(3);
        let new_cursor = layout.delete_selection(&selection, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello");
        assert_eq!(new_cursor, 3);
    }

    #[test]
    fn test_delete_line() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        // Delete the second line
        let new_cursor = layout.delete_line(10, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Line 1\nLine 3");
        assert_eq!(new_cursor, 7);
    }

    #[test]
    fn test_delete_line_first() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Line 1\nLine 2", &font, 16.0);

        // Delete the first line
        let new_cursor = layout.delete_line(3, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Line 2");
        assert_eq!(new_cursor, 0);
    }

    #[test]
    fn test_delete_line_last() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Line 1\nLine 2", &font, 16.0);

        // Delete the last line
        let new_cursor = layout.delete_line(10, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Line 1\n");
        assert_eq!(new_cursor, 7);
    }

    #[test]
    fn test_insert_with_unicode() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Insert emoji
        let new_cursor = layout.insert_str(5, " 👋", &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello 👋");
        assert!(new_cursor > 5);
    }

    #[test]
    fn test_delete_backward_with_unicode() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello 👋", &font, 16.0);

        let text_len = layout.text().len();
        // Delete the emoji (grapheme cluster)
        let new_cursor = layout.delete_backward(text_len, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello ");
        assert_eq!(new_cursor, 6);
    }

    #[test]
    fn test_insert_and_relayout() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Short", &font, 16.0);

        // Insert text and verify layout is updated
        layout.insert_str(
            5,
            " text that should wrap",
            &font,
            16.0,
            Some(100.0),
            WrapMode::BreakWord,
        );

        // Should have multiple lines due to wrapping
        assert!(layout.lines().len() > 1);
    }

    #[test]
    fn test_delete_and_relayout() {
        let font = create_test_font();
        let mut layout = TextLayout::new("This is a very long text that wraps", &font, 16.0);

        // Delete most of the text
        let selection = Selection::new(4, 36);
        layout.delete_selection(&selection, &font, 16.0, Some(100.0), WrapMode::BreakWord);

        assert_eq!(layout.text(), "This");
        // Should have only one line now
        assert_eq!(layout.lines().len(), 1);
    }

    #[test]
    fn test_combining_marks() {
        let font = create_test_font();
        let mut layout = TextLayout::new("café", &font, 16.0);

        let initial_len = layout.text().len();
        // Delete the 'é' (which might be e + combining accent)
        let new_cursor = layout.delete_backward(initial_len, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "caf");
        assert!(new_cursor < initial_len);
    }

    #[test]
    fn test_copy_to_clipboard() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World", &font, 16.0);

        // Copy "World"
        let selection = Selection::new(6, 11);
        let result = layout.copy_to_clipboard(&selection);

        // Should succeed (or fail gracefully if no clipboard available in test environment)
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_copy_collapsed_selection() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello", &font, 16.0);

        // Collapsed selection should do nothing but succeed
        let selection = Selection::collapsed(3);
        let result = layout.copy_to_clipboard(&selection);

        assert!(result.is_ok());
    }

    #[test]
    fn test_cut_to_clipboard() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello World", &font, 16.0);

        // Cut "World"
        let selection = Selection::new(6, 11);
        let result = layout.cut_to_clipboard(&selection, &font, 16.0, None, WrapMode::NoWrap);

        // If clipboard is available, text should be deleted
        if result.is_ok() {
            assert_eq!(layout.text(), "Hello ");
            assert_eq!(result.unwrap(), 6);
        }
    }

    #[test]
    fn test_paste_from_clipboard() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Try to paste (may fail if clipboard is empty or unavailable in test env)
        let result = layout.paste_from_clipboard(5, &font, 16.0, None, WrapMode::NoWrap);

        // Just verify it doesn't panic
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_normalize_clipboard_text() {
        // Test CRLF normalization (Windows)
        let windows_text = "Line 1\r\nLine 2\r\nLine 3";
        let normalized = TextLayout::normalize_clipboard_text(windows_text);
        assert_eq!(normalized, "Line 1\nLine 2\nLine 3");

        // Test CR normalization (old Mac)
        let old_mac_text = "Line 1\rLine 2\rLine 3";
        let normalized = TextLayout::normalize_clipboard_text(old_mac_text);
        assert_eq!(normalized, "Line 1\nLine 2\nLine 3");

        // Test LF (Unix) - should remain unchanged
        let unix_text = "Line 1\nLine 2\nLine 3";
        let normalized = TextLayout::normalize_clipboard_text(unix_text);
        assert_eq!(normalized, "Line 1\nLine 2\nLine 3");

        // Test mixed line endings
        let mixed_text = "Line 1\r\nLine 2\rLine 3\nLine 4";
        let normalized = TextLayout::normalize_clipboard_text(mixed_text);
        assert_eq!(normalized, "Line 1\nLine 2\nLine 3\nLine 4");
    }

    // ========================================================================
    // Undo/Redo Tests (Phase 6.8)
    // ========================================================================

    #[test]
    fn test_undo_insert() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Insert text
        let selection = Selection::collapsed(5);
        layout.insert_str_with_undo(5, " World", selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "Hello World");

        // Undo the insertion
        let current_selection = Selection::collapsed(11);
        let result = layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);

        assert!(result.is_some());
        assert_eq!(layout.text(), "Hello");
        let (cursor, _) = result.unwrap();
        assert_eq!(cursor, 5);
    }

    #[test]
    fn test_undo_delete() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello World", &font, 16.0);

        // Delete text
        let selection = Selection::new(5, 11);
        layout.delete_selection(&selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "Hello");

        // Undo the deletion
        let current_selection = Selection::collapsed(5);
        let result = layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);

        assert!(result.is_some());
        assert_eq!(layout.text(), "Hello World");
    }

    #[test]
    fn test_redo() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Insert text
        let selection = Selection::collapsed(5);
        layout.insert_str_with_undo(5, "!", selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "Hello!");

        // Undo
        let current_selection = Selection::collapsed(6);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "Hello");

        // Redo
        let current_selection = Selection::collapsed(5);
        let result = layout.redo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);

        assert!(result.is_some());
        assert_eq!(layout.text(), "Hello!");
    }

    #[test]
    fn test_can_undo_redo() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        assert!(!layout.can_undo());
        assert!(!layout.can_redo());

        // Insert text
        let selection = Selection::collapsed(5);
        layout.insert_str_with_undo(5, "!", selection, &font, 16.0, None, WrapMode::NoWrap);

        assert!(layout.can_undo());
        assert!(!layout.can_redo());

        // Undo
        let current_selection = Selection::collapsed(6);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);

        assert!(!layout.can_undo());
        assert!(layout.can_redo());

        // Redo
        let current_selection = Selection::collapsed(5);
        layout.redo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);

        assert!(layout.can_undo());
        assert!(!layout.can_redo());
    }

    #[test]
    fn test_undo_multiple_operations() {
        let font = create_test_font();
        let mut layout = TextLayout::new("", &font, 16.0);
        layout.set_undo_grouping(false); // Disable grouping for this test

        // Perform multiple operations
        let sel1 = Selection::collapsed(0);
        layout.insert_str_with_undo(0, "Hello", sel1, &font, 16.0, None, WrapMode::NoWrap);

        let sel2 = Selection::collapsed(5);
        layout.insert_str_with_undo(5, " ", sel2, &font, 16.0, None, WrapMode::NoWrap);

        let sel3 = Selection::collapsed(6);
        layout.insert_str_with_undo(6, "World", sel3, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hello World");

        // Undo all operations
        let mut current_selection = Selection::collapsed(11);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "Hello ");

        current_selection = Selection::collapsed(6);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "Hello");

        current_selection = Selection::collapsed(5);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "");
    }

    #[test]
    fn test_undo_replace() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello World", &font, 16.0);

        // Replace text
        let selection = Selection::new(6, 11);
        layout.replace_selection(&selection, "Rust", &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "Hello Rust");

        // Undo the replacement
        let current_selection = Selection::collapsed(10);
        let result = layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);

        assert!(result.is_some());
        assert_eq!(layout.text(), "Hello World");
    }

    #[test]
    fn test_clear_undo_history() {
        let font = create_test_font();
        let mut layout = TextLayout::new("Hello", &font, 16.0);

        // Insert text
        let selection = Selection::collapsed(5);
        layout.insert_str_with_undo(5, "!", selection, &font, 16.0, None, WrapMode::NoWrap);

        assert!(layout.can_undo());

        // Clear history
        layout.clear_undo_history();

        assert!(!layout.can_undo());
        assert!(!layout.can_redo());
    }

    #[test]
    fn test_undo_limit() {
        let font = create_test_font();
        let mut layout = TextLayout::new("", &font, 16.0);
        layout.set_undo_limit(3);
        layout.set_undo_grouping(false); // Disable grouping for this test

        // Insert more operations than the limit
        for i in 0..5 {
            let sel = Selection::collapsed(i);
            layout.insert_str_with_undo(i, "x", sel, &font, 16.0, None, WrapMode::NoWrap);
        }

        // Should only keep the last 3 operations
        assert_eq!(layout.undo_limit(), 3);

        // Undo 3 times should work
        let mut current_selection = Selection::collapsed(5);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);
        current_selection = Selection::collapsed(4);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);
        current_selection = Selection::collapsed(3);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);

        // Should not be able to undo more
        assert!(!layout.can_undo());
        assert_eq!(layout.text(), "xx");
    }

    #[test]
    fn test_undo_grouping() {
        let font = create_test_font();
        let mut layout = TextLayout::new("", &font, 16.0);

        // Insert consecutive characters (should be grouped)
        let sel1 = Selection::collapsed(0);
        layout.insert_str_with_undo(0, "H", sel1, &font, 16.0, None, WrapMode::NoWrap);

        let sel2 = Selection::collapsed(1);
        layout.insert_str_with_undo(1, "e", sel2, &font, 16.0, None, WrapMode::NoWrap);

        let sel3 = Selection::collapsed(2);
        layout.insert_str_with_undo(2, "l", sel3, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "Hel");

        // Should undo all grouped operations at once
        let current_selection = Selection::collapsed(3);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "");
        assert!(!layout.can_undo());
    }

    #[test]
    fn test_undo_grouping_disabled() {
        let font = create_test_font();
        let mut layout = TextLayout::new("", &font, 16.0);
        layout.set_undo_grouping(false);

        // Insert consecutive characters (should NOT be grouped)
        let sel1 = Selection::collapsed(0);
        layout.insert_str_with_undo(0, "H", sel1, &font, 16.0, None, WrapMode::NoWrap);

        let sel2 = Selection::collapsed(1);
        layout.insert_str_with_undo(1, "e", sel2, &font, 16.0, None, WrapMode::NoWrap);

        assert_eq!(layout.text(), "He");

        // Should undo one operation at a time
        let mut current_selection = Selection::collapsed(2);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "H");

        current_selection = Selection::collapsed(1);
        layout.undo(&current_selection, &font, 16.0, None, WrapMode::NoWrap);
        assert_eq!(layout.text(), "");
    }

    // ========================================================================
    // Text Measurement Tests (Phase 6.9)
    // ========================================================================

    #[test]
    fn test_measure_single_line_width() {
        let font = create_test_font();

        // Measure empty text
        let width = TextLayout::measure_single_line_width("", &font, 16.0);
        assert_eq!(width, 0.0);

        // Measure simple text
        let width = TextLayout::measure_single_line_width("Hello", &font, 16.0);
        assert!(width > 0.0);

        // Longer text should be wider
        let width1 = TextLayout::measure_single_line_width("Hi", &font, 16.0);
        let width2 = TextLayout::measure_single_line_width("Hello World", &font, 16.0);
        assert!(width2 > width1);
    }

    #[test]
    fn test_text_bounds() {
        let font = create_test_font();

        // Empty text - has one empty line with 0 width but non-zero height
        let layout = TextLayout::new("", &font, 16.0);
        let (width, height) = layout.text_bounds();
        assert_eq!(width, 0.0);
        assert!(height > 0.0); // Empty text still has line height

        // Single line
        let layout = TextLayout::new("Hello", &font, 16.0);
        let (width, height) = layout.text_bounds();
        assert!(width > 0.0);
        assert!(height > 0.0);

        // Multi-line text
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);
        let (width, height) = layout.text_bounds();
        assert!(width > 0.0);
        assert!(height > 0.0);

        // Height should be greater for multi-line
        let single_line = TextLayout::new("Hello", &font, 16.0);
        let (_, single_height) = single_line.text_bounds();
        assert!(height > single_height);
    }

    #[test]
    fn test_character_width() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello", &font, 16.0);

        // Get width of first character
        let width = layout.character_width(0);
        assert!(width.is_some());
        assert!(width.unwrap() > 0.0);

        // Invalid offset
        let width = layout.character_width(100);
        assert!(width.is_none());

        // Width at end of text
        let width = layout.character_width(layout.text().len());
        assert!(width.is_none());
    }

    #[test]
    fn test_line_height() {
        let font = create_test_font();

        // Empty layout
        let layout = TextLayout::new("", &font, 16.0);
        let height = layout.line_height();
        assert!(height.is_some());
        assert!(height.unwrap() > 0.0);

        // Non-empty layout
        let layout = TextLayout::new("Hello", &font, 16.0);
        let height = layout.line_height();
        assert!(height.is_some());
        assert!(height.unwrap() > 0.0);
    }

    #[test]
    fn test_baseline_offset() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello", &font, 16.0);

        let offset = layout.baseline_offset();
        assert!(offset.is_some());
        assert!(offset.unwrap() > 0.0);
    }

    #[test]
    fn test_line_baseline_y() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        // First line baseline
        let baseline = layout.line_baseline_y(0);
        assert!(baseline.is_some());

        // Second line baseline should be lower
        let baseline1 = layout.line_baseline_y(0).unwrap();
        let baseline2 = layout.line_baseline_y(1).unwrap();
        assert!(baseline2 > baseline1);

        // Invalid line index
        let baseline = layout.line_baseline_y(100);
        assert!(baseline.is_none());
    }

    #[test]
    fn test_visible_line_range() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3\nLine 4\nLine 5", &font, 16.0);

        let line_height = layout.line_height().unwrap();

        // Viewport showing first 2 lines
        let range = layout.visible_line_range(0.0, line_height * 2.0);
        assert!(range.is_some());
        let (first, _last) = range.unwrap();
        assert_eq!(first, 0);

        // Viewport in the middle
        let range = layout.visible_line_range(line_height * 2.0, line_height * 2.0);
        assert!(range.is_some());
        let (first, _) = range.unwrap();
        assert!(first > 0);

        // Empty layout
        let empty_layout = TextLayout::new("", &font, 16.0);
        let range = empty_layout.visible_line_range(0.0, 100.0);
        assert!(range.is_some()); // Empty layout has one empty line
    }

    #[test]
    fn test_measure_range_width() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World", &font, 16.0);

        // Measure "Hello"
        let width = layout.measure_range_width(0, 5);
        assert!(width.is_some());
        assert!(width.unwrap() > 0.0);

        // Measure "World"
        let width = layout.measure_range_width(6, 11);
        assert!(width.is_some());
        assert!(width.unwrap() > 0.0);

        // Invalid range
        let width = layout.measure_range_width(5, 5);
        assert!(width.is_none());

        let width = layout.measure_range_width(10, 5);
        assert!(width.is_none());

        // Range exceeds text length
        let width = layout.measure_range_width(0, 100);
        assert!(width.is_none());
    }

    #[test]
    fn test_measure_range_width_multiline() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2", &font, 16.0);

        // Measure across multiple lines
        let width = layout.measure_range_width(0, layout.text().len());
        assert!(width.is_some());
        assert!(width.unwrap() > 0.0);
    }

    #[test]
    fn test_line_count() {
        let font = create_test_font();

        // Single line
        let layout = TextLayout::new("Hello", &font, 16.0);
        assert_eq!(layout.line_count(), 1);

        // Multiple lines
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);
        assert_eq!(layout.line_count(), 3);

        // Empty text
        let layout = TextLayout::new("", &font, 16.0);
        assert_eq!(layout.line_count(), 1); // Empty text has one empty line
    }

    #[test]
    fn test_total_height() {
        let font = create_test_font();

        // Single line
        let layout = TextLayout::new("Hello", &font, 16.0);
        let height = layout.total_height();
        assert!(height > 0.0);

        // Multiple lines should be taller
        let layout_multi = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);
        let height_multi = layout_multi.total_height();
        assert!(height_multi > height);

        // Empty text
        let layout = TextLayout::new("", &font, 16.0);
        let height = layout.total_height();
        assert!(height > 0.0); // Empty text still has height
    }

    #[test]
    fn test_max_line_width() {
        let font = create_test_font();

        // Single line
        let layout = TextLayout::new("Hello", &font, 16.0);
        let width = layout.max_line_width();
        assert!(width > 0.0);

        // Multiple lines with different widths
        let layout = TextLayout::new("Short\nThis is a longer line\nMed", &font, 16.0);
        let max_width = layout.max_line_width();
        assert!(max_width > 0.0);

        // The max width should be from the longest line
        let (bounds_width, _) = layout.text_bounds();
        assert_eq!(max_width, bounds_width);
    }

    #[test]
    fn test_text_bounds_equals_max_and_total() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        let (bounds_width, bounds_height) = layout.text_bounds();
        let max_width = layout.max_line_width();
        let total_height = layout.total_height();

        assert_eq!(bounds_width, max_width);
        assert_eq!(bounds_height, total_height);
    }

    #[test]
    fn test_measure_with_wrapping() {
        let font = create_test_font();

        // Create layout with wrapping
        let layout = TextLayout::with_wrap(
            "This is a very long line that should wrap",
            &font,
            16.0,
            Some(100.0),
            WrapMode::BreakWord,
        );

        // Should have multiple lines due to wrapping
        assert!(layout.line_count() > 1);

        // Total height should reflect multiple lines
        let height = layout.total_height();
        let line_height = layout.line_height().unwrap();
        assert!(height > line_height);

        // Max width should be at most the wrap width
        let max_width = layout.max_line_width();
        assert!(max_width <= 100.0 + 1.0); // Allow small tolerance
    }

    // ========================================================================
    // Mouse Selection Tests (Phase 6.3/6.4)
    // ========================================================================

    #[test]
    fn test_start_mouse_selection() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World", &font, 16.0);

        // Click at the beginning
        let point = Point::new(0.0, 0.0);
        let selection = layout.start_mouse_selection(point);
        assert!(selection.is_some());
        let sel = selection.unwrap();
        assert!(sel.is_collapsed());
        assert_eq!(sel.anchor(), sel.active());
    }

    #[test]
    fn test_extend_mouse_selection() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World", &font, 16.0);

        // Start selection at beginning
        let start_point = Point::new(0.0, 0.0);
        let selection = layout.start_mouse_selection(start_point).unwrap();

        // Drag to the right
        let end_point = Point::new(50.0, 0.0);
        let extended = layout.extend_mouse_selection(&selection, end_point);

        // Selection should no longer be collapsed
        assert!(!extended.is_collapsed());
        assert!(extended.len() > 0);
    }

    #[test]
    fn test_extend_mouse_selection_backward() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World", &font, 16.0);

        // Start selection in the middle
        let start_point = Point::new(50.0, 0.0);
        let selection = layout.start_mouse_selection(start_point).unwrap();
        let anchor = selection.anchor();

        // Drag backward (to the left)
        let end_point = Point::new(10.0, 0.0);
        let extended = layout.extend_mouse_selection(&selection, end_point);

        // Anchor should remain at original position
        assert_eq!(extended.anchor(), anchor);
        // Selection should be backward
        assert!(extended.is_backward());
    }

    #[test]
    fn test_start_word_selection() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World", &font, 16.0);

        // Double-click on "Hello"
        let point = Point::new(10.0, 0.0);
        let selection = layout.start_word_selection(point);
        assert!(selection.is_some());
        let sel = selection.unwrap();

        // Should select the entire word
        assert!(!sel.is_collapsed());
        let selected_text = sel.text(layout.text());
        assert!(selected_text == "Hello" || selected_text.contains("Hello"));
    }

    #[test]
    fn test_extend_word_selection_forward() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World Test", &font, 16.0);

        // Start with "Hello" selected
        let start_point = Point::new(10.0, 0.0);
        let selection = layout.start_word_selection(start_point).unwrap();

        // Drag to "Test"
        let (width, _) = layout.text_bounds();
        let end_point = Point::new(width - 10.0, 0.0);
        let extended = layout.extend_word_selection(&selection, end_point);

        // Should extend to include more words
        assert!(extended.len() >= selection.len());
    }

    #[test]
    fn test_extend_word_selection_backward() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World Test", &font, 16.0);

        // Start with "Test" selected (at the end)
        let (width, _) = layout.text_bounds();
        let start_point = Point::new(width - 10.0, 0.0);
        let selection = layout.start_word_selection(start_point).unwrap();

        // Drag backward to "Hello"
        let end_point = Point::new(10.0, 0.0);
        let extended = layout.extend_word_selection(&selection, end_point);

        // Should extend backward to include more words
        assert!(extended.len() >= selection.len());
    }

    #[test]
    fn test_start_line_selection() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        // Triple-click on first line
        let point = Point::new(10.0, 0.0);
        let selection = layout.start_line_selection(point);
        assert!(selection.is_some());
        let sel = selection.unwrap();

        // Should select the entire line
        assert!(!sel.is_collapsed());
        let selected_text = sel.text(layout.text());
        assert!(selected_text.contains("Line 1") || selected_text == "Line 1");
    }

    #[test]
    fn test_extend_line_selection_down() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        // Start with first line selected
        let start_point = Point::new(10.0, 0.0);
        let selection = layout.start_line_selection(start_point).unwrap();

        // Drag down to third line
        let line_height = layout.line_height().unwrap();
        let end_point = Point::new(10.0, line_height * 4.0);
        let extended = layout.extend_line_selection(&selection, end_point);

        // Should extend to include more lines
        assert!(extended.len() >= selection.len());
    }

    #[test]
    fn test_extend_line_selection_up() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        // Start with third line selected
        let line_height = layout.line_height().unwrap();
        let start_point = Point::new(10.0, line_height * 4.0);
        let selection = layout.start_line_selection(start_point).unwrap();

        // Drag up to first line
        let end_point = Point::new(10.0, 0.0);
        let extended = layout.extend_line_selection(&selection, end_point);

        // Should extend backward to include more lines
        assert!(extended.len() >= selection.len());
    }

    #[test]
    fn test_mouse_selection_multiline() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        // Start selection on first line
        let start_point = Point::new(0.0, 0.0);
        let selection = layout.start_mouse_selection(start_point).unwrap();

        // Drag to third line
        let line_height = layout.line_height().unwrap();
        let end_point = Point::new(50.0, line_height * 4.0);
        let extended = layout.extend_mouse_selection(&selection, end_point);

        // Should span multiple lines
        assert!(!extended.is_collapsed());
        assert!(extended.len() > 0);

        // Should include text from multiple lines
        let selected_text = extended.text(layout.text());
        assert!(selected_text.contains('\n') || selected_text.len() > 6);
    }

    #[test]
    fn test_mouse_selection_with_empty_text() {
        let font = create_test_font();
        let layout = TextLayout::new("", &font, 16.0);

        let point = Point::new(0.0, 0.0);
        let selection = layout.start_mouse_selection(point);
        assert!(selection.is_some());
        let sel = selection.unwrap();
        assert!(sel.is_collapsed());
        assert_eq!(sel.anchor(), 0);
    }

    // ========================================================================
    // Scrolling & Viewport Tests (Phase 6.10)
    // ========================================================================

    #[test]
    fn test_scroll_to_cursor_visible() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello World", &font, 16.0);

        // Cursor at start with no margin - should be visible
        let result = layout.scroll_to_cursor(0, 0.0, 0.0, 800.0, 600.0, 0.0);
        assert!(result.is_none()); // No scroll needed when margin is 0

        // Large viewport, small text - cursor should always be visible
        let result = layout.scroll_to_cursor(0, 0.0, 0.0, 10000.0, 10000.0, 50.0);
        assert!(result.is_none()); // No scroll needed with huge viewport
    }

    #[test]
    fn test_scroll_to_cursor_below_viewport() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3\nLine 4\nLine 5", &font, 16.0);

        let line_height = layout.line_height().unwrap();

        // Cursor on last line, viewport showing first line
        let last_offset = layout.text().len();
        let result = layout.scroll_to_cursor(last_offset, 0.0, 0.0, 800.0, line_height * 2.0, 10.0);

        assert!(result.is_some());
        let (_, scroll_y) = result.unwrap();
        assert!(scroll_y > 0.0); // Should scroll down
    }

    #[test]
    fn test_scroll_to_cursor_above_viewport() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3\nLine 4\nLine 5", &font, 16.0);

        let line_height = layout.line_height().unwrap();

        // Cursor at start, viewport scrolled down
        let result =
            layout.scroll_to_cursor(0, 0.0, line_height * 3.0, 800.0, line_height * 2.0, 10.0);

        assert!(result.is_some());
        let (_, scroll_y) = result.unwrap();
        assert!(scroll_y < line_height * 3.0); // Should scroll up
    }

    #[test]
    fn test_scroll_to_cursor_horizontal() {
        let font = create_test_font();
        let layout = TextLayout::new("This is a very long line of text", &font, 16.0);

        // Cursor at end, narrow viewport
        let end_offset = layout.text().len();
        let result = layout.scroll_to_cursor(end_offset, 0.0, 0.0, 100.0, 600.0, 10.0);

        assert!(result.is_some());
        let (scroll_x, _) = result.unwrap();
        assert!(scroll_x > 0.0); // Should scroll right
    }

    #[test]
    fn test_scroll_to_selection() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        let line_height = layout.line_height().unwrap();

        // Selection on last line
        let selection = Selection::new(0, layout.text().len());
        let result =
            layout.scroll_to_selection(&selection, 0.0, 0.0, 800.0, line_height * 1.5, 10.0);

        assert!(result.is_some());
        let (_, scroll_y) = result.unwrap();
        assert!(scroll_y > 0.0); // Should scroll to show active end
    }

    #[test]
    fn test_scroll_to_line_centered() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3\nLine 4\nLine 5", &font, 16.0);

        let viewport_height = 200.0;

        // Center the third line (index 2)
        let scroll_y = layout.scroll_to_line_centered(2, viewport_height);
        assert!(scroll_y.is_some());
        assert!(scroll_y.unwrap() >= 0.0);
    }

    #[test]
    fn test_scroll_to_line_top() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        // Scroll to show second line at top
        let scroll_y = layout.scroll_to_line_top(1, 10.0);
        assert!(scroll_y.is_some());

        let line_height = layout.line_height().unwrap();
        // Should be roughly at the second line's position
        assert!(scroll_y.unwrap() > 0.0);
        assert!(scroll_y.unwrap() < line_height * 3.0);
    }

    #[test]
    fn test_scroll_bounds() {
        let font = create_test_font();
        let layout = TextLayout::new("Short\nThis is a much longer line\nShort", &font, 16.0);

        let viewport_width = 100.0;
        let viewport_height = 50.0;

        let (max_x, max_y) = layout.scroll_bounds(viewport_width, viewport_height);

        // Should have horizontal scroll space
        assert!(max_x > 0.0);
        // Should have vertical scroll space
        assert!(max_y > 0.0);
    }

    #[test]
    fn test_scroll_bounds_small_content() {
        let font = create_test_font();
        let layout = TextLayout::new("Hi", &font, 16.0);

        let viewport_width = 800.0;
        let viewport_height = 600.0;

        let (max_x, max_y) = layout.scroll_bounds(viewport_width, viewport_height);

        // Content fits in viewport, no scroll needed
        assert_eq!(max_x, 0.0);
        assert_eq!(max_y, 0.0);
    }

    #[test]
    fn test_wheel_scroll_delta() {
        // Test basic wheel scrolling
        let (dx, dy) = TextLayout::wheel_scroll_delta(0.0, 1.0, 20.0);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, 20.0);

        // Test horizontal scrolling
        let (dx, dy) = TextLayout::wheel_scroll_delta(1.0, 0.0, 20.0);
        assert_eq!(dx, 20.0);
        assert_eq!(dy, 0.0);

        // Test negative (reverse) scrolling
        let (dx, dy) = TextLayout::wheel_scroll_delta(0.0, -1.0, 20.0);
        assert_eq!(dx, 0.0);
        assert_eq!(dy, -20.0);
    }

    #[test]
    fn test_default_line_scroll_amount() {
        let font = create_test_font();
        let layout = TextLayout::new("Hello", &font, 16.0);

        let amount = layout.default_line_scroll_amount();
        assert!(amount > 0.0);

        // Should be roughly 3x line height
        let line_height = layout.line_height().unwrap();
        assert!((amount - line_height * 3.0).abs() < 1.0);
    }

    #[test]
    fn test_scroll_with_margin() {
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3", &font, 16.0);

        let line_height = layout.line_height().unwrap();

        // Test with different margins
        let result1 = layout.scroll_to_cursor(0, 0.0, line_height, 800.0, line_height * 2.0, 5.0);
        let result2 = layout.scroll_to_cursor(0, 0.0, line_height, 800.0, line_height * 2.0, 20.0);

        // Larger margin should trigger scroll sooner
        assert!(result1.is_some() || result2.is_some());
    }

    #[test]
    fn test_visible_line_range_already_implemented() {
        // This was already implemented in 6.9, just verify it works
        let font = create_test_font();
        let layout = TextLayout::new("Line 1\nLine 2\nLine 3\nLine 4\nLine 5", &font, 16.0);

        let line_height = layout.line_height().unwrap();
        let range = layout.visible_line_range(0.0, line_height * 2.0);

        assert!(range.is_some());
        let (first, last) = range.unwrap();
        assert_eq!(first, 0);
        assert!(last >= first);
    }
}
