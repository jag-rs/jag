use super::cursor::{CursorAffinity, CursorPosition};

/// Result of a hit test operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitTestResult {
    /// Byte offset in the source text.
    pub byte_offset: usize,
    /// Cursor affinity at this position.
    pub affinity: CursorAffinity,
    /// Line index containing this position.
    pub line_index: usize,
}

impl HitTestResult {
    /// Create a new hit test result.
    pub fn new(byte_offset: usize, affinity: CursorAffinity, line_index: usize) -> Self {
        Self {
            byte_offset,
            affinity,
            line_index,
        }
    }

    /// Convert to a cursor position.
    pub fn to_cursor_position(&self) -> CursorPosition {
        CursorPosition::with_affinity(self.byte_offset, self.affinity)
    }
}

/// Represents a 2D point in zone-local coordinates.
///
/// Zone-local coordinates are relative to the top-left corner of the text layout,
/// not absolute screen coordinates. This ensures the hit testing works correctly
/// regardless of where the text is positioned on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X coordinate in pixels (relative to layout origin).
    pub x: f32,
    /// Y coordinate in pixels (relative to layout origin).
    pub y: f32,
}

impl Point {
    /// Create a new point.
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Create a point at the origin.
    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

/// Represents a 2D position in zone-local coordinates.
///
/// This is used for mapping character offsets to visual positions.
/// All coordinates are relative to the text layout's origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// X coordinate in pixels (relative to layout origin).
    pub x: f32,
    /// Y coordinate in pixels (relative to layout origin).
    pub y: f32,
    /// Line index.
    pub line_index: usize,
}

impl Position {
    /// Create a new position.
    pub fn new(x: f32, y: f32, line_index: usize) -> Self {
        Self { x, y, line_index }
    }

    /// Convert to a point (discarding line index).
    pub fn to_point(&self) -> Point {
        Point::new(self.x, self.y)
    }
}

/// Hit test policy for handling edge cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitTestPolicy {
    /// Clamp to the nearest valid position within the text.
    Clamp,
    /// Return None if the point is outside the text bounds.
    Strict,
}

impl Default for HitTestPolicy {
    fn default() -> Self {
        Self::Clamp
    }
}

/// Helper to determine cursor affinity at BiDi boundaries.
///
/// When a cursor position is at a boundary between LTR and RTL text,
/// the affinity determines which side the cursor visually appears on.
pub fn affinity_at_bidi_boundary(
    _byte_offset: usize,
    _bidi_level: u8,
    is_rtl_run: bool,
) -> CursorAffinity {
    // For RTL text, upstream affinity means visually to the right
    // For LTR text, upstream affinity means visually to the left
    if is_rtl_run {
        // In RTL, prefer downstream (visually left)
        CursorAffinity::Downstream
    } else {
        // In LTR, prefer downstream (visually right)
        CursorAffinity::Downstream
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hit_test_result() {
        let result = HitTestResult::new(10, CursorAffinity::Downstream, 2);
        assert_eq!(result.byte_offset, 10);
        assert_eq!(result.affinity, CursorAffinity::Downstream);
        assert_eq!(result.line_index, 2);

        let pos = result.to_cursor_position();
        assert_eq!(pos.byte_offset, 10);
        assert_eq!(pos.affinity, CursorAffinity::Downstream);
    }

    #[test]
    fn test_point() {
        let p = Point::new(10.0, 20.0);
        assert_eq!(p.x, 10.0);
        assert_eq!(p.y, 20.0);

        let zero = Point::zero();
        assert_eq!(zero.x, 0.0);
        assert_eq!(zero.y, 0.0);
    }

    #[test]
    fn test_position() {
        let pos = Position::new(15.0, 30.0, 1);
        assert_eq!(pos.x, 15.0);
        assert_eq!(pos.y, 30.0);
        assert_eq!(pos.line_index, 1);

        let point = pos.to_point();
        assert_eq!(point.x, 15.0);
        assert_eq!(point.y, 30.0);
    }

    #[test]
    fn test_hit_test_policy_default() {
        assert_eq!(HitTestPolicy::default(), HitTestPolicy::Clamp);
    }
}
