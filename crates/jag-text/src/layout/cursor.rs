use unicode_segmentation::UnicodeSegmentation;

/// Cursor affinity determines which side of a character the cursor is on.
/// This is important for BiDi text and when a cursor position is ambiguous
/// (e.g., at line boundaries).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorAffinity {
    /// Cursor is on the left/upstream side of the character.
    Upstream,
    /// Cursor is on the right/downstream side of the character.
    Downstream,
}

impl Default for CursorAffinity {
    fn default() -> Self {
        Self::Downstream
    }
}

/// Represents a cursor position in text with support for grapheme boundaries
/// and cursor affinity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorPosition {
    /// Byte offset in the source text.
    /// This should always be at a grapheme cluster boundary.
    pub byte_offset: usize,
    /// Cursor affinity (left/right of character).
    pub affinity: CursorAffinity,
}

impl CursorPosition {
    /// Create a new cursor position at the given byte offset.
    pub fn new(byte_offset: usize) -> Self {
        Self {
            byte_offset,
            affinity: CursorAffinity::default(),
        }
    }

    /// Create a new cursor position with explicit affinity.
    pub fn with_affinity(byte_offset: usize, affinity: CursorAffinity) -> Self {
        Self {
            byte_offset,
            affinity,
        }
    }

    /// Ensure the cursor is at a valid grapheme boundary.
    /// If not, move it to the nearest boundary.
    pub fn snap_to_grapheme_boundary(mut self, text: &str) -> Self {
        if self.byte_offset > text.len() {
            self.byte_offset = text.len();
            return self;
        }

        // Check if we're already at a boundary
        if self.byte_offset == 0 || self.byte_offset == text.len() {
            return self;
        }

        // Find the nearest grapheme boundary
        let mut last_boundary = 0;
        for (idx, _) in text.grapheme_indices(true) {
            if idx == self.byte_offset {
                // Already at a boundary
                return self;
            }
            if idx > self.byte_offset {
                // We're between boundaries, snap to the previous one
                self.byte_offset = last_boundary;
                return self;
            }
            last_boundary = idx;
        }

        // If we get here, snap to the last boundary (end of text)
        self.byte_offset = text.len();
        self
    }
}

/// Manages cursor state including position, visibility, and animation.
#[derive(Debug, Clone)]
pub struct Cursor {
    /// Current cursor position.
    position: CursorPosition,
    /// Whether the cursor is currently visible.
    visible: bool,
    /// Time accumulator for blink animation (in seconds).
    blink_time: f32,
    /// Blink interval (in seconds).
    blink_interval: f32,
}

impl Cursor {
    /// Create a new cursor at the start of the text.
    pub fn new() -> Self {
        Self {
            position: CursorPosition::new(0),
            visible: true,
            blink_time: 0.0,
            blink_interval: 0.5,
        }
    }

    /// Create a cursor at a specific position.
    pub fn at_position(position: CursorPosition) -> Self {
        Self {
            position,
            visible: true,
            blink_time: 0.0,
            blink_interval: 0.5,
        }
    }

    /// Get the current cursor position.
    pub fn position(&self) -> CursorPosition {
        self.position
    }

    /// Set the cursor position.
    pub fn set_position(&mut self, position: CursorPosition) {
        self.position = position;
        // Reset blink animation when cursor moves
        self.blink_time = 0.0;
        self.visible = true;
    }

    /// Get the byte offset of the cursor.
    pub fn byte_offset(&self) -> usize {
        self.position.byte_offset
    }

    /// Set the cursor to a specific byte offset.
    pub fn set_byte_offset(&mut self, offset: usize) {
        self.position.byte_offset = offset;
        // Reset blink animation when cursor moves
        self.blink_time = 0.0;
        self.visible = true;
    }

    /// Get the cursor affinity.
    pub fn affinity(&self) -> CursorAffinity {
        self.position.affinity
    }

    /// Set the cursor affinity.
    pub fn set_affinity(&mut self, affinity: CursorAffinity) {
        self.position.affinity = affinity;
    }

