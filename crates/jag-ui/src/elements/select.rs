//! Dropdown select element with options list.

use jag_draw::{Brush, Color, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A dropdown select element with an options list.
#[derive(Clone)]
pub struct Select {
    /// Bounding rect of the closed select field.
    pub rect: Rect,
    /// Currently displayed label text.
    pub label: String,
    /// Placeholder text shown when no option is selected.
    pub placeholder: String,
    /// Label font size.
    pub label_size: f32,
    /// Label text color.
    pub label_color: ColorLinPremul,
    /// Placeholder text color.
    pub placeholder_color: ColorLinPremul,
    /// Whether the placeholder is currently shown.
    pub is_placeholder: bool,
    /// Whether the dropdown is open.
    pub open: bool,
    /// Whether this select is focused.
    pub focused: bool,
    /// List of option strings.
    pub options: Vec<String>,
    /// Index of the currently selected option.
    pub selected_index: Option<usize>,
    /// Padding values.
    pub padding: [f32; 4],
    /// Background color.
    pub bg_color: ColorLinPremul,
    /// Border color.
    pub border_color: ColorLinPremul,
    /// Border width.
    pub border_width: f32,
    /// Corner radius.
    pub radius: f32,
    /// Validation error message.
    pub validation_error: Option<String>,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl Select {
    /// Height of each option row in the dropdown overlay.
    const OPTION_HEIGHT: f32 = 36.0;
    /// Internal padding in the overlay.
    const OVERLAY_PADDING: f32 = 4.0;

    /// Create a select with sensible defaults.
    pub fn new(placeholder: impl Into<String>) -> Self {
        let ph = placeholder.into();
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 36.0,
            },
            label: ph.clone(),
            placeholder: ph,
            label_size: 14.0,
            label_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            placeholder_color: ColorLinPremul::from_srgba_u8([160, 160, 160, 255]),
            is_placeholder: true,
            open: false,
            focused: false,
            options: Vec::new(),
            selected_index: None,
            padding: [8.0, 12.0, 8.0, 12.0],
            bg_color: ColorLinPremul::from_srgba_u8([40, 40, 40, 255]),
            border_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            border_width: 1.0,
            radius: 6.0,
            validation_error: None,
            focus_id: FocusId(0),
        }
    }

    /// Toggle the dropdown open/closed.
    pub fn toggle_open(&mut self) {
        self.open = !self.open;
    }

    /// Close the dropdown.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Get the overlay bounds (if open).
    pub fn get_overlay_bounds(&self) -> Option<Rect> {
        if !self.open || self.options.is_empty() {
            return None;
        }
        let overlay_height =
            (self.options.len() as f32 * Self::OPTION_HEIGHT) + (Self::OVERLAY_PADDING * 2.0);
        Some(Rect {
            x: self.rect.x,
            y: self.rect.y + self.rect.h + 4.0,
            w: self.rect.w,
            h: overlay_height,
        })
    }

    /// Get the selected option text.
    pub fn selected_option(&self) -> Option<&String> {
        self.selected_index.and_then(|idx| self.options.get(idx))
    }

    /// Set the selected index and update the label.
    pub fn set_selected_index(&mut self, index: Option<usize>) {
        self.selected_index = index;
        if let Some(idx) = index {
            if idx < self.options.len() {
                self.label = self.options[idx].clone();
                self.is_placeholder = false;
            }
        } else {
            self.label = self.placeholder.clone();
            self.is_placeholder = true;
        }
    }

    /// Handle click on the field (toggle dropdown).
    fn handle_field_click(&mut self, x: f32, y: f32) -> bool {
        if x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
        {
            self.toggle_open();
            true
        } else {
            false
        }
    }

    /// Handle click on the overlay options.
    fn handle_overlay_click(&mut self, x: f32, y: f32) -> bool {
        if !self.open || self.options.is_empty() {
            return false;
        }
        let overlay_bounds = match self.get_overlay_bounds() {
            Some(b) => b,
            None => return false,
        };

        if x < overlay_bounds.x
            || x > overlay_bounds.x + overlay_bounds.w
            || y < overlay_bounds.y
            || y > overlay_bounds.y + overlay_bounds.h
        {
            return false;
        }

        let local_y = y - overlay_bounds.y - Self::OVERLAY_PADDING;
        if local_y >= 0.0 {
            let idx = (local_y / Self::OPTION_HEIGHT) as usize;
            if idx < self.options.len() {
                self.selected_index = Some(idx);
                self.label = self.options[idx].clone();
                self.is_placeholder = false;
                self.open = false;
                return true;
            }
        }
        false
    }

    /// Render the dropdown overlay.
    fn render_dropdown_overlay(&self, canvas: &mut Canvas, z: i32) {
        let overlay_bounds = match self.get_overlay_bounds() {
            Some(b) => b,
            None => return,
        };

        let radius = 6.0;
        let overlay_rrect = RoundedRect {
            rect: overlay_bounds,
            radii: RoundedRadii {
                tl: radius,
                tr: radius,
                br: radius,
                bl: radius,
            },
        };

        // Background
        let overlay_bg = Color::rgba(255, 255, 255, 255);
        canvas.rounded_rect(overlay_rrect, Brush::Solid(overlay_bg), z);

        // Border
        jag_surface::shapes::draw_rounded_rectangle(
            canvas,
            overlay_rrect,
            None,
            Some(1.0),
            Some(Brush::Solid(self.border_color)),
            z + 1,
        );

        // Options
        let pad_left = self.padding[3];
        for (idx, option) in self.options.iter().enumerate() {
            let option_y =
                overlay_bounds.y + Self::OVERLAY_PADDING + (idx as f32 * Self::OPTION_HEIGHT);

            let is_selected = self.selected_index == Some(idx);
            if is_selected {
                let highlight_bg = Color::rgba(220, 220, 224, 255);
                canvas.fill_rect(
                    overlay_bounds.x,
                    option_y,
                    overlay_bounds.w,
                    Self::OPTION_HEIGHT,
                    Brush::Solid(highlight_bg),
                    z + 2,
                );
            }

            let text_x = overlay_bounds.x + Self::OVERLAY_PADDING + pad_left;
            let text_y = option_y + Self::OPTION_HEIGHT * 0.5 + self.label_size * 0.35;
            let text_color = if is_selected {
                Color::rgba(20, 24, 30, 255)
            } else {
                Color::rgba(34, 42, 52, 255)
            };

            canvas.draw_text_run_weighted(
                [text_x, text_y],
                option.clone(),
                self.label_size,
                400.0,
                text_color,
                z + 3,
            );
        }
    }
}

