use core::ops::Range;

/// Represents a text selection with start and end byte offsets.
///
/// The selection is always stored in logical order (start <= end),
/// but the anchor and active positions track which end the user is
/// actively moving (for extending selections).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// The anchor point where the selection started (byte offset).
    /// This is the fixed end when extending the selection.
    anchor: usize,
    /// The active point where the selection currently ends (byte offset).
    /// This is the moving end when extending the selection.
    active: usize,
}

impl Selection {
    /// Create a new selection from anchor to active position.
    ///
    /// The anchor is where the selection started, and active is where
    /// it currently ends. These may be in any order.
    pub fn new(anchor: usize, active: usize) -> Self {
        Self { anchor, active }
    }

    /// Create a collapsed selection (cursor) at the given position.
    pub fn collapsed(offset: usize) -> Self {
        Self {
            anchor: offset,
            active: offset,
        }
    }

    /// Get the anchor position (where selection started).
    pub fn anchor(&self) -> usize {
        self.anchor
    }

    /// Get the active position (current end of selection).
    pub fn active(&self) -> usize {
        self.active
    }

    /// Get the selection range in logical order (start..end).
    ///
    /// The start is always <= end, regardless of selection direction.
    pub fn range(&self) -> Range<usize> {
        if self.anchor <= self.active {
            self.anchor..self.active
        } else {
            self.active..self.anchor
        }
    }

    /// Get the start of the selection (minimum of anchor and active).
    pub fn start(&self) -> usize {
        self.anchor.min(self.active)
    }

    /// Get the end of the selection (maximum of anchor and active).
    pub fn end(&self) -> usize {
        self.anchor.max(self.active)
    }

    /// Check if the selection is collapsed (no range selected).
    pub fn is_collapsed(&self) -> bool {
        self.anchor == self.active
    }

    /// Get the length of the selection in bytes.
    pub fn len(&self) -> usize {
        if self.anchor > self.active {
            self.anchor - self.active
        } else {
            self.active - self.anchor
        }
    }

    /// Check if the selection is empty (same as is_collapsed).
    pub fn is_empty(&self) -> bool {
        self.is_collapsed()
    }

    /// Check if the selection is forward (anchor <= active).
    pub fn is_forward(&self) -> bool {
        self.anchor <= self.active
    }

    /// Check if the selection is backward (anchor > active).
    pub fn is_backward(&self) -> bool {
        self.anchor > self.active
    }

    /// Extend the selection by moving the active end to a new position.
    ///
    /// The anchor remains fixed, and the active end moves to the new offset.
    pub fn extend_to(&mut self, offset: usize) {
        self.active = offset;
    }

    /// Move the selection (both anchor and active) to a new collapsed position.
    pub fn move_to(&mut self, offset: usize) {
        self.anchor = offset;
        self.active = offset;
    }

    /// Collapse the selection to the start.
    pub fn collapse_to_start(&mut self) {
        let start = self.start();
        self.anchor = start;
        self.active = start;
    }

    /// Collapse the selection to the end.
    pub fn collapse_to_end(&mut self) {
        let end = self.end();
        self.anchor = end;
        self.active = end;
    }

    /// Collapse the selection to the anchor position.
    pub fn collapse_to_anchor(&mut self) {
        self.active = self.anchor;
    }

    /// Collapse the selection to the active position.
    pub fn collapse_to_active(&mut self) {
        self.anchor = self.active;
    }

    /// Check if the selection contains the given byte offset.
    pub fn contains(&self, offset: usize) -> bool {
        let range = self.range();
        range.contains(&offset)
    }

    /// Flip the selection direction (swap anchor and active).
    pub fn flip(&mut self) {
        core::mem::swap(&mut self.anchor, &mut self.active);
    }

    /// Get the selected text from the source string.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        let range = self.range();
        &source[range]
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::collapsed(0)
    }
}

