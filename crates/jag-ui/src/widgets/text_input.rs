//! A reusable text input widget with blink-timer, placeholder, and flexible
//! rendering.
//!
//! Used by popup menu filter, find bar, goto bar, and future bottom-panel
//! inputs.

use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use super::types::WidgetColors;

const CARET_BLINK_RATE: f32 = 1.0;

/// A reusable text input widget with blink-timer, placeholder, and flexible
/// rendering.
#[derive(Debug, Clone)]
pub struct TextInput {
    text: String,
    caret_timer: f32,
    placeholder: String,
    text_size: f32,
}

impl TextInput {
    pub fn new(placeholder: impl Into<String>, text_size: f32) -> Self {
        Self {
            text: String::new(),
            caret_timer: 0.0,
            placeholder: placeholder.into(),
            text_size,
        }
    }

    // -- State mutators (all reset caret) --

    pub fn push_str(&mut self, s: &str) {
        self.text.push_str(s);
        self.reset_caret();
    }

    pub fn push_char(&mut self, c: char) {
        self.text.push(c);
        self.reset_caret();
    }

    pub fn pop_char(&mut self) -> Option<char> {
        let c = self.text.pop();
        self.reset_caret();
        c
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.reset_caret();
    }

    pub fn set_text(&mut self, s: String) {
        self.text = s;
        self.reset_caret();
    }

    // -- Accessors --

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    // -- Blink --

    pub fn update_blink(&mut self, dt: f32) {
        self.caret_timer = (self.caret_timer + dt) % CARET_BLINK_RATE;
    }

    pub fn reset_caret(&mut self) {
        self.caret_timer = 0.0;
    }

    fn caret_visible(&self) -> bool {
        self.caret_timer < CARET_BLINK_RATE * 0.5
    }

    // -- Rendering --

    /// Render the input field.
    ///
    /// - `corner_radius`: 0.0 for sharp (find/goto), >0 for rounded (popup).
    /// - `display_prefix`: prepended to text on screen (e.g. `":"` for goto
    ///   bar).
    /// - `focused`: controls caret visibility.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &self,
        canvas: &mut Canvas,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        bg: ColorLinPremul,
        border: Option<ColorLinPremul>,
        focused: bool,
        corner_radius: f32,
        display_prefix: Option<&str>,
        colors: &WidgetColors,
        z: i32,
    ) {
        let pad = if corner_radius > 0.0 { 6.0 } else { 4.0 };

        // Background
        if corner_radius > 0.0 {
            let rrect = RoundedRect {
                rect: Rect { x, y, w, h },
                radii: RoundedRadii {
                    tl: corner_radius,
                    tr: corner_radius,
                    br: corner_radius,
                    bl: corner_radius,
                },
            };
            canvas.rounded_rect(rrect, Brush::Solid(bg), z);
        } else {
            canvas.fill_rect(x, y, w, h, Brush::Solid(bg), z);
        }

        // Border
        if let Some(bc) = border {
            if corner_radius > 0.0 {
                let rrect = RoundedRect {
                    rect: Rect { x, y, w, h },
                    radii: RoundedRadii {
                        tl: corner_radius,
                        tr: corner_radius,
                        br: corner_radius,
                        bl: corner_radius,
                    },
                };
                canvas.stroke_rounded_rect(rrect, 1.0, Brush::Solid(bc), z + 1);
            } else {
                let b = 1.0;
                canvas.fill_rect(x, y, w, b, Brush::Solid(bc), z + 1);
                canvas.fill_rect(x, y + h - b, w, b, Brush::Solid(bc), z + 1);
                canvas.fill_rect(x, y, b, h, Brush::Solid(bc), z + 1);
                canvas.fill_rect(x + w - b, y, b, h, Brush::Solid(bc), z + 1);
            }
        }

        // Text / placeholder
        let text_y = y + h / 2.0 + self.text_size * 0.35;
        let text_x = x + pad;

        if self.text.is_empty() && display_prefix.is_none() {
            canvas.draw_text_run(
                [text_x, text_y],
                self.placeholder.clone(),
                self.text_size,
                colors.text_muted,
                z + 2,
            );
        } else {
            let display = match display_prefix {
                Some(pre) => format!("{pre}{}", self.text),
                None => self.text.clone(),
            };
            let color = if self.text.is_empty() {
                colors.text_muted
            } else {
                colors.text
            };
            canvas.draw_text_run([text_x, text_y], display, self.text_size, color, z + 2);
        }

        // Caret
        if focused && self.caret_visible() {
            let display_text = match display_prefix {
                Some(pre) if !self.text.is_empty() => format!("{pre}{}", self.text),
                _ => self.text.clone(),
            };
            let text_w = if display_text.is_empty() {
                0.0
            } else {
                canvas.measure_text_width(&display_text, self.text_size)
            };
            let caret_x = text_x + text_w;
            let caret_y = y + 3.0;
            let caret_h = h - 6.0;
            canvas.fill_rect(
                caret_x,
                caret_y,
                1.5,
                caret_h,
                Brush::Solid(colors.text),
                z + 3,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_input_is_empty() {
        let input = TextInput::new("Search...", 14.0);
        assert!(input.is_empty());
        assert_eq!(input.text(), "");
    }

    #[test]
    fn push_and_pop() {
        let mut input = TextInput::new("", 14.0);
        input.push_char('a');
        input.push_char('b');
        assert_eq!(input.text(), "ab");
        assert_eq!(input.pop_char(), Some('b'));
        assert_eq!(input.text(), "a");
    }

    #[test]
    fn push_str_appends() {
        let mut input = TextInput::new("", 14.0);
        input.push_str("hello");
        assert_eq!(input.text(), "hello");
    }

    #[test]
    fn clear_empties() {
        let mut input = TextInput::new("", 14.0);
        input.push_str("data");
        input.clear();
        assert!(input.is_empty());
    }

    #[test]
    fn set_text_replaces() {
        let mut input = TextInput::new("", 14.0);
        input.push_str("old");
        input.set_text("new".to_string());
        assert_eq!(input.text(), "new");
    }

    #[test]
    fn caret_blink_cycle() {
        let mut input = TextInput::new("", 14.0);
        // Initially visible (timer = 0).
        assert!(input.caret_visible());
        // After half the blink rate, caret should be hidden.
        input.update_blink(CARET_BLINK_RATE * 0.5);
        assert!(!input.caret_visible());
        // After a full cycle, visible again.
        input.update_blink(CARET_BLINK_RATE * 0.5);
        assert!(input.caret_visible());
    }

    #[test]
    fn reset_caret_makes_visible() {
        let mut input = TextInput::new("", 14.0);
        input.update_blink(CARET_BLINK_RATE * 0.75);
        assert!(!input.caret_visible());
        input.reset_caret();
        assert!(input.caret_visible());
    }
}