    /// Check if the cursor is currently visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Set cursor visibility.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
        if visible {
            self.blink_time = 0.0;
        }
    }

    /// Toggle cursor visibility.
    pub fn toggle_visibility(&mut self) {
        self.visible = !self.visible;
    }

    /// Update the cursor blink animation.
    ///
    /// # Arguments
    /// * `delta_time` - Time elapsed since last update in seconds.
    pub fn update_blink(&mut self, delta_time: f32) {
        self.blink_time += delta_time;
        if self.blink_time >= self.blink_interval {
            self.blink_time -= self.blink_interval;
            self.visible = !self.visible;
        }
    }

    /// Set the blink interval.
    pub fn set_blink_interval(&mut self, interval: f32) {
        self.blink_interval = interval.max(0.1); // Minimum 0.1s
    }

    /// Get the blink interval.
    pub fn blink_interval(&self) -> f32 {
        self.blink_interval
    }

    /// Reset the blink animation (make cursor visible and reset timer).
    pub fn reset_blink(&mut self) {
        self.blink_time = 0.0;
        self.visible = true;
    }

    /// Ensure the cursor is at a valid grapheme boundary in the given text.
    pub fn snap_to_grapheme_boundary(&mut self, text: &str) {
        self.position = self.position.snap_to_grapheme_boundary(text);
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents the visual rectangle for rendering a cursor.
#[derive(Debug, Clone, Copy)]
pub struct CursorRect {
    /// X position in pixels.
    pub x: f32,
    /// Y position in pixels (top of cursor).
    pub y: f32,
    /// Width in pixels (typically 1-2px).
    pub width: f32,
    /// Height in pixels (typically line height).
    pub height: f32,
}

impl CursorRect {
    /// Create a new cursor rectangle.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cursor_position_new() {
        let pos = CursorPosition::new(5);
        assert_eq!(pos.byte_offset, 5);
        assert_eq!(pos.affinity, CursorAffinity::Downstream);
    }

    #[test]
    fn test_cursor_position_with_affinity() {
        let pos = CursorPosition::with_affinity(10, CursorAffinity::Upstream);
        assert_eq!(pos.byte_offset, 10);
        assert_eq!(pos.affinity, CursorAffinity::Upstream);
    }

    #[test]
    fn test_snap_to_grapheme_boundary() {
        let text = "Hello 世界";

        // At valid boundary (0)
        let pos = CursorPosition::new(0).snap_to_grapheme_boundary(text);
        assert_eq!(pos.byte_offset, 0);

        // At valid boundary (6, after "Hello ")
        let pos = CursorPosition::new(6).snap_to_grapheme_boundary(text);
        assert_eq!(pos.byte_offset, 6);

        // At end
        let pos = CursorPosition::new(text.len()).snap_to_grapheme_boundary(text);
        assert_eq!(pos.byte_offset, text.len());

        // Beyond end
        let pos = CursorPosition::new(100).snap_to_grapheme_boundary(text);
        assert_eq!(pos.byte_offset, text.len());
    }

    #[test]
    fn test_cursor_new() {
        let cursor = Cursor::new();
        assert_eq!(cursor.byte_offset(), 0);
        assert!(cursor.is_visible());
        assert_eq!(cursor.affinity(), CursorAffinity::Downstream);
    }

    #[test]
    fn test_cursor_set_position() {
        let mut cursor = Cursor::new();
        cursor.set_position(CursorPosition::new(10));
        assert_eq!(cursor.byte_offset(), 10);
        assert!(cursor.is_visible()); // Should reset to visible
    }

    #[test]
    fn test_cursor_visibility() {
        let mut cursor = Cursor::new();
        assert!(cursor.is_visible());

        cursor.set_visible(false);
        assert!(!cursor.is_visible());

        cursor.toggle_visibility();
        assert!(cursor.is_visible());
    }

    #[test]
    fn test_cursor_blink() {
        let mut cursor = Cursor::new();
        cursor.set_blink_interval(0.5);

        assert!(cursor.is_visible());

        // Update with half interval - should still be visible
        cursor.update_blink(0.25);
        assert!(cursor.is_visible());

        // Update to complete interval - should toggle
        cursor.update_blink(0.25);
        assert!(!cursor.is_visible());

        // Another interval - should toggle back
        cursor.update_blink(0.5);
        assert!(cursor.is_visible());
    }

    #[test]
    fn test_cursor_rect() {
        let rect = CursorRect::new(10.0, 20.0, 2.0, 16.0);
        assert_eq!(rect.x, 10.0);
        assert_eq!(rect.y, 20.0);
        assert_eq!(rect.width, 2.0);
        assert_eq!(rect.height, 16.0);
    }
}