/// A rectangle representing a portion of a selection for rendering.
///
/// Selections may span multiple lines, so they are represented as
/// multiple rectangles (one per line).
#[derive(Debug, Clone, Copy)]
pub struct SelectionRect {
    /// X position in pixels.
    pub x: f32,
    /// Y position in pixels (top of selection).
    pub y: f32,
    /// Width in pixels.
    pub width: f32,
    /// Height in pixels (typically line height).
    pub height: f32,
}

impl SelectionRect {
    /// Create a new selection rectangle.
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
    fn test_selection_new() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.anchor(), 5);
        assert_eq!(sel.active(), 10);
        assert_eq!(sel.range(), 5..10);
    }

    #[test]
    fn test_selection_collapsed() {
        let sel = Selection::collapsed(5);
        assert_eq!(sel.anchor(), 5);
        assert_eq!(sel.active(), 5);
        assert!(sel.is_collapsed());
        assert_eq!(sel.len(), 0);
    }

    #[test]
    fn test_selection_range_forward() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.range(), 5..10);
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 10);
        assert!(sel.is_forward());
        assert!(!sel.is_backward());
    }

    #[test]
    fn test_selection_range_backward() {
        let sel = Selection::new(10, 5);
        assert_eq!(sel.range(), 5..10);
        assert_eq!(sel.start(), 5);
        assert_eq!(sel.end(), 10);
        assert!(!sel.is_forward());
        assert!(sel.is_backward());
    }

    #[test]
    fn test_selection_extend() {
        let mut sel = Selection::new(5, 10);
        sel.extend_to(15);
        assert_eq!(sel.anchor(), 5);
        assert_eq!(sel.active(), 15);
        assert_eq!(sel.range(), 5..15);
    }

    #[test]
    fn test_selection_extend_backward() {
        let mut sel = Selection::new(10, 15);
        sel.extend_to(5);
        assert_eq!(sel.anchor(), 10);
        assert_eq!(sel.active(), 5);
        assert_eq!(sel.range(), 5..10);
        assert!(sel.is_backward());
    }

    #[test]
    fn test_selection_move_to() {
        let mut sel = Selection::new(5, 10);
        sel.move_to(20);
        assert_eq!(sel.anchor(), 20);
        assert_eq!(sel.active(), 20);
        assert!(sel.is_collapsed());
    }

    #[test]
    fn test_selection_collapse_to_start() {
        let mut sel = Selection::new(5, 10);
        sel.collapse_to_start();
        assert_eq!(sel.anchor(), 5);
        assert_eq!(sel.active(), 5);
        assert!(sel.is_collapsed());
    }

    #[test]
    fn test_selection_collapse_to_end() {
        let mut sel = Selection::new(5, 10);
        sel.collapse_to_end();
        assert_eq!(sel.anchor(), 10);
        assert_eq!(sel.active(), 10);
        assert!(sel.is_collapsed());
    }

    #[test]
    fn test_selection_contains() {
        let sel = Selection::new(5, 10);
        assert!(!sel.contains(4));
        assert!(sel.contains(5));
        assert!(sel.contains(7));
        assert!(sel.contains(9));
        assert!(!sel.contains(10));
        assert!(!sel.contains(11));
    }

    #[test]
    fn test_selection_flip() {
        let mut sel = Selection::new(5, 10);
        assert!(sel.is_forward());
        sel.flip();
        assert_eq!(sel.anchor(), 10);
        assert_eq!(sel.active(), 5);
        assert!(sel.is_backward());
    }

    #[test]
    fn test_selection_text() {
        let text = "Hello, World!";
        let sel = Selection::new(0, 5);
        assert_eq!(sel.text(text), "Hello");

        let sel = Selection::new(7, 12);
        assert_eq!(sel.text(text), "World");
    }

    #[test]
    fn test_selection_len() {
        let sel = Selection::new(5, 10);
        assert_eq!(sel.len(), 5);

        let sel = Selection::new(10, 5);
        assert_eq!(sel.len(), 5);

        let sel = Selection::collapsed(5);
        assert_eq!(sel.len(), 0);
    }
}
