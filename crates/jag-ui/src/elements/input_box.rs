//! Single-line text input element.

use jag_draw::{Brush, Color, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;
use super::text_align::TextAlign;

/// A single-line text input with cursor, placeholder, and basic editing.
///
/// This is a simplified standalone version that stores text as a `String`
/// and tracks a cursor byte position.  It does not depend on external
/// text-layout crates, making it suitable for lightweight UI toolkits.
pub struct InputBox {
    /// Bounding rect of the input field.
    pub rect: Rect,
    /// Current text content.
    pub text: String,
    /// Font size in logical pixels.
    pub text_size: f32,
    /// Text color.
    pub text_color: ColorLinPremul,
    /// Placeholder text shown when empty.
    pub placeholder: Option<String>,
    /// Text alignment within the field.
    pub text_align: TextAlign,
    /// Whether this input is focused.
    pub focused: bool,
    /// Whether this input is disabled.
    pub disabled: bool,
    /// Background color.
    pub bg_color: ColorLinPremul,
    /// Border color.
    pub border_color: ColorLinPremul,
    /// Border width.
    pub border_width: f32,
    /// Corner radius.
    pub corner_radius: f32,
    /// Input type hint (e.g. "text", "password", "email").
    pub input_type: String,
    /// Validation error message.
    pub validation_error: Option<String>,
    /// Cursor byte position in `text`.
    pub cursor_position: usize,
    /// Horizontal scroll offset.
    scroll_x: f32,
    /// Horizontal padding.
    padding_x: f32,
    /// Vertical padding.
    padding_y: f32,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl InputBox {
    /// Create a new input box with sensible defaults.
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            text: String::new(),
            text_size: 14.0,
            text_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            placeholder: None,
            text_align: TextAlign::Left,
            focused: false,
            disabled: false,
            bg_color: ColorLinPremul::from_srgba_u8([40, 40, 40, 255]),
            border_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            border_width: 1.0,
            corner_radius: 4.0,
            input_type: "text".to_string(),
            validation_error: None,
            cursor_position: 0,
            scroll_x: 0.0,
            padding_x: 8.0,
            padding_y: 4.0,
            focus_id: FocusId(0),
        }
    }

    /// Get the current text content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Set the text content and move cursor to the end.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor_position = self.text.len();
    }

    /// Set the placeholder text.
    pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
        self.placeholder = Some(placeholder.into());
    }

    /// Insert text at the current cursor position.
    pub fn insert_text(&mut self, s: &str) {
        if self.disabled {
            return;
        }
        self.text.insert_str(self.cursor_position, s);
        self.cursor_position += s.len();
    }

    /// Delete one character before the cursor (backspace).
    pub fn delete_char_before(&mut self) {
        if self.disabled || self.cursor_position == 0 {
            return;
        }
        // Find previous char boundary.
        let prev = self.text[..self.cursor_position]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.text.drain(prev..self.cursor_position);
        self.cursor_position = prev;
    }

    /// Delete one character after the cursor (delete key).
    pub fn delete_char_after(&mut self) {
        if self.disabled || self.cursor_position >= self.text.len() {
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

    /// Move cursor to the beginning.
    pub fn move_cursor_home(&mut self) {
        self.cursor_position = 0;
    }

    /// Move cursor to the end.
    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.text.len();
    }

    /// Display text (masks password fields).
    fn display_text(&self) -> String {
        if self.input_type == "password" {
            "\u{2022}".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        }
    }

    /// Hit-test the input field.
    pub fn hit_test(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
    }
}

