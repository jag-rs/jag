//! Alert notification banner element.

use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// Severity level for an alert, controlling its color scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertSeverity {
    Info,
    Success,
    Warning,
    Error,
}

impl AlertSeverity {
    /// Background color for this severity.
    pub fn bg_color(self) -> ColorLinPremul {
        match self {
            Self::Info => ColorLinPremul::from_srgba_u8([219, 234, 254, 255]),
            Self::Success => ColorLinPremul::from_srgba_u8([220, 252, 231, 255]),
            Self::Warning => ColorLinPremul::from_srgba_u8([254, 249, 195, 255]),
            Self::Error => ColorLinPremul::from_srgba_u8([254, 226, 226, 255]),
        }
    }

    /// Text color for this severity.
    pub fn text_color(self) -> ColorLinPremul {
        match self {
            Self::Info => ColorLinPremul::from_srgba_u8([30, 64, 175, 255]),
            Self::Success => ColorLinPremul::from_srgba_u8([22, 101, 52, 255]),
            Self::Warning => ColorLinPremul::from_srgba_u8([133, 77, 14, 255]),
            Self::Error => ColorLinPremul::from_srgba_u8([153, 27, 27, 255]),
        }
    }

    /// Border/accent color for this severity.
    pub fn border_color(self) -> ColorLinPremul {
        match self {
            Self::Info => ColorLinPremul::from_srgba_u8([147, 197, 253, 255]),
            Self::Success => ColorLinPremul::from_srgba_u8([134, 239, 172, 255]),
            Self::Warning => ColorLinPremul::from_srgba_u8([253, 224, 71, 255]),
            Self::Error => ColorLinPremul::from_srgba_u8([252, 165, 165, 255]),
        }
    }
}

/// A simple notification banner showing a message with severity-based
/// coloring and an optional dismiss action.
pub struct Alert {
    pub rect: Rect,
    /// Message text.
    pub message: String,
    /// Severity determines the color scheme.
    pub severity: AlertSeverity,
    /// Font size for the message.
    pub font_size: f32,
    /// Corner radius.
    pub radius: f32,
    /// Whether a dismiss ("X") button is shown.
    pub dismissible: bool,
    /// Whether the alert has been dismissed.
    pub dismissed: bool,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl Alert {
    /// Create an alert with the given message and severity.
    pub fn new(message: impl Into<String>, severity: AlertSeverity) -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 400.0,
                h: 48.0,
            },
            message: message.into(),
            severity,
            font_size: 14.0,
            radius: 6.0,
            dismissible: true,
            dismissed: false,
            focus_id: FocusId(0),
        }
    }

    /// Dismiss button rectangle (right side of the alert).
    fn dismiss_rect(&self) -> Rect {
        let size = 24.0;
        Rect {
            x: self.rect.x + self.rect.w - size - 12.0,
            y: self.rect.y + (self.rect.h - size) * 0.5,
            w: size,
            h: size,
        }
    }

    /// Hit-test the dismiss button.
    pub fn hit_test_dismiss(&self, x: f32, y: f32) -> bool {
        if !self.dismissible {
            return false;
        }
        let r = self.dismiss_rect();
        x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
    }

    /// Hit-test the entire alert area.
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

impl Element for Alert {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        if self.dismissed {
            return;
        }

        let bg = self.severity.bg_color();
        let border = self.severity.border_color();
        let text_color = self.severity.text_color();

        // Background + border
        let rrect = RoundedRect {
            rect: self.rect,
            radii: RoundedRadii {
                tl: self.radius,
                tr: self.radius,
                br: self.radius,
                bl: self.radius,
            },
        };
        jag_surface::shapes::draw_snapped_rounded_rectangle(
            canvas,
            rrect,
            Some(Brush::Solid(bg)),
            Some(1.0),
            Some(Brush::Solid(border)),
            z,
        );

        // Message text
        let text_x = self.rect.x + 16.0;
        let text_y = self.rect.y + self.rect.h * 0.5 + self.font_size * 0.35;
        canvas.draw_text_run_weighted(
            [text_x, text_y],
            self.message.clone(),
            self.font_size,
            400.0,
            text_color,
            z + 1,
        );

        // Dismiss button
        if self.dismissible {
            let dr = self.dismiss_rect();
            canvas.draw_text_run_weighted(
                [dr.x + 5.0, dr.y + dr.h - 5.0],
                "\u{2715}".to_string(),
                14.0,
                400.0,
                text_color,
                z + 2,
            );
        }
    }

    fn focus_id(&self) -> Option<FocusId> {
        if self.dismissible {
            Some(self.focus_id)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait
// ---------------------------------------------------------------------------

impl EventHandler for Alert {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if self.dismissed {
            return EventResult::Ignored;
        }
        if event.button != MouseButton::Left || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        if self.hit_test_dismiss(event.x, event.y) {
            self.dismissed = true;
            EventResult::Handled
        } else {
            EventResult::Ignored
        }
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
        if self.dismissed {
            return false;
        }
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
    fn alert_severity_colors_differ() {
        let info_bg = AlertSeverity::Info.bg_color();
        let error_bg = AlertSeverity::Error.bg_color();
        // They should not be equal.
        assert_ne!(info_bg, error_bg);
    }

    #[test]
    fn alert_defaults() {
        let a = Alert::new("Something happened", AlertSeverity::Info);
        assert_eq!(a.message, "Something happened");
        assert_eq!(a.severity, AlertSeverity::Info);
        assert!(a.dismissible);
        assert!(!a.dismissed);
    }

    #[test]
    fn alert_dismiss_click() {
        let mut a = Alert::new("Msg", AlertSeverity::Warning);
        a.rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 48.0,
        };
        let dr = a.dismiss_rect();
        let evt = MouseClickEvent {
            button: MouseButton::Left,
            state: ElementState::Pressed,
            x: dr.x + 5.0,
            y: dr.y + 5.0,
            click_count: 1,
        };
        assert_eq!(a.handle_mouse_click(&evt), EventResult::Handled);
        assert!(a.dismissed);
    }

    #[test]
    fn alert_not_hittable_when_dismissed() {
        let mut a = Alert::new("Msg", AlertSeverity::Error);
        a.rect = Rect {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 48.0,
        };
        a.dismissed = true;
        assert!(!a.contains_point(200.0, 24.0));
    }

    #[test]
    fn alert_hit_test() {
        let a = Alert::new("Msg", AlertSeverity::Success);
        assert!(a.hit_test(200.0, 24.0));
        assert!(!a.hit_test(500.0, 24.0));
    }
}
