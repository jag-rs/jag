//! Hyperlink element with underline and click handling.

use jag_draw::{ColorLinPremul, FontStyle, Hyperlink, Rect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A clickable hyperlink rendered as underlined text.
pub struct Link {
    /// Display text.
    pub text: String,
    /// Baseline-left position in logical coordinates.
    pub pos: [f32; 2],
    /// Font size in logical pixels.
    pub size: f32,
    /// Text color.
    pub color: ColorLinPremul,
    /// Target URL.
    pub url: String,
    /// Font weight (e.g. 400.0 normal, 700.0 bold).
    pub weight: f32,
    /// Pre-measured text width for accurate hit-testing.
    pub measured_width: Option<f32>,
    /// Whether to show underline decoration.
    pub underline: bool,
    /// Custom underline color (defaults to text color if `None`).
    pub underline_color: Option<ColorLinPremul>,
    /// Optional font family override.
    pub font_family: Option<String>,
    /// Font style.
    pub font_style: FontStyle,
    /// Whether this link is focused.
    pub focused: bool,
    /// Whether this link is hovered.
    pub hovered: bool,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl Link {
    /// Create a new hyperlink with default styling (blue with underline).
    pub fn new(text: impl Into<String>, url: impl Into<String>, pos: [f32; 2], size: f32) -> Self {
        Self {
            text: text.into(),
            pos,
            size,
            color: ColorLinPremul::from_srgba_u8([0x00, 0x7a, 0xff, 0xff]),
            url: url.into(),
            weight: 400.0,
            measured_width: None,
            underline: true,
            underline_color: None,
            font_family: None,
            font_style: FontStyle::Normal,
            focused: false,
            hovered: false,
            focus_id: FocusId(0),
        }
    }

    /// Builder: set the text color.
    pub fn with_color(mut self, color: ColorLinPremul) -> Self {
        self.color = color;
        self
    }

    /// Builder: set the font weight.
    pub fn with_weight(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    /// Builder: set an explicit measured width.
    pub fn with_measured_width(mut self, measured_width: f32) -> Self {
        self.measured_width = Some(measured_width.max(0.0));
        self
    }

    /// Builder: enable or disable underline.
    pub fn with_underline(mut self, underline: bool) -> Self {
        self.underline = underline;
        self
    }

    /// Get the URL of this link.
    pub fn get_url(&self) -> &str {
        &self.url
    }

    /// Set the URL.
    pub fn set_url(&mut self, url: String) {
        self.url = url;
    }

    /// Approximate bounding box for hit-testing.
    fn get_bounds(&self) -> (f32, f32, f32, f32) {
        let char_width = self.size * 0.5;
        let text_width = self.text.len() as f32 * char_width;
        let text_height = self.size * 1.2;
        (
            self.pos[0],
            self.pos[1] - self.size * 0.8,
            text_width,
            text_height,
        )
    }

    /// Hit-test the link text area.
    pub fn hit_test(&self, x: f32, y: f32) -> bool {
        let (bx, by, bw, bh) = self.get_bounds();
        x >= bx && x <= bx + bw && y >= by && y <= by + bh
    }
}

impl Default for Link {
    fn default() -> Self {
        Self::new("Link", "https://example.com", [0.0, 0.0], 16.0)
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Link {
    fn rect(&self) -> Rect {
        let (bx, by, bw, bh) = self.get_bounds();
        Rect {
            x: bx,
            y: by,
            w: bw,
            h: bh,
        }
    }

    fn set_rect(&mut self, rect: Rect) {
        self.pos = [rect.x, rect.y + rect.h * 0.8];
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        let hyperlink = Hyperlink {
            text: self.text.clone(),
            pos: self.pos,
            size: self.size,
            color: self.color,
            url: self.url.clone(),
            weight: self.weight,
            measured_width: self.measured_width,
            underline: self.underline,
            underline_color: self.underline_color,
            family: self.font_family.clone(),
            style: self.font_style,
        };
        canvas.draw_hyperlink(hyperlink, z);
    }

    fn focus_id(&self) -> Option<FocusId> {
        Some(self.focus_id)
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait
// ---------------------------------------------------------------------------

impl EventHandler for Link {
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
        match event.key {
            KeyCode::Space | KeyCode::Enter => EventResult::Handled,
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent) -> EventResult {
        let was_hovered = self.hovered;
        self.hovered = self.hit_test(event.x, event.y);
        if was_hovered != self.hovered {
            EventResult::Handled
        } else {
            EventResult::Ignored
        }
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
    fn link_new_defaults() {
        let link = Link::new("Click", "https://example.com", [10.0, 20.0], 14.0);
        assert_eq!(link.text, "Click");
        assert_eq!(link.url, "https://example.com");
        assert!(link.underline);
        assert!(!link.focused);
        assert!(!link.hovered);
    }

    #[test]
    fn link_hit_test() {
        let link = Link::new("Hello", "https://example.com", [10.0, 20.0], 14.0);
        // bounds: x=10, y=20-14*0.8=8.8, w=5*7=35, h=14*1.2=16.8
        assert!(link.hit_test(15.0, 15.0));
        assert!(!link.hit_test(0.0, 0.0));
    }

    #[test]
    fn link_builder_methods() {
        let link = Link::new("Test", "http://test.com", [0.0, 0.0], 16.0)
            .with_weight(700.0)
            .with_underline(false)
            .with_measured_width(100.0);
        assert_eq!(link.weight, 700.0);
        assert!(!link.underline);
        assert_eq!(link.measured_width, Some(100.0));
    }

    #[test]
    fn link_get_set_url() {
        let mut link = Link::default();
        assert_eq!(link.get_url(), "https://example.com");
        link.set_url("https://other.com".to_string());
        assert_eq!(link.get_url(), "https://other.com");
    }

    #[test]
    fn link_hover_state() {
        let mut link = Link::new("Hover", "https://example.com", [10.0, 20.0], 14.0);
        assert!(!link.hovered);
        let evt = MouseMoveEvent { x: 15.0, y: 15.0 };
        let result = link.handle_mouse_move(&evt);
        assert_eq!(result, EventResult::Handled);
        assert!(link.hovered);
    }
}
