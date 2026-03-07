//! Bidirectional (BiDi) text support built on `unicode-bidi`.
//!
//! Phase 3.1-3.2:
//! - Paragraph-level direction detection
//! - Embedding levels and visual reordering
//! - Mixed-direction runs and bracket mirroring helpers

pub mod levels;
pub mod mirror;
pub mod reorder;

pub use levels::{BaseDirection, ParagraphBidi, ParagraphDirection};
pub use mirror::mirrored_bracket;
pub use reorder::{BidiRun, reorder_line, visual_runs};
