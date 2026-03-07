//! Simple text element.

use jag_draw::{Color, Rect};
use jag_surface::Canvas;

use crate::event::{
    EventHandler, EventResult, KeyboardEvent, MouseClickEvent, MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A single run of text rendered at a given position, size, and color.
pub struct Text {
    pub content: String,
    pub pos: [f32; 2],
    pub size: f32,
    pub color: Color,
}

impl Text {
    /// Create a new text element with the given content and font size.
    ///
    /// Position defaults to `[0, 0]` and color to white.
    pub fn new(content: impl Into<String>, size: f32) -> Self {
        Self {
            content: content.into(),
            pos: [0.0, 0.0],
            size,
            color: Color::from_srgba_u8([255, 255, 255, 255]),
        }
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Text {
    fn rect(&self) -> Rect {
        // Approximate width based on character count; real measurement
        // requires a text provider which is available at render time.
        let approx_w = self.content.len() as f32 * self.size * 0.5;
        Rect {
            x: self.pos[0],
            y: self.pos[1] - self.size,
            w: approx_w,
            h: self.size,
        }
    }

    fn set_rect(&mut self, rect: Rect) {
        self.pos = [rect.x, rect.y + rect.h];
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        canvas.draw_text_run_weighted(
            self.pos,
            self.content.clone(),
            self.size,
            400.0,
            self.color,
            z,
        );
    }

    fn focus_id(&self) -> Option<FocusId> {
        None
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait (text is non-interactive by default)
// ---------------------------------------------------------------------------

impl EventHandler for Text {
    fn handle_mouse_click(&mut self, _event: &MouseClickEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_keyboard(&mut self, _event: &KeyboardEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_mouse_move(&mut self, _event: &MouseMoveEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_scroll(&mut self, _event: &ScrollEvent) -> EventResult {
        EventResult::Ignored
    }

    fn is_focused(&self) -> bool {
        false
    }

    fn set_focused(&mut self, _focused: bool) {
        // Text elements are not focusable.
    }

    fn contains_point(&self, x: f32, y: f32) -> bool {
        let r = self.rect();
        x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_new() {
        let t = Text::new("Hello", 16.0);
        assert_eq!(t.content, "Hello");
        assert_eq!(t.size, 16.0);
        assert_eq!(t.pos, [0.0, 0.0]);
    }

    #[test]
    fn text_is_not_focusable() {
        let t = Text::new("No focus", 12.0);
        assert!(t.focus_id().is_none());
        assert!(!t.is_focused());
    }

    #[test]
    fn text_set_rect_updates_pos() {
        let mut t = Text::new("Pos", 14.0);
        t.set_rect(Rect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 14.0,
        });
        assert_eq!(t.pos[0], 10.0);
        // pos[1] = rect.y + rect.h = 20 + 14 = 34
        assert!((t.pos[1] - 34.0).abs() < f32::EPSILON);
    }
}