impl Default for InputBox {
    fn default() -> Self {
        Self::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 32.0,
        })
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for InputBox {
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

        // Content area
        let content_x = self.rect.x + self.padding_x;
        let content_h = (self.rect.h - self.padding_y * 2.0).max(0.0);
        let baseline_y = self.rect.y + self.padding_y + content_h * 0.5 + self.text_size * 0.35;

        if self.text.is_empty() {
            // Placeholder
            if let Some(ref ph) = self.placeholder {
                let ph_color = ColorLinPremul::from_srgba_u8([160, 160, 160, 255]);
                canvas.draw_text_run_weighted(
                    [content_x, baseline_y],
                    ph.clone(),
                    self.text_size,
                    400.0,
                    ph_color,
                    z + 1,
                );
            }
        } else {
            // Text content
            let display = self.display_text();
            let text_x = content_x - self.scroll_x;
            canvas.draw_text_run_weighted(
                [text_x, baseline_y],
                display,
                self.text_size,
                400.0,
                self.text_color,
                z + 1,
            );
        }

        // Cursor (simple vertical line)
        if self.focused {
            let display = self.display_text();
            let cursor_text = if self.cursor_position <= display.len() {
                &display[..self.cursor_position]
            } else {
                &display
            };
            let cursor_offset = canvas.measure_text_width(cursor_text, self.text_size);
            let cursor_x = content_x + cursor_offset - self.scroll_x;
            let cursor_y = self.rect.y + self.padding_y + 2.0;
            let cursor_h = content_h - 4.0;

            canvas.fill_rect(
                cursor_x,
                cursor_y,
                1.5,
                cursor_h.max(0.0),
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

impl EventHandler for InputBox {
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
        if event.state != ElementState::Pressed || !self.focused || self.disabled {
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

    fn handle_scroll(&mut self, _event: &ScrollEvent) -> EventResult {
        EventResult::Ignored
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
    fn input_box_defaults() {
        let ib = InputBox::default();
        assert!(ib.text.is_empty());
        assert!(ib.placeholder.is_none());
        assert!(!ib.focused);
        assert_eq!(ib.cursor_position, 0);
    }

    #[test]
    fn input_box_set_text() {
        let mut ib = InputBox::default();
        ib.set_text("Hello");
        assert_eq!(ib.text(), "Hello");
        assert_eq!(ib.cursor_position, 5);
    }

    #[test]
    fn input_box_insert_and_delete() {
        let mut ib = InputBox::default();
        ib.insert_text("abc");
        assert_eq!(ib.text(), "abc");
        assert_eq!(ib.cursor_position, 3);

        ib.delete_char_before();
        assert_eq!(ib.text(), "ab");
        assert_eq!(ib.cursor_position, 2);

        ib.move_cursor_left();
        // cursor is now at position 1, before 'b'
        ib.delete_char_after();
        // 'b' was deleted, leaving "a"
        assert_eq!(ib.text(), "a");
        assert_eq!(ib.cursor_position, 1); // stayed at position 1
    }

    #[test]
    fn input_box_cursor_movement() {
        let mut ib = InputBox::default();
        ib.set_text("Hello");
        assert_eq!(ib.cursor_position, 5);

        ib.move_cursor_home();
        assert_eq!(ib.cursor_position, 0);

        ib.move_cursor_end();
        assert_eq!(ib.cursor_position, 5);

        ib.move_cursor_left();
        assert_eq!(ib.cursor_position, 4);

        ib.move_cursor_right();
        assert_eq!(ib.cursor_position, 5);
    }

    #[test]
    fn input_box_password_display() {
        let mut ib = InputBox::default();
        ib.input_type = "password".to_string();
        ib.set_text("secret");
        let display = ib.display_text();
        assert_eq!(display.chars().count(), 6);
        assert!(display.chars().all(|c| c == '\u{2022}'));
    }

    #[test]
    fn input_box_hit_test() {
        let ib = InputBox::new(Rect {
            x: 10.0,
            y: 10.0,
            w: 200.0,
            h: 32.0,
        });
        assert!(ib.hit_test(50.0, 25.0));
        assert!(!ib.hit_test(0.0, 0.0));
    }

    #[test]
    fn input_box_disabled_no_edit() {
        let mut ib = InputBox::default();
        ib.disabled = true;
        ib.insert_text("nope");
        assert!(ib.text.is_empty());
    }

    #[test]
    fn input_box_keyboard_typing() {
        let mut ib = InputBox::default();
        ib.focused = true;
        let evt = KeyboardEvent {
            key: KeyCode::Other(65),
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: Some("a".to_string()),
        };
        assert_eq!(ib.handle_keyboard(&evt), EventResult::Handled);
        assert_eq!(ib.text(), "a");
    }
}
