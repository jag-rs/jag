//! Modal dialog overlay element.

use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// Result of a click on the modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalClickResult {
    /// The close button was clicked.
    CloseButton,
    /// A modal button was clicked (index into `buttons`).
    Button(usize),
    /// The background scrim was clicked.
    Background,
    /// Somewhere on the panel body was clicked.
    Panel,
}

/// Configuration for a modal action button.
#[derive(Debug, Clone)]
pub struct ModalButton {
    pub label: String,
    pub primary: bool,
}

impl ModalButton {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            primary: false,
        }
    }

    pub fn primary(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            primary: true,
        }
    }
}

/// An overlay dialog centered on the screen with title, content, and
/// action buttons.
pub struct Modal {
    /// Overall bounding rect (typically the full viewport).
    pub rect: Rect,
    /// Panel width.
    pub panel_width: f32,
    /// Panel height.
    pub panel_height: f32,
    /// Title text.
    pub title: String,
    /// Content/body text (newlines supported).
    pub content: String,
    /// Action buttons rendered at the bottom of the panel.
    pub buttons: Vec<ModalButton>,
    /// Semi-transparent overlay color.
    pub overlay_color: ColorLinPremul,
    /// Panel background.
    pub panel_bg: ColorLinPremul,
    /// Panel border color.
    pub panel_border_color: ColorLinPremul,
    /// Title text color.
    pub title_color: ColorLinPremul,
    /// Content text color.
    pub content_color: ColorLinPremul,
    /// Title font size.
    pub title_size: f32,
    /// Content font size.
    pub content_size: f32,
    /// Button label font size.
    pub button_label_size: f32,
    /// Panel corner radius.
    pub panel_radius: f32,
    /// Whether the modal is currently visible.
    pub visible: bool,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl Modal {
    /// Create a modal with default styling.
    pub fn new(
        viewport: Rect,
        title: impl Into<String>,
        content: impl Into<String>,
        buttons: Vec<ModalButton>,
    ) -> Self {
        Self {
            rect: viewport,
            panel_width: 480.0,
            panel_height: 300.0,
            title: title.into(),
            content: content.into(),
            buttons,
            overlay_color: ColorLinPremul::from_srgba_u8([0, 0, 0, 140]),
            panel_bg: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            panel_border_color: ColorLinPremul::from_srgba_u8([200, 200, 200, 255]),
            title_color: ColorLinPremul::from_srgba_u8([20, 20, 20, 255]),
            content_color: ColorLinPremul::from_srgba_u8([60, 60, 60, 255]),
            title_size: 20.0,
            content_size: 14.0,
            button_label_size: 14.0,
            panel_radius: 8.0,
            visible: true,
            focus_id: FocusId(0),
        }
    }

    /// Compute the centered panel rectangle.
    pub fn panel_rect(&self) -> Rect {
        Rect {
            x: self.rect.x + (self.rect.w - self.panel_width) * 0.5,
            y: self.rect.y + (self.rect.h - self.panel_height) * 0.5,
            w: self.panel_width,
            h: self.panel_height,
        }
    }

    /// Close button rectangle (top-right of panel).
    pub fn close_button_rect(&self) -> Rect {
        let panel = self.panel_rect();
        let size = 32.0;
        Rect {
            x: panel.x + panel.w - size - 8.0,
            y: panel.y + 8.0,
            w: size,
            h: size,
        }
    }

    /// Compute rectangles for all action buttons (centered at bottom).
    pub fn button_rects(&self) -> Vec<Rect> {
        let panel = self.panel_rect();
        let btn_h = 36.0;
        let btn_w = 100.0;
        let spacing = 12.0;
        let n = self.buttons.len();
        if n == 0 {
            return vec![];
        }
        let total_w = btn_w * n as f32 + spacing * (n - 1) as f32;
        let start_x = panel.x + (panel.w - total_w) * 0.5;
        let y = panel.y + panel.h - 20.0 - btn_h;
        (0..n)
            .map(|i| Rect {
                x: start_x + (btn_w + spacing) * i as f32,
                y,
                w: btn_w,
                h: btn_h,
            })
            .collect()
    }

    /// Handle a click and return what was hit.
    pub fn handle_click(&self, x: f32, y: f32) -> ModalClickResult {
        let panel = self.panel_rect();
        let in_panel =
            x >= panel.x && x <= panel.x + panel.w && y >= panel.y && y <= panel.y + panel.h;

        if !in_panel {
            return ModalClickResult::Background;
        }

        // Close button
        let close = self.close_button_rect();
        if x >= close.x && x <= close.x + close.w && y >= close.y && y <= close.y + close.h {
            return ModalClickResult::CloseButton;
        }

        // Action buttons
        for (i, r) in self.button_rects().iter().enumerate() {
            if x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h {
                return ModalClickResult::Button(i);
            }
        }

        ModalClickResult::Panel
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Modal {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        if !self.visible {
            return;
        }

        // 1. Overlay scrim
        canvas.fill_rect(
            self.rect.x,
            self.rect.y,
            self.rect.w,
            self.rect.h,
            Brush::Solid(self.overlay_color),
            z,
        );

        // 2. Panel
        let panel = self.panel_rect();
        let rrect = RoundedRect {
            rect: panel,
            radii: RoundedRadii {
                tl: self.panel_radius,
                tr: self.panel_radius,
                br: self.panel_radius,
                bl: self.panel_radius,
            },
        };
        jag_surface::shapes::draw_snapped_rounded_rectangle(
            canvas,
            rrect,
            Some(Brush::Solid(self.panel_bg)),
            Some(1.0),
            Some(Brush::Solid(self.panel_border_color)),
            z + 1,
        );

        // 3. Close button "X"
        let close = self.close_button_rect();
        let x_text_x = close.x + close.w * 0.5 - 4.0;
        let x_text_y = close.y + close.h * 0.5 + 5.0;
        canvas.draw_text_run_weighted(
            [x_text_x, x_text_y],
            "\u{2715}".to_string(),
            14.0,
            400.0,
            ColorLinPremul::from_srgba_u8([100, 100, 100, 255]),
            z + 3,
        );

        // 4. Title
        canvas.draw_text_run_weighted(
            [panel.x + 20.0, panel.y + 30.0],
            self.title.clone(),
            self.title_size,
            600.0,
            self.title_color,
            z + 2,
        );

        // 5. Content (multi-line)
        let line_height = self.content_size * 1.4;
        let content_y = panel.y + 70.0;
        for (i, line) in self.content.split('\n').enumerate() {
            canvas.draw_text_run_weighted(
                [panel.x + 20.0, content_y + i as f32 * line_height],
                line.to_string(),
                self.content_size,
                400.0,
                self.content_color,
                z + 2,
            );
        }

        // 6. Buttons
        for (i, (button, btn_rect)) in self.buttons.iter().zip(self.button_rects()).enumerate() {
            let (bg, fg) = if button.primary {
                (
                    ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
                    ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
                )
            } else {
                (
                    ColorLinPremul::from_srgba_u8([240, 240, 240, 255]),
                    ColorLinPremul::from_srgba_u8([60, 60, 60, 255]),
                )
            };

            let btn_rrect = RoundedRect {
                rect: btn_rect,
                radii: RoundedRadii {
                    tl: 6.0,
                    tr: 6.0,
                    br: 6.0,
                    bl: 6.0,
                },
            };
            canvas.rounded_rect(btn_rrect, Brush::Solid(bg), z + 3 + i as i32);

            let text_w = button.label.len() as f32 * self.button_label_size * 0.5;
            let tx = btn_rect.x + (btn_rect.w - text_w) * 0.5;
            let ty = btn_rect.y + btn_rect.h * 0.5 + self.button_label_size * 0.35;
            canvas.draw_text_run_weighted(
                [tx, ty],
                button.label.clone(),
                self.button_label_size,
                600.0,
                fg,
                z + 4 + i as i32,
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

impl EventHandler for Modal {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if !self.visible {
            return EventResult::Ignored;
        }
        if event.button != MouseButton::Left || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        // Modal captures all clicks when visible.
        let _result = self.handle_click(event.x, event.y);
        EventResult::Handled
    }

    fn handle_keyboard(&mut self, event: &KeyboardEvent) -> EventResult {
        if !self.visible || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        match event.key {
            KeyCode::Escape => EventResult::Handled,
            KeyCode::Enter => {
                if self.buttons.iter().any(|b| b.primary) {
                    EventResult::Handled
                } else {
                    EventResult::Ignored
                }
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse_move(&mut self, _event: &MouseMoveEvent) -> EventResult {
        if self.visible {
            EventResult::Handled
        } else {
            EventResult::Ignored
        }
    }

    fn handle_scroll(&mut self, _event: &ScrollEvent) -> EventResult {
        if self.visible {
            EventResult::Handled
        } else {
            EventResult::Ignored
        }
    }

    fn is_focused(&self) -> bool {
        self.visible
    }

    fn set_focused(&mut self, _focused: bool) {}

    fn contains_point(&self, _x: f32, _y: f32) -> bool {
        // Modal captures all input when visible.
        self.visible
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        }
    }

    #[test]
    fn modal_panel_centered() {
        let m = Modal::new(viewport(), "Title", "Body", vec![]);
        let p = m.panel_rect();
        let cx = p.x + p.w * 0.5;
        let cy = p.y + p.h * 0.5;
        assert!((cx - 400.0).abs() < 1.0);
        assert!((cy - 300.0).abs() < 1.0);
    }

    #[test]
    fn modal_click_background() {
        let m = Modal::new(viewport(), "T", "C", vec![]);
        let result = m.handle_click(0.0, 0.0);
        assert_eq!(result, ModalClickResult::Background);
    }

    #[test]
    fn modal_click_close_button() {
        let m = Modal::new(viewport(), "T", "C", vec![]);
        let close = m.close_button_rect();
        let result = m.handle_click(close.x + 5.0, close.y + 5.0);
        assert_eq!(result, ModalClickResult::CloseButton);
    }

    #[test]
    fn modal_click_action_button() {
        let m = Modal::new(
            viewport(),
            "T",
            "C",
            vec![ModalButton::new("Cancel"), ModalButton::primary("OK")],
        );
        let rects = m.button_rects();
        assert_eq!(rects.len(), 2);
        let r = rects[1];
        let result = m.handle_click(r.x + 5.0, r.y + 5.0);
        assert_eq!(result, ModalClickResult::Button(1));
    }

    #[test]
    fn modal_captures_input_when_visible() {
        let m = Modal::new(viewport(), "T", "C", vec![]);
        assert!(m.contains_point(0.0, 0.0));
    }

    #[test]
    fn modal_ignores_when_hidden() {
        let mut m = Modal::new(viewport(), "T", "C", vec![]);
        m.visible = false;
        assert!(!m.contains_point(400.0, 300.0));
    }

    #[test]
    fn modal_escape_handled() {
        let mut m = Modal::new(viewport(), "T", "C", vec![]);
        let evt = KeyboardEvent {
            key: KeyCode::Escape,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert_eq!(m.handle_keyboard(&evt), EventResult::Handled);
    }
}
