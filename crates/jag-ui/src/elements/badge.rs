//! Small label badge element.

use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    EventHandler, EventResult, KeyboardEvent, MouseClickEvent, MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A small rounded pill badge for labeling or status indication.
pub struct Badge {
    pub rect: Rect,
    /// Display text.
    pub text: String,
    /// Background color.
    pub bg_color: ColorLinPremul,
    /// Text color.
    pub text_color: ColorLinPremul,
    /// Font size.
    pub font_size: f32,
    /// Horizontal padding.
    pub padding_x: f32,
    /// Vertical padding.
    pub padding_y: f32,
}

impl Badge {
    /// Create a badge with default styling (blue pill).
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 0.0,
                h: 0.0,
            },
            text: text.into(),
            bg_color: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
            text_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            font_size: 12.0,
            padding_x: 8.0,
            padding_y: 4.0,
        }
    }

    /// Set custom colors.
    pub fn with_colors(mut self, bg: ColorLinPremul, fg: ColorLinPremul) -> Self {
        self.bg_color = bg;
        self.text_color = fg;
        self
    }

    /// Compute the auto-sized rect based on text length.
    /// The badge sizes itself around its text if rect dimensions are zero.
    pub fn auto_size(&mut self) {
        let approx_text_w = self.text.len() as f32 * self.font_size * 0.6;
        self.rect.w = approx_text_w + self.padding_x * 2.0;
        self.rect.h = self.font_size + self.padding_y * 2.0;
    }

    /// Corner radius for the pill shape (half of height).
    fn pill_radius(&self) -> f32 {
        self.rect.h * 0.5
    }

    /// Hit-test.
    pub fn hit_test(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Badge {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        let r = self.pill_radius();
        let rrect = RoundedRect {
            rect: self.rect,
            radii: RoundedRadii {
                tl: r,
                tr: r,
                br: r,
                bl: r,
            },
        };
        canvas.rounded_rect(rrect, Brush::Solid(self.bg_color), z);

        // Centered text
        let approx_text_w = self.text.len() as f32 * self.font_size * 0.6;
        let tx = self.rect.x + (self.rect.w - approx_text_w) * 0.5;
        let ty = self.rect.y + self.rect.h * 0.5 + self.font_size * 0.35;
        canvas.draw_text_run_weighted(
            [tx, ty],
            self.text.clone(),
            self.font_size,
            600.0,
            self.text_color,
            z + 1,
        );
    }

    fn focus_id(&self) -> Option<FocusId> {
        None
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait
// ---------------------------------------------------------------------------

impl EventHandler for Badge {
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

    fn set_focused(&mut self, _focused: bool) {}

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
    fn badge_defaults() {
        let b = Badge::new("New");
        assert_eq!(b.text, "New");
        assert!((b.font_size - 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn badge_auto_size() {
        let mut b = Badge::new("Hello");
        b.auto_size();
        assert!(b.rect.w > 0.0);
        assert!(b.rect.h > 0.0);
        // Width should be approx 5 chars * 12 * 0.6 + 16 padding = 52
        assert!((b.rect.w - 52.0).abs() < 1.0);
    }

    #[test]
    fn badge_with_colors() {
        let red = ColorLinPremul::from_srgba_u8([255, 0, 0, 255]);
        let white = ColorLinPremul::from_srgba_u8([255, 255, 255, 255]);
        let b = Badge::new("Error").with_colors(red, white);
        assert_eq!(b.bg_color, red);
        assert_eq!(b.text_color, white);
    }

    #[test]
    fn badge_pill_radius() {
        let mut b = Badge::new("X");
        b.rect.h = 24.0;
        assert!((b.pill_radius() - 12.0).abs() < f32::EPSILON);
    }

    #[test]
    fn badge_hit_test() {
        let mut b = Badge::new("Test");
        b.rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 60.0,
            h: 24.0,
        };
        assert!(b.hit_test(30.0, 20.0));
        assert!(!b.hit_test(0.0, 0.0));
    }

    #[test]
    fn badge_not_focusable() {
        let b = Badge::new("X");
        assert!(b.focus_id().is_none());
    }
}
