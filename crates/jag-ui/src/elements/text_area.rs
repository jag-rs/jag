//! Multi-line text area element.

use jag_draw::{Brush, Color, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A multi-line text area with vertical scrolling, cursor, and basic editing.
///
/// This is a simplified standalone version that stores text as a `String`.
pub struct TextArea {
    /// Bounding rect.
    pub rect: Rect,
    /// Current text content.
    pub text: String,
    /// Font size.
    pub text_size: f32,
    /// Text color.
    pub text_color: ColorLinPremul,
    /// Placeholder text when empty.
    pub placeholder: Option<String>,
    /// Whether this text area is focused.
    pub focused: bool,
    /// Background color.
    pub bg_color: ColorLinPremul,
    /// Border color.
    pub border_color: ColorLinPremul,
    /// Border width.
    pub border_width: f32,
    /// Corner radius.
    pub corner_radius: f32,
    /// Validation error message.
    pub validation_error: Option<String>,
    /// Cursor byte position.
    pub cursor_position: usize,
    /// Vertical scroll offset.
    scroll_y: f32,
    /// Horizontal padding.
    padding_x: f32,
    /// Vertical padding.
    padding_y: f32,
    /// Line height multiplier.
    line_height_factor: f32,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl TextArea {
    /// Create a new text area.
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            text: String::new(),
            text_size: 14.0,
            text_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            placeholder: None,
            focused: false,
            bg_color: ColorLinPremul::from_srgba_u8([40, 40, 40, 255]),
            border_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            border_width: 1.0,
            corner_radius: 4.0,
            validation_error: None,
            cursor_position: 0,
            scroll_y: 0.0,
            padding_x: 8.0,
            padding_y: 8.0,
            line_height_factor: 1.3,
            focus_id: FocusId(0),
        }
    }

    /// Get the current text content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Set the text and move cursor to end.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor_position = self.text.len();
    }

    /// Set the placeholder text.
    pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
        self.placeholder = Some(placeholder.into());
    }

    /// Line height in logical pixels.
    fn line_height(&self) -> f32 {
        self.text_size * self.line_height_factor
    }

    /// Insert text at the cursor position.
    pub fn insert_text(&mut self, s: &str) {
        self.text.insert_str(self.cursor_position, s);
        self.cursor_position += s.len();
    }

    /// Delete one character before the cursor.
    pub fn delete_char_before(&mut self) {
        if self.cursor_position == 0 {
            return;
        }
        let prev = self.text[..self.cursor_position]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.text.drain(prev..self.cursor_position);
        self.cursor_position = prev;
    }

    /// Delete one character after the cursor.
    pub fn delete_char_after(&mut self) {
        if self.cursor_position >= self.text.len() {
            return;
        }
        let next = self.text[self.cursor_position..]
            .char_indices()
            .nth(1)
            .map(|(i, _)| self.cursor_position + i)
            .unwrap_or(self.text.len());
        self.text.drain(self.cursor_position..next);
    }

    /// Move cursor left by one character.
    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position = self.text[..self.cursor_position]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right by one character.
    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.text.len() {
            self.cursor_position = self.text[self.cursor_position..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_position + i)
                .unwrap_or(self.text.len());
        }
    }

    /// Move cursor home (beginning of current line).
    pub fn move_cursor_home(&mut self) {
        let before = &self.text[..self.cursor_position];
        if let Some(nl) = before.rfind('\n') {
            self.cursor_position = nl + 1;
        } else {
            self.cursor_position = 0;
        }
    }

    /// Move cursor to end of current line.
    pub fn move_cursor_end(&mut self) {
        let after = &self.text[self.cursor_position..];
        if let Some(nl) = after.find('\n') {
            self.cursor_position += nl;
        } else {
            self.cursor_position = self.text.len();
        }
    }

    /// Split text into lines for rendering.
    fn lines(&self) -> Vec<&str> {
        self.text.split('\n').collect()
    }

    /// Hit-test the text area rect.
    pub fn hit_test(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
    }
}

