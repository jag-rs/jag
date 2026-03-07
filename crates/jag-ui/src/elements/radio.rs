//! Radio button element with circle, dot, and optional label.

use jag_draw::{Brush, Color, ColorLinPremul, Rect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A radio button rendered as a circle with an inner dot when selected.
pub struct Radio {
    /// Center of the radio circle in logical coordinates.
    pub center: [f32; 2],
    /// Outer radius of the radio circle.
    pub radius: f32,
    /// Whether this radio button is currently selected.
    pub selected: bool,
    /// Optional label rendered to the right of the circle.
    pub label: Option<String>,
    /// Label font size in logical pixels.
    pub label_size: f32,
    /// Label text color.
    pub label_color: ColorLinPremul,
    /// Fill color of the outer circle.
    pub bg: ColorLinPremul,
    /// Border color of the outer circle.
    pub border_color: ColorLinPremul,
    /// Border width of the outer circle.
    pub border_width: f32,
    /// Color of the inner dot when selected.
    pub dot_color: ColorLinPremul,
    /// Whether this radio button is focused.
    pub focused: bool,
    /// Validation error message displayed below the radio.
    pub validation_error: Option<String>,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl Radio {
    /// Create a radio button with sensible defaults (unselected, no label).
    pub fn new() -> Self {
        Self {
            center: [9.0, 9.0],
            radius: 9.0,
            selected: false,
            label: None,
            label_size: 14.0,
            label_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            bg: ColorLinPremul::from_srgba_u8([40, 40, 40, 255]),
            border_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            border_width: 1.0,
            dot_color: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
            focused: false,
            validation_error: None,
            focus_id: FocusId(0),
        }
    }

    /// Select this radio button.
    pub fn select(&mut self) {
        self.selected = true;
    }

    /// Deselect this radio button.
    pub fn deselect(&mut self) {
        self.selected = false;
    }

    /// Hit-test the radio circle.
    pub fn hit_test_circle(&self, x: f32, y: f32) -> bool {
        let dx = x - self.center[0];
        let dy = y - self.center[1];
        dx * dx + dy * dy <= self.radius * self.radius
    }

    /// Hit-test the label area (if a label exists).
    pub fn hit_test_label(&self, x: f32, y: f32) -> bool {
        if let Some(label) = &self.label {
            let label_x = self.center[0] + self.radius + 8.0;
            let char_width = self.label_size * 0.5;
            let label_width = label.len() as f32 * char_width;
            let clickable_height = (self.radius * 2.0).max(self.label_size * 1.2);

            x >= label_x
                && x <= label_x + label_width
                && y >= self.center[1] - clickable_height / 2.0
                && y <= self.center[1] + clickable_height / 2.0
        } else {
            false
        }
    }
}

impl Default for Radio {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Radio {
    fn rect(&self) -> Rect {
        let d = self.radius * 2.0;
        Rect {
            x: self.center[0] - self.radius,
            y: self.center[1] - self.radius,
            w: d,
            h: d,
        }
    }

    fn set_rect(&mut self, rect: Rect) {
        self.center = [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5];
        self.radius = rect.w.min(rect.h) * 0.5;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        // Background circle
        canvas.ellipse(
            self.center,
            [self.radius, self.radius],
            Brush::Solid(self.bg),
            z,
        );

        // Border
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
            jag_surface::shapes::draw_ellipse(
                canvas,
                self.center,
                [self.radius, self.radius],
                None,
                Some(border_width),
                Some(Brush::Solid(border_color)),
                z + 1,
            );
        }

        // Selected inner dot
        if self.selected {
            let inner = self.radius * 0.6;
            canvas.ellipse(
                self.center,
                [inner, inner],
                Brush::Solid(self.dot_color),
                z + 2,
            );
        }

        // Focus ring
        if self.focused {
            let focus_radius = self.radius + 2.0;
            jag_surface::shapes::draw_ellipse(
                canvas,
                self.center,
                [focus_radius, focus_radius],
                None,
                Some(2.0),
                Some(Brush::Solid(Color::rgba(63, 130, 246, 255))),
                z + 3,
            );
        }

        // Label
        if let Some(text) = &self.label {
            let pos = [
                self.center[0] + self.radius + 8.0,
                self.center[1] + self.label_size * 0.35,
            ];
            canvas.draw_text_run_weighted(
                pos,
                text.clone(),
                self.label_size,
                400.0,
                self.label_color,
                z + 3,
            );
        }

        // Validation error
        if let Some(ref error_msg) = self.validation_error {
            let error_size = (self.label_size * 0.85).max(12.0);
            let baseline_offset = error_size * 0.8;
            let top_gap = 3.0;
            let control_height = self.radius * 2.0;
            let error_y = self.center[1] + control_height * 0.5 + top_gap + baseline_offset;
            let error_x = self.center[0] - self.radius;
            let error_color = ColorLinPremul::from_srgba_u8([220, 38, 38, 255]);

            canvas.draw_text_run_weighted(
                [error_x, error_y],
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

impl EventHandler for Radio {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if event.button != MouseButton::Left || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        if self.hit_test_circle(event.x, event.y) || self.hit_test_label(event.x, event.y) {
            self.select();
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
                self.select();
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
        self.hit_test_circle(x, y) || self.hit_test_label(x, y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn radio_new_defaults() {
        let r = Radio::new();
        assert!(!r.selected);
        assert!(!r.focused);
        assert!(r.label.is_none());
    }

    #[test]
    fn radio_select_deselect() {
        let mut r = Radio::new();
        assert!(!r.selected);
        r.select();
        assert!(r.selected);
        r.deselect();
        assert!(!r.selected);
    }

    #[test]
    fn radio_hit_test_circle() {
        let mut r = Radio::new();
        r.center = [50.0, 50.0];
        r.radius = 10.0;
        assert!(r.hit_test_circle(50.0, 50.0)); // center
        assert!(r.hit_test_circle(55.0, 50.0)); // inside
        assert!(!r.hit_test_circle(70.0, 50.0)); // outside
    }

    #[test]
    fn radio_hit_test_label() {
        let mut r = Radio::new();
        r.center = [50.0, 50.0];
        r.radius = 10.0;
        r.label = Some("Option A".to_string());
        // label starts at x = 50 + 10 + 8 = 68
        assert!(r.hit_test_label(70.0, 50.0));
        assert!(!r.hit_test_label(40.0, 50.0));
    }

    #[test]
    fn radio_focus() {
        let mut r = Radio::new();
        assert!(!r.is_focused());
        r.set_focused(true);
        assert!(r.is_focused());
    }

    #[test]
    fn radio_keyboard_select() {
        let mut r = Radio::new();
        r.focused = true;
        let evt = KeyboardEvent {
            key: KeyCode::Space,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert!(!r.selected);
        assert_eq!(r.handle_keyboard(&evt), EventResult::Handled);
        assert!(r.selected);
    }
}
