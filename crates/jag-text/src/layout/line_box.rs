use core::ops::Range;

use crate::shaping::ShapedRun;

/// A single line of text with precise baseline positioning.
#[derive(Debug, Clone)]
pub struct LineBox {
    /// Byte offset range in the source text for this line.
    pub text_range: Range<usize>,
    /// Visual width of the line in pixels.
    pub width: f32,
    /// Total height of the line box in pixels.
    pub height: f32,
    /// Distance from line box top to baseline in pixels.
    pub baseline_offset: f32,
    /// Maximum ascent in this line (pixels above baseline).
    pub ascent: f32,
    /// Maximum descent in this line (pixels below baseline).
    pub descent: f32,
    /// Leading (line gap) in pixels.
    pub leading: f32,
    /// Shaped runs in visual order.
    pub runs: Vec<ShapedRun>,
    /// Y position of line box top (relative to paragraph) in pixels.
    pub y_offset: f32,
}

impl LineBox {
    /// Get baseline Y position (relative to paragraph).
    pub fn baseline_y(&self) -> f32 {
        self.y_offset + self.baseline_offset
    }

    /// Get line box bottom Y position.
    pub fn bottom_y(&self) -> f32 {
        self.y_offset + self.height
    }

    /// Check if a point is within this line.
    pub fn contains_point(&self, x: f32, y: f32) -> bool {
        y >= self.y_offset && y < self.bottom_y() && x >= 0.0 && x < self.width
    }
}