impl Default for TextArea {
    fn default() -> Self {
        Self::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 120.0,
        })
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for TextArea {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        // Background + border
        let rrect = RoundedRect {
            rect: self.rect,
            radii: RoundedRadii {
                tl: self.corner_radius,
                tr: self.corner_radius,
                br: self.corner_radius,
                bl: self.corner_radius,
            },
        };

        let has_error = self.validation_error.is_some();
        let border_color = if has_error {
            Color::rgba(220, 38, 38, 255)
        } else if self.focused {
            Color::rgba(63, 130, 246, 255)
        } else {
            self.border_color
        };
        let border_width = if has_error {
            self.border_width.max(2.0)
        } else if self.focused {
            (self.border_width + 1.0).max(2.0)
        } else {
            self.border_width
        };

        jag_surface::shapes::draw_snapped_rounded_rectangle(
            canvas,
            rrect,
            Some(Brush::Solid(self.bg_color)),
            Some(border_width),
            Some(Brush::Solid(border_color)),
            z,
        );

        let content_x = self.rect.x + self.padding_x;
        let content_y = self.rect.y + self.padding_y;
        let lh = self.line_height();

        if self.text.is_empty() {
            // Placeholder
            if let Some(ref ph) = self.placeholder {
                let ph_color = ColorLinPremul::from_srgba_u8([160, 160, 160, 255]);
                let baseline = content_y + self.text_size * 0.85 - self.scroll_y;
                canvas.draw_text_run_weighted(
                    [content_x, baseline],
                    ph.clone(),
                    self.text_size,
                    400.0,
                    ph_color,
                    z + 1,
                );
            }
        } else {
            // Render lines
            let lines = self.lines();
            for (i, line) in lines.iter().enumerate() {
                let baseline = content_y + (i as f32 * lh) + self.text_size * 0.85 - self.scroll_y;
                if baseline < self.rect.y - lh || baseline > self.rect.y + self.rect.h + lh {
                    continue; // skip lines outside the viewport
                }
                canvas.draw_text_run_weighted(
                    [content_x, baseline],
                    line.to_string(),
                    self.text_size,
                    400.0,
                    self.text_color,
                    z + 1,
                );
            }
        }

        // Cursor
        if self.focused {
            let before_cursor = &self.text[..self.cursor_position];
            let line_idx = before_cursor.matches('\n').count();
            let line_start = if let Some(nl) = before_cursor.rfind('\n') {
                nl + 1
            } else {
                0
            };
            let line_text = &self.text[line_start..self.cursor_position];
            let cursor_offset = canvas.measure_text_width(line_text, self.text_size);

            let cursor_x = content_x + cursor_offset;
            let cursor_y = content_y + (line_idx as f32 * lh) + 2.0 - self.scroll_y;
            let cursor_h = (lh - 4.0).max(0.0);

            canvas.fill_rect(
                cursor_x,
                cursor_y,
                1.5,
                cursor_h,
                Brush::Solid(self.text_color),
                z + 2,
            );
        }

        // Validation error
        if let Some(ref error_msg) = self.validation_error {
            let error_size = (self.text_size * 0.85).max(12.0);
            let baseline_offset = error_size * 0.8;
            let top_gap = 3.0;
            let error_y = self.rect.y + self.rect.h + top_gap + baseline_offset;
            let error_color = ColorLinPremul::from_srgba_u8([220, 38, 38, 255]);

            canvas.draw_text_run_weighted(
                [self.rect.x + self.padding_x, error_y],
                error_msg.clone(),
                error_size,
                400.0,
                error_color,
                z + 3,
            );
        }
    }

    fn focus_id(&self) -> Option<FocusId> {
        Some(self.focus_id)
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait
// ---------------------------------------------------------------------------

impl EventHandler for TextArea {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if event.button != MouseButton::Left || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        if self.hit_test(event.x, event.y) {
            EventResult::Handled
        } else {
            EventResult::Ignored
        }
    }

    fn handle_keyboard(&mut self, event: &KeyboardEvent) -> EventResult {
        if event.state != ElementState::Pressed || !self.focused {
            return EventResult::Ignored;
        }
        match event.key {
            KeyCode::Backspace => {
                self.delete_char_before();
                EventResult::Handled
            }
            KeyCode::Delete => {
                self.delete_char_after();
                EventResult::Handled
            }
            KeyCode::ArrowLeft => {
                self.move_cursor_left();
                EventResult::Handled
            }
            KeyCode::ArrowRight => {
                self.move_cursor_right();
                EventResult::Handled
            }
            KeyCode::Home => {
                self.move_cursor_home();
                EventResult::Handled
            }
            KeyCode::End => {
                self.move_cursor_end();
                EventResult::Handled
            }
            KeyCode::Enter => {
                self.insert_text("\n");
                EventResult::Handled
            }
            _ => {
                if let Some(ref text) = event.text
                    && !text.is_empty()
                    && text.chars().all(|c| !c.is_control() || c == ' ')
                {
                    self.insert_text(text);
                    return EventResult::Handled;
                }
                EventResult::Ignored
            }
        }
    }

    fn handle_mouse_move(&mut self, _event: &MouseMoveEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_scroll(&mut self, event: &ScrollEvent) -> EventResult {
        if self.hit_test(event.x, event.y) {
            self.scroll_y = (self.scroll_y - event.delta_y).max(0.0);
            EventResult::Handled
        } else {
            EventResult::Ignored
        }
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if focused {
            self.cursor_position = self.text.len();
        }
    }

    fn contains_point(&self, x: f32, y: f32) -> bool {
        self.hit_test(x, y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_area_defaults() {
        let ta = TextArea::default();
        assert!(ta.text.is_empty());
        assert!(!ta.focused);
        assert_eq!(ta.cursor_position, 0);
    }

    #[test]
    fn text_area_set_text() {
        let mut ta = TextArea::default();
        ta.set_text("Line1\nLine2");
        assert_eq!(ta.text(), "Line1\nLine2");
        assert_eq!(ta.cursor_position, 11);
    }

    #[test]
    fn text_area_insert_newline() {
        let mut ta = TextArea::default();
        ta.insert_text("hello");
        ta.insert_text("\n");
        ta.insert_text("world");
        assert_eq!(ta.text(), "hello\nworld");
        assert_eq!(ta.lines().len(), 2);
    }

    #[test]
    fn text_area_cursor_home_end() {
        let mut ta = TextArea::default();
        ta.set_text("line1\nline2\nline3");
        // Cursor at end of "line3"
        assert_eq!(ta.cursor_position, 17);

        ta.move_cursor_home();
        // Should go to start of "line3" (after second \n = 12)
        assert_eq!(ta.cursor_position, 12);

        ta.move_cursor_end();
        // Should go to end of "line3"
        assert_eq!(ta.cursor_position, 17);
    }

    #[test]
    fn text_area_delete() {
        let mut ta = TextArea::default();
        ta.set_text("abcd");
        ta.delete_char_before();
        assert_eq!(ta.text(), "abc");
        ta.move_cursor_home();
        ta.delete_char_after();
        assert_eq!(ta.text(), "bc");
    }

    #[test]
    fn text_area_keyboard_enter() {
        let mut ta = TextArea::default();
        ta.focused = true;
        ta.set_text("hello");
        ta.cursor_position = 5;
        let evt = KeyboardEvent {
            key: KeyCode::Enter,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert_eq!(ta.handle_keyboard(&evt), EventResult::Handled);
        assert_eq!(ta.text(), "hello\n");
    }

    #[test]
    fn text_area_scroll() {
        let mut ta = TextArea::default();
        ta.rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 100.0,
        };
        let evt = ScrollEvent {
            x: 50.0,
            y: 50.0,
            delta_x: 0.0,
            delta_y: -10.0,
        };
        assert_eq!(ta.handle_scroll(&evt), EventResult::Handled);
        assert!((ta.scroll_y - 10.0).abs() < f32::EPSILON);
    }
}
