//! Simplified date picker element with text input.

use jag_draw::{Brush, Color, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A simplified date picker rendered as a text input with a date value.
///
/// For now this is a text-input style element that stores a date as
/// an optional `(year, month, day)` tuple.  A full calendar popup can
/// be layered on top in a future iteration.
pub struct DatePicker {
    /// Bounding rect.
    pub rect: Rect,
    /// Font size.
    pub label_size: f32,
    /// Text color.
    pub label_color: ColorLinPremul,
    /// Whether the picker is focused.
    pub focused: bool,
    /// Selected date as (year, month, day).
    pub selected_date: Option<(u32, u32, u32)>,
    /// Background color.
    pub bg_color: ColorLinPremul,
    /// Border color.
    pub border_color: ColorLinPremul,
    /// Border width.
    pub border_width: f32,
    /// Corner radius.
    pub radius: f32,
    /// Padding [top, right, bottom, left].
    pub padding: [f32; 4],
    /// Validation error message.
    pub validation_error: Option<String>,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl DatePicker {
    /// Create a date picker with default styling and no date selected.
    pub fn new() -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 180.0,
                h: 36.0,
            },
            label_size: 14.0,
            label_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            focused: false,
            selected_date: None,
            bg_color: ColorLinPremul::from_srgba_u8([40, 40, 40, 255]),
            border_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            border_width: 1.0,
            radius: 4.0,
            padding: [8.0, 12.0, 8.0, 12.0],
            validation_error: None,
            focus_id: FocusId(0),
        }
    }

    /// Get the selected date as a formatted string.
    pub fn date_string(&self) -> String {
        match self.selected_date {
            Some((y, m, d)) => format!("{y:04}-{m:02}-{d:02}"),
            None => String::new(),
        }
    }

    /// Set the date.
    pub fn set_date(&mut self, year: u32, month: u32, day: u32) {
        self.selected_date = Some((year, month, day));
    }

    /// Clear the date.
    pub fn clear_date(&mut self) {
        self.selected_date = None;
    }

    /// Hit-test the field.
    pub fn hit_test(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
    }
}

impl Default for DatePicker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for DatePicker {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        let rrect = RoundedRect {
            rect: self.rect,
            radii: RoundedRadii {
                tl: self.radius,
                tr: self.radius,
                br: self.radius,
                bl: self.radius,
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

        // Date text or placeholder
        let pad_top = self.padding[0];
        let pad_left = self.padding[3];
        let pad_bottom = self.padding[2];
        let content_h = (self.rect.h - pad_top - pad_bottom).max(0.0);
        let baseline_y = self.rect.y + pad_top + content_h * 0.5 + self.label_size * 0.35;
        let text_x = self.rect.x + pad_left;

        let date_str = self.date_string();
        if date_str.is_empty() {
            let ph_color = ColorLinPremul::from_srgba_u8([160, 160, 160, 255]);
            canvas.draw_text_run_weighted(
                [text_x, baseline_y],
                "YYYY-MM-DD".to_string(),
                self.label_size,
                400.0,
                ph_color,
                z + 1,
            );
        } else {
            canvas.draw_text_run_weighted(
                [text_x, baseline_y],
                date_str,
                self.label_size,
                400.0,
                self.label_color,
                z + 1,
            );
        }

        // Calendar icon (simple text glyph)
        let pad_right = self.padding[1];
        let icon_x = self.rect.x + self.rect.w - pad_right - 14.0;
        canvas.draw_text_run_weighted(
            [icon_x, baseline_y],
            "\u{1F4C5}".to_string(),
            self.label_size,
            400.0,
            self.label_color,
            z + 2,
        );

        // Validation error
        if let Some(ref error_msg) = self.validation_error {
            let error_size = (self.label_size * 0.85).max(12.0);
            let baseline_offset = error_size * 0.8;
            let top_gap = 3.0;
            let error_y = self.rect.y + self.rect.h + top_gap + baseline_offset;
            let error_color = ColorLinPremul::from_srgba_u8([220, 38, 38, 255]);

            canvas.draw_text_run_weighted(
                [self.rect.x + pad_left, error_y],
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

impl EventHandler for DatePicker {
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
        // Date pickers are primarily mouse-driven; minimal keyboard support.
        match event.key {
            KeyCode::Escape => EventResult::Handled,
            _ => EventResult::Ignored,
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
    fn date_picker_defaults() {
        let dp = DatePicker::new();
        assert!(dp.selected_date.is_none());
        assert!(!dp.focused);
        assert_eq!(dp.date_string(), "");
    }

    #[test]
    fn date_picker_set_and_clear() {
        let mut dp = DatePicker::new();
        dp.set_date(2026, 3, 7);
        assert_eq!(dp.date_string(), "2026-03-07");
        assert_eq!(dp.selected_date, Some((2026, 3, 7)));

        dp.clear_date();
        assert!(dp.selected_date.is_none());
        assert_eq!(dp.date_string(), "");
    }

    #[test]
    fn date_picker_hit_test() {
        let mut dp = DatePicker::new();
        dp.rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 180.0,
            h: 36.0,
        };
        assert!(dp.hit_test(50.0, 25.0));
        assert!(!dp.hit_test(0.0, 0.0));
    }

    #[test]
    fn date_picker_focus() {
        let mut dp = DatePicker::new();
        assert!(!dp.is_focused());
        dp.set_focused(true);
        assert!(dp.is_focused());
    }
}
