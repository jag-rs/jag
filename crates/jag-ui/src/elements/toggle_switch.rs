//! Toggle switch element with sliding thumb.

use jag_draw::{Brush, Color, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A toggle switch with a sliding thumb, on/off state, and optional label.
pub struct ToggleSwitch {
    /// Bounding rect of the toggle track.
    pub rect: Rect,
    /// Whether the toggle is ON (`true`) or OFF (`false`).
    pub on: bool,
    /// Whether the toggle is currently focused.
    pub focused: bool,
    /// Optional label displayed after the toggle.
    pub label: Option<String>,
    /// Label font size.
    pub label_size: f32,
    /// Label text color.
    pub label_color: ColorLinPremul,
    /// Track color when ON.
    pub on_color: ColorLinPremul,
    /// Track color when OFF.
    pub off_color: ColorLinPremul,
    /// Thumb (sliding circle) color.
    pub thumb_color: ColorLinPremul,
    /// Border color for the track.
    pub border_color: ColorLinPremul,
    /// Border width.
    pub border_width: f32,
    /// Validation error message.
    pub validation_error: Option<String>,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl ToggleSwitch {
    /// Default track width.
    pub const DEFAULT_WIDTH: f32 = 44.0;
    /// Default track height.
    pub const DEFAULT_HEIGHT: f32 = 24.0;
    /// Padding inside the track for the thumb.
    const THUMB_PADDING: f32 = 2.0;

    /// Create a toggle switch with sensible defaults (OFF).
    pub fn new() -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: Self::DEFAULT_WIDTH,
                h: Self::DEFAULT_HEIGHT,
            },
            on: false,
            focused: false,
            label: None,
            label_size: 14.0,
            label_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            on_color: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
            off_color: ColorLinPremul::from_srgba_u8([120, 120, 120, 255]),
            thumb_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            border_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            border_width: 0.0,
            validation_error: None,
            focus_id: FocusId(0),
        }
    }

    /// Toggle the switch state.
    pub fn toggle(&mut self) {
        self.on = !self.on;
    }

    /// Hit-test the toggle track.
    pub fn hit_test_track(&self, x: f32, y: f32) -> bool {
        x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
    }

    /// Hit-test the label area.
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

impl Default for ToggleSwitch {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for ToggleSwitch {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        let track_height = self.rect.h;
        let corner_radius = track_height * 0.5; // pill shape

        let track_color = if self.on {
            self.on_color
        } else {
            self.off_color
        };

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

        let track_rrect = RoundedRect {
            rect: self.rect,
            radii: RoundedRadii {
                tl: corner_radius,
                tr: corner_radius,
                br: corner_radius,
                bl: corner_radius,
            },
        };

        if border_width > 0.0 {
            jag_surface::shapes::draw_snapped_rounded_rectangle(
                canvas,
                track_rrect,
                Some(Brush::Solid(track_color)),
                Some(border_width),
                Some(Brush::Solid(border_color)),
                z,
            );
        } else {
            canvas.rounded_rect(track_rrect, Brush::Solid(track_color), z);
        }

        // Focus outline
        if self.focused {
            let focus_rr = RoundedRect {
                rect: self.rect,
                radii: RoundedRadii {
                    tl: corner_radius,
                    tr: corner_radius,
                    br: corner_radius,
                    bl: corner_radius,
                },
            };
            jag_surface::shapes::draw_snapped_rounded_rectangle(
                canvas,
                focus_rr,
                None,
                Some(2.0),
                Some(Brush::Solid(Color::rgba(63, 130, 246, 255))),
                z + 2,
            );
        }

        // Thumb
        let thumb_diameter = track_height - Self::THUMB_PADDING * 2.0;
        let thumb_radius = thumb_diameter * 0.5;

        let thumb_x = if self.on {
            self.rect.x + self.rect.w - Self::THUMB_PADDING - thumb_diameter
        } else {
            self.rect.x + Self::THUMB_PADDING
        };
        let thumb_y = self.rect.y + Self::THUMB_PADDING;

        let thumb_rect = Rect {
            x: thumb_x,
            y: thumb_y,
            w: thumb_diameter,
            h: thumb_diameter,
        };
        let thumb_rrect = RoundedRect {
            rect: thumb_rect,
            radii: RoundedRadii {
                tl: thumb_radius,
                tr: thumb_radius,
                br: thumb_radius,
                bl: thumb_radius,
            },
        };

        // Shadow
        let shadow_rect = Rect {
            x: thumb_rect.x,
            y: thumb_rect.y + 1.0,
            w: thumb_rect.w,
            h: thumb_rect.h,
        };
        let shadow_rrect = RoundedRect {
            rect: shadow_rect,
            radii: thumb_rrect.radii,
        };
        canvas.rounded_rect(shadow_rrect, Brush::Solid(Color::rgba(0, 0, 0, 40)), z + 2);

        // Thumb circle
        canvas.rounded_rect(thumb_rrect, Brush::Solid(self.thumb_color), z + 3);

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

        // Validation error
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

impl EventHandler for ToggleSwitch {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if event.button != MouseButton::Left || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        if self.hit_test_track(event.x, event.y) || self.hit_test_label(event.x, event.y) {
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
        self.hit_test_track(x, y) || self.hit_test_label(x, y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_new_defaults() {
        let ts = ToggleSwitch::new();
        assert!(!ts.on);
        assert!(!ts.focused);
        assert!(ts.label.is_none());
    }

    #[test]
    fn toggle_toggle() {
        let mut ts = ToggleSwitch::new();
        assert!(!ts.on);
        ts.toggle();
        assert!(ts.on);
        ts.toggle();
        assert!(!ts.on);
    }

    #[test]
    fn toggle_hit_test_track() {
        let mut ts = ToggleSwitch::new();
        ts.rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 44.0,
            h: 24.0,
        };
        assert!(ts.hit_test_track(30.0, 20.0));
        assert!(!ts.hit_test_track(0.0, 0.0));
    }

    #[test]
    fn toggle_keyboard() {
        let mut ts = ToggleSwitch::new();
        ts.focused = true;
        let evt = KeyboardEvent {
            key: KeyCode::Space,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert!(!ts.on);
        assert_eq!(ts.handle_keyboard(&evt), EventResult::Handled);
        assert!(ts.on);
    }

    #[test]
    fn toggle_focus() {
        let mut ts = ToggleSwitch::new();
        assert!(!ts.is_focused());
        ts.set_focused(true);
        assert!(ts.is_focused());
    }
}
