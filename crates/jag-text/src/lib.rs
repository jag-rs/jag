//! detir-text: Custom text layout and shaping engine.
//!
//! Phase 1: core foundation pieces for detir-text.
//! - 1.2: font management layer (font loading, metrics, glyph outlines/bitmaps)
//! - 1.3: basic text shaping using harfbuzz_rs
//! - 1.4: Unicode grapheme handling (clusters, combining marks, emoji/ZWJ)

pub mod bidi;
pub mod font;
pub mod layout;
pub mod shaping;
pub mod unicode;

pub use font::{
    FontError,
    face::FontFace,
    loader::{FontCache, FontKey},
    metrics::{FontMetrics, ScaledFontMetrics},
};

pub use layout::{
    Cursor, CursorAffinity, CursorPosition, CursorRect, HitTestPolicy, HitTestResult, Point,
    Position,
};

/// Simple helper to allow smoke tests to link against this crate.
pub fn is_available() -> bool {
    true
}