impl Default for Select {
    fn default() -> Self {
        Self::new("Select...")
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Select {
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
            self.border_width.max(1.0)
        };

        jag_surface::shapes::draw_snapped_rounded_rectangle(
            canvas,
            rrect,
            Some(Brush::Solid(self.bg_color)),
            Some(border_width),
            Some(Brush::Solid(border_color)),
            z,
        );

        // Label text
        let pad_top = self.padding[0];
        let pad_left = self.padding[3];
        let pad_bottom = self.padding[2];
        let content_h = (self.rect.h - pad_top - pad_bottom).max(0.0);
        let tp = [
            self.rect.x + pad_left,
            self.rect.y + pad_top + content_h * 0.5 + self.label_size * 0.35,
        ];
        let (text, color) = if self.is_placeholder {
            (&self.placeholder, self.placeholder_color)
        } else {
            (&self.label, self.label_color)
        };
        canvas.draw_text_run_weighted(tp, text.clone(), self.label_size, 400.0, color, z + 2);

        // Chevron indicator (simple text)
        let pad_right = self.padding[1];
        let chevron = if self.open { "\u{25B2}" } else { "\u{25BC}" };
        let chevron_x = self.rect.x + self.rect.w - pad_right - 12.0;
        let chevron_y = tp[1];
        canvas.draw_text_run_weighted(
            [chevron_x, chevron_y],
            chevron.to_string(),
            self.label_size * 0.7,
            400.0,
            self.label_color,
            z + 3,
        );

        // Dropdown overlay
        if self.open && !self.options.is_empty() {
            self.render_dropdown_overlay(canvas, z + 1000);
        }

        // Validation error
        if let Some(ref error_msg) = self.validation_error {
            let error_size = (self.label_size * 0.9).max(12.0);
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
                z + 5,
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

impl EventHandler for Select {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if event.button != MouseButton::Left || event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        if self.open && self.handle_overlay_click(event.x, event.y) {
            return EventResult::Handled;
        }
        if self.handle_field_click(event.x, event.y) {
            return EventResult::Handled;
        }
        EventResult::Ignored
    }

    fn handle_keyboard(&mut self, event: &KeyboardEvent) -> EventResult {
        if event.state != ElementState::Pressed {
            return EventResult::Ignored;
        }
        if !self.focused && !self.open {
            return EventResult::Ignored;
        }
        match event.key {
            KeyCode::ArrowDown => {
                if !self.open {
                    self.open = true;
                } else if !self.options.is_empty() {
                    let new_idx = match self.selected_index {
                        Some(idx) if idx + 1 < self.options.len() => idx + 1,
                        Some(idx) => idx,
                        None => 0,
                    };
                    self.set_selected_index(Some(new_idx));
                }
                EventResult::Handled
            }
            KeyCode::ArrowUp => {
                if self.open && !self.options.is_empty() {
                    let new_idx = match self.selected_index {
                        Some(idx) if idx > 0 => idx - 1,
                        Some(idx) => idx,
                        None => 0,
                    };
                    self.set_selected_index(Some(new_idx));
                    EventResult::Handled
                } else {
                    EventResult::Ignored
                }
            }
            KeyCode::Enter => {
                self.open = !self.open;
                EventResult::Handled
            }
            KeyCode::Escape => {
                if self.open {
                    self.open = false;
                    EventResult::Handled
                } else {
                    EventResult::Ignored
                }
            }
            KeyCode::Space => {
                self.toggle_open();
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
        // Field area
        if x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= self.rect.y
            && y <= self.rect.y + self.rect.h
        {
            return true;
        }
        // Overlay area (if open)
        if let Some(ob) = self.get_overlay_bounds()
            && x >= ob.x
            && x <= ob.x + ob.w
            && y >= ob.y
            && y <= ob.y + ob.h
        {
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_new_defaults() {
        let sel = Select::new("Choose...");
        assert_eq!(sel.placeholder, "Choose...");
        assert!(sel.is_placeholder);
        assert!(!sel.open);
        assert!(sel.selected_index.is_none());
    }

    #[test]
    fn select_set_selected_index() {
        let mut sel = Select::new("Pick");
        sel.options = vec!["A".into(), "B".into(), "C".into()];
        sel.set_selected_index(Some(1));
        assert_eq!(sel.selected_index, Some(1));
        assert_eq!(sel.label, "B");
        assert!(!sel.is_placeholder);

        sel.set_selected_index(None);
        assert_eq!(sel.label, "Pick");
        assert!(sel.is_placeholder);
    }

    #[test]
    fn select_toggle_open() {
        let mut sel = Select::new("Pick");
        assert!(!sel.open);
        sel.toggle_open();
        assert!(sel.open);
        sel.toggle_open();
        assert!(!sel.open);
    }

    #[test]
    fn select_contains_point_field() {
        let mut sel = Select::new("Pick");
        sel.rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 200.0,
            h: 36.0,
        };
        assert!(sel.contains_point(100.0, 25.0));
        assert!(!sel.contains_point(0.0, 0.0));
    }

    #[test]
    fn select_keyboard_open_close() {
        let mut sel = Select::new("Pick");
        sel.focused = true;
        sel.options = vec!["X".into()];

        let down = KeyboardEvent {
            key: KeyCode::ArrowDown,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert_eq!(sel.handle_keyboard(&down), EventResult::Handled);
        assert!(sel.open);

        let esc = KeyboardEvent {
            key: KeyCode::Escape,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert_eq!(sel.handle_keyboard(&esc), EventResult::Handled);
        assert!(!sel.open);
    }
}
