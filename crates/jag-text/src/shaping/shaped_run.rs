use core::ops::Range;

use swash::GlyphId;

/// Logical direction of a shaped run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
}

/// Unicode script tag for the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Latin,
    // Future: add more scripts or use Unicode script property directly.
}

/// A run of text shaped with a single font.
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// Byte range in source text.
    pub text_range: Range<usize>,
    /// Font identifier within a layout/font context.
    pub font_id: u32,
    /// Font size in pixels.
    pub font_size: f32,
    /// Glyph IDs in logical order for this run.
    pub glyphs: Vec<GlyphId>,
    /// Glyph positions (x, y offsets from pen position).
    pub positions: Vec<GlyphPosition>,
    /// Glyph advances in pixels.
    pub advances: Vec<f32>,
    /// Cluster indices mapping glyphs to byte offsets in the source text.
    /// Each entry is a byte offset relative to text_range.start.
    /// Multiple glyphs can map to the same cluster (ligatures).
    pub clusters: Vec<u32>,
    /// Total advance width of the run in pixels.
    pub width: f32,
    /// X offset within line (for alignment).
    pub x_offset: f32,
    /// BiDi embedding level. For Phase 1.3 we only handle simple LTR (level 0).
    pub bidi_level: u8,
    /// Logical direction of this run.
    pub direction: Direction,
    /// Script of this run.
    pub script: Script,
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphPosition {
    pub x_offset: f32,
    pub y_offset: f32,
}
