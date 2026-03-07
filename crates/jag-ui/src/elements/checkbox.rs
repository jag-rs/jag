//! Checkbox element with label and validation support.

use jag_draw::{Brush, Color, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A toggle checkbox with optional label and validation state.
pub struct Checkbox {
    pub rect: Rect,
    pub checked: bool,
    pub focused: bool,
    pub label: Option<String>,
    pub label_size: f32,
    pub label_color: ColorLinPremul,
    pub box_fill: ColorLinPremul,
    pub border_color: ColorLinPremul,
    pub border_width: f32,
    pub check_color: ColorLinPremul,
    /// Whether the checkbox must be checked for form validation.
    pub required: bool,
    /// Static error message shown when validation fails.
    pub error_message: Option<String>,
    /// Dynamic validation error (set by form logic).
    pub validation_error: Option<String>,
    /// Focus identifier for this checkbox.
    pub focus_id: FocusId,
}

impl Checkbox {
    /// Create a checkbox with sensible defaults (unchecked, no label).
    pub fn new() -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 18.0,
                h: 18.0,
            },
            checked: false,
            focused: false,
            label: None,
            label_size: 14.0,
            label_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            box_fill: ColorLinPremul::from_srgba_u8([40, 40, 40, 255]),
            border_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            border_width: 1.0,
            check_color: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
            required: false,
            error_message: None,
            validation_error: None,
            focus_id: FocusId(0),
        }
    }

    /// Toggle the checked state.
    pub fn toggle(&mut self) {
        self.checked = !self.checked;
    }

    /// Hit-test the checkbox box itself.
    pub fn hit_test_box(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
    }

    /// Hit-test the label area (if a label exists).
    pub fn hit_test_label(&self, x: f32, y: f32) -> bool {
        if let Some(label) = &self.label {
            let label_x = self.rect.x + self.rect.w + 8.0;
            let char_width = self.label_size * 0.5;
            let label_width = label.len() as f32 * char_width;
            let clickable_height = self.rect.h.max(self.label_size * 1.2);

            x >= label_x
                && x <= label_x + label_width
                && y >= self.rect.y
                && y <= self.rect.y + clickable_height
        } else {
            false
        }
    }
}

impl Default for Checkbox {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Checkbox {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        let base_rrect = RoundedRect {
            rect: self.rect,
            radii: RoundedRadii {
                tl: 2.0,
                tr: 2.0,
                br: 2.0,
                bl: 2.0,
            },
        };

        // Border + fill
        if self.border_width > 0.0 {
            let has_error = self.validation_error.is_some();
            let border_color = if has_error {
                Color::rgba(220, 38, 38, 255)
            } else {
                self.border_color
            };
            let border_width = if has_error {
                self.border_width.max(2.0)
            } else {
                self.border_width
            };

            jag_surface::shapes::draw_snapped_rounded_rectangle(
                canvas,
                base_rrect,
                Some(Brush::Solid(self.box_fill)),
                Some(border_width),
                Some(Brush::Solid(border_color)),
                z,
            );
        } else {
            canvas.fill_rect(
                self.rect.x,
                self.rect.y,
                self.rect.w,
                self.rect.h,
                Brush::Solid(self.box_fill),
                z,
            );
        }

        // Focus outline
        if self.focused {
            let focus_rr = RoundedRect {
                rect: self.rect,
                radii: RoundedRadii {
                    tl: 2.0,
                    tr: 2.0,
                    br: 2.0,
                    bl: 2.0,
                },
            };
            let focus = Brush::Solid(Color::rgba(63, 130, 246, 255));
            jag_surface::shapes::draw_snapped_rounded_rectangle(
                canvas,
                focus_rr,
                None,
                Some(2.0),
                Some(focus),
                z + 2,
            );
        }

        // Checked state: inner filled square with checkmark
        if self.checked {
            let inset = 2.0_f32;
            let inner = Rect {
                x: (self.rect.x + inset).round(),
                y: (self.rect.y + inset).round(),
                w: (self.rect.w - 2.0 * inset).max(0.0).round(),
                h: (self.rect.h - 2.0 * inset).max(0.0).round(),
            };
            let inner_rr = RoundedRect {
                rect: inner,
                radii: RoundedRadii {
                    tl: 1.5,
                    tr: 1.5,
                    br: 1.5,
                    bl: 1.5,
                },
            };
            canvas.rounded_rect(inner_rr, Brush::Solid(self.check_color), z + 2);

            // Simple text checkmark instead of SVG (standalone, no asset deps).
            let mark_size = inner.w * 0.7;
            let mark_x = inner.x + (inner.w - mark_size * 0.5) * 0.5;
            let mark_y = inner.y + inner.h * 0.75;
            canvas.draw_text_run_weighted(
                [mark_x, mark_y],
                "\u{2713}".to_string(),
                mark_size,
                700.0,
                ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
                z + 3,
            );
        }

