//! Clickable button element.

use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;
use crate::theme::Theme;

use super::Element;

/// Horizontal alignment for button labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonLabelAlign {
    Start,
    Center,
    End,
}

/// A clickable button with label, optional icon, and focus support.
pub struct Button {
    pub rect: Rect,
    pub label: String,
    pub label_size: f32,
    pub label_align: ButtonLabelAlign,
    pub bg: ColorLinPremul,
    pub fg: ColorLinPremul,
    pub radius: f32,
    pub focused: bool,
    pub focus_visible: bool,
    /// Padding: [top, right, bottom, left].
    pub padding: [f32; 4],
    /// Optional icon asset path (SVG or raster).
    pub icon_path: Option<String>,
    /// Logical icon size in pixels (square).
    pub icon_size: f32,
    /// Horizontal spacing between icon and label.
    pub icon_spacing: f32,
    /// When `true`, suppress label rendering (icon-only button).
    pub icon_only: bool,
    /// Focus identifier for this button.
    pub focus_id: FocusId,
}

impl Button {
    /// Create a button with sensible defaults.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 120.0,
                h: 36.0,
            },
            label: label.into(),
            label_size: 14.0,
            label_align: ButtonLabelAlign::Center,
            bg: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
            fg: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            radius: 6.0,
            focused: false,
            focus_visible: false,
            padding: [8.0, 16.0, 8.0, 16.0],
            icon_path: None,
            icon_size: 16.0,
            icon_spacing: 6.0,
            icon_only: false,
            focus_id: FocusId(0),
        }
    }

    /// Create a button that derives its colors from a [`Theme`].
    pub fn with_theme(label: impl Into<String>, theme: &Theme) -> Self {
        let mut btn = Self::new(label);
        btn.bg = theme.colors.button_bg;
        btn.fg = theme.colors.button_fg;
        btn.radius = theme.border_radius;
        btn.label_size = theme.font_size;
        btn
    }

    /// Hit-test: is `(x, y)` inside the button rect?
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

impl Element for Button {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        // Rounded background
        let rrect = RoundedRect {
            rect: self.rect,
            radii: RoundedRadii {
                tl: self.radius,
                tr: self.radius,
                br: self.radius,
                bl: self.radius,
            },
        };
        canvas.rounded_rect(rrect, Brush::Solid(self.bg), z);

        // Padding unpacking
        let pad_top = self.padding[0];
        let pad_right = self.padding[1];
        let pad_bottom = self.padding[2];
        let pad_left = self.padding[3];

        let trimmed_label = if self.icon_only {
            ""
        } else {
            self.label.trim()
        };

        let label_len = trimmed_label.chars().count() as f32;
        let approx_text_width = if label_len == 0.0 {
            0.0
        } else {
            canvas.measure_text_width(trimmed_label, self.label_size) + 2.0
        };

        let content_w = (self.rect.w - pad_left - pad_right).max(0.0);
        let content_h = (self.rect.h - pad_top - pad_bottom).max(0.0);
        let base_x = self.rect.x + pad_left;

        let has_icon = self.icon_path.is_some() && self.icon_size > 0.0;
        let icon_w = if has_icon { self.icon_size } else { 0.0 };
        let icon_spacing = if has_icon {
            self.icon_spacing.max(0.0)
        } else {
            0.0
        };
        let combined_width = icon_w + icon_spacing + approx_text_width;

        let origin_x = match self.label_align {
            ButtonLabelAlign::Center => base_x + (content_w - combined_width).max(0.0) * 0.5,
            ButtonLabelAlign::End => base_x + (content_w - combined_width).max(0.0),
            ButtonLabelAlign::Start => base_x,
        };

        let text_x = if has_icon {
            origin_x + icon_w + icon_spacing
        } else {
            origin_x
        };

        let content_center_y = self.rect.y + pad_top + content_h * 0.5;
        let text_y = content_center_y + self.label_size * 0.35;

        // Draw label (skip icon rendering since jag-ui is standalone and does
        // not have asset resolution; icon support is provided as data fields
        // for higher-level integrations to use).
        if !self.icon_only {
            canvas.draw_text_run_weighted(
                [text_x, text_y],
                trimmed_label.to_string(),
                self.label_size,
                400.0,
                self.fg,
                z + 2,
            );
        }

        // Focus ring
        if self.focused && self.focus_visible {
            let focus_color = ColorLinPremul::from_srgba_u8([59, 130, 246, 180]);
            let offset = 3.0;
            let focus_rect = Rect {
                x: self.rect.x - offset,
                y: self.rect.y - offset,
                w: self.rect.w + offset * 2.0,
                h: self.rect.h + offset * 2.0,
            };
            let focus_rrect = RoundedRect {
                rect: focus_rect,
                radii: RoundedRadii {
                    tl: self.radius + offset,
                    tr: self.radius + offset,
                    br: self.radius + offset,
                    bl: self.radius + offset,
                },
            };
            jag_surface::shapes::draw_snapped_rounded_rectangle(
                canvas,
                focus_rrect,
                None,
                Some(2.0),
                Some(Brush::Solid(focus_color)),
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

impl EventHandler for Button {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if event.button != MouseButton::Left || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        if self.contains_point(event.x, event.y) {
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
            KeyCode::Space | KeyCode::Enter => EventResult::Handled,
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
    fn button_new_defaults() {
        let btn = Button::new("Click me");
        assert_eq!(btn.label, "Click me");
        assert!(!btn.focused);
        assert!(!btn.focus_visible);
        assert!(!btn.icon_only);
        assert!(btn.icon_path.is_none());
    }

    #[test]
    fn button_hit_test() {
        let mut btn = Button::new("Test");
        btn.set_rect(Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 40.0,
        });
        assert!(btn.contains_point(50.0, 30.0));
        assert!(!btn.contains_point(0.0, 0.0));
        assert!(btn.contains_point(10.0, 10.0)); // edge
        assert!(btn.contains_point(110.0, 50.0)); // opposite edge
        assert!(!btn.contains_point(111.0, 30.0)); // just outside
    }

    #[test]
    fn button_focus() {
        let mut btn = Button::new("Focus");
        assert!(!btn.is_focused());
        btn.set_focused(true);
        assert!(btn.is_focused());
        btn.set_focused(false);
        assert!(!btn.is_focused());
    }

    #[test]
    fn button_with_theme() {
        let theme = Theme::dark();
        let btn = Button::with_theme("Themed", &theme);
        assert_eq!(btn.bg, theme.colors.button_bg);
        assert_eq!(btn.fg, theme.colors.button_fg);
        assert_eq!(btn.radius, theme.border_radius);
        assert_eq!(btn.label_size, theme.font_size);
    }

    #[test]
    fn button_element_trait() {
        let mut btn = Button::new("Elem");
        let r = Rect {
            x: 5.0,
            y: 5.0,
            w: 80.0,
            h: 30.0,
        };
        btn.set_rect(r);
        assert_eq!(btn.rect().x, 5.0);
        assert_eq!(btn.rect().w, 80.0);
        assert!(btn.focus_id().is_some());
    }

    #[test]
    fn button_keyboard_requires_focus() {
        let mut btn = Button::new("KB");
        btn.focused = false;
        let evt = KeyboardEvent {
            key: KeyCode::Enter,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert_eq!(btn.handle_keyboard(&evt), EventResult::Ignored);

        btn.focused = true;
        assert_eq!(btn.handle_keyboard(&evt), EventResult::Handled);
    }
}
