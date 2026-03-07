pub mod cursor;
pub mod cursor_movement;
pub mod hit_test;
pub mod line_box;
pub mod line_breaker;
pub mod prefix_sums;
pub mod selection;
pub mod text_layout;
pub mod undo;

pub use cursor::{Cursor, CursorAffinity, CursorPosition, CursorRect};
pub use cursor_movement::{CursorMovement, MovementDirection, MovementUnit};
pub use hit_test::{HitTestPolicy, HitTestResult, Point, Position};
pub use line_box::LineBox;
pub use line_breaker::{LineBreak, LineBreakKind, WordBoundary, WordBoundaryKind};
pub use prefix_sums::PrefixSums;
pub use selection::{Selection, SelectionRect};
pub use text_layout::TextLayout;
pub use undo::{TextOperation, UndoStack};

/// Line wrapping strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    /// Do not perform automatic wrapping (only explicit newlines).
    NoWrap,
    /// Wrap at word boundaries where possible, falling back to
    /// grapheme boundaries for long words.
    BreakWord,
    /// Allow breaking between all grapheme clusters (aggressive).
    BreakAll,
}