        // Label
        if let Some(text) = &self.label {
            let tx = self.rect.x + self.rect.w + 8.0;
            let ty = self.rect.y + self.rect.h * 0.5 + self.label_size * 0.32;
            canvas.draw_text_run_weighted(
                [tx, ty],
                text.clone(),
                self.label_size,
                400.0,
                self.label_color,
                z + 3,
            );
        }

        // Validation error message
        if let Some(ref error_msg) = self.validation_error {
            let error_size = (self.label_size * 0.85).max(12.0);
            let baseline_offset = error_size * 0.8;
            let top_gap = 3.0;
            let control_height = self.rect.h.max(self.label_size * 1.2);
            let error_y = self.rect.y + control_height + top_gap + baseline_offset;
            let error_color = ColorLinPremul::from_srgba_u8([220, 38, 38, 255]);

            canvas.draw_text_run_weighted(
                [self.rect.x, error_y],
                error_msg.clone(),
                error_size,
                400.0,
                error_color,
                z + 4,
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

impl EventHandler for Checkbox {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if event.button != MouseButton::Left || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        if self.hit_test_box(event.x, event.y) || self.hit_test_label(event.x, event.y) {
            self.toggle();
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
            KeyCode::Space | KeyCode::Enter => {
                self.toggle();
                EventResult::Handled
            }
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
        self.hit_test_box(x, y) || self.hit_test_label(x, y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkbox_toggle() {
        let mut cb = Checkbox::new();
        assert!(!cb.checked);
        cb.toggle();
        assert!(cb.checked);
        cb.toggle();
        assert!(!cb.checked);
    }

    #[test]
    fn checkbox_default_is_new() {
        let cb = Checkbox::default();
        assert!(!cb.checked);
        assert!(!cb.focused);
        assert!(cb.label.is_none());
    }

    #[test]
    fn checkbox_hit_test_box() {
        let mut cb = Checkbox::new();
        cb.rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 18.0,
            h: 18.0,
        };
        assert!(cb.hit_test_box(15.0, 15.0));
        assert!(!cb.hit_test_box(0.0, 0.0));
    }

    #[test]
    fn checkbox_hit_test_label() {
        let mut cb = Checkbox::new();
        cb.rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 18.0,
            h: 18.0,
        };
        cb.label = Some("Accept".to_string());
        // label starts at x=10+18+8=36
        assert!(cb.hit_test_label(40.0, 15.0));
        assert!(!cb.hit_test_label(5.0, 15.0));
    }

    #[test]
    fn checkbox_contains_point_covers_both() {
        let mut cb = Checkbox::new();
        cb.rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 18.0,
            h: 18.0,
        };
        cb.label = Some("Check".to_string());
        // Box area
        assert!(cb.contains_point(15.0, 15.0));
        // Label area
        assert!(cb.contains_point(40.0, 15.0));
        // Outside both
        assert!(!cb.contains_point(0.0, 0.0));
    }

    #[test]
    fn checkbox_focus() {
        let mut cb = Checkbox::new();
        assert!(!cb.is_focused());
        cb.set_focused(true);
        assert!(cb.is_focused());
    }

    #[test]
    fn checkbox_keyboard_toggle() {
        let mut cb = Checkbox::new();
        cb.focused = true;
        let evt = KeyboardEvent {
            key: KeyCode::Space,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert!(!cb.checked);
        assert_eq!(cb.handle_keyboard(&evt), EventResult::Handled);
        assert!(cb.checked);
    }

    #[test]
    fn checkbox_keyboard_ignored_without_focus() {
        let mut cb = Checkbox::new();
        cb.focused = false;
        let evt = KeyboardEvent {
            key: KeyCode::Space,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert_eq!(cb.handle_keyboard(&evt), EventResult::Ignored);
        assert!(!cb.checked);
    }
}
