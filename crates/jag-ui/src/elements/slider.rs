//! Horizontal slider element with draggable thumb.

use jag_draw::{Brush, Color, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    ElementState, EventHandler, EventResult, KeyCode, KeyboardEvent, MouseButton, MouseClickEvent,
    MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// A horizontal slider with a track and draggable thumb.
pub struct Slider {
    /// Bounding rect of the slider track.
    pub rect: Rect,
    /// Current value.
    pub value: f32,
    /// Minimum value.
    pub min: f32,
    /// Maximum value.
    pub max: f32,
    /// Step increment for keyboard/discrete adjustments.
    pub step: f32,
    /// Whether this slider is focused.
    pub focused: bool,
    /// Whether the thumb is currently being dragged.
    pub dragging: bool,
    /// Track color (unfilled portion).
    pub track_color: ColorLinPremul,
    /// Filled track color (from min to current value).
    pub fill_color: ColorLinPremul,
    /// Thumb color.
    pub thumb_color: ColorLinPremul,
    /// Track height.
    pub track_height: f32,
    /// Thumb radius.
    pub thumb_radius: f32,
    /// Focus identifier.
    pub focus_id: FocusId,
}

impl Slider {
    /// Create a slider with default range [0, 100] and value 0.
    pub fn new() -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 24.0,
            },
            value: 0.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            focused: false,
            dragging: false,
            track_color: ColorLinPremul::from_srgba_u8([80, 80, 80, 255]),
            fill_color: ColorLinPremul::from_srgba_u8([59, 130, 246, 255]),
            thumb_color: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            track_height: 4.0,
            thumb_radius: 8.0,
            focus_id: FocusId(0),
        }
    }

    /// Normalized position of the value between 0.0 and 1.0.
    pub fn normalized(&self) -> f32 {
        if (self.max - self.min).abs() < f32::EPSILON {
            return 0.0;
        }
        ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// Set the value, clamping to [min, max].
    pub fn set_value(&mut self, value: f32) {
        self.value = value.clamp(self.min, self.max);
    }

    /// X coordinate of the thumb center.
    fn thumb_x(&self) -> f32 {
        let usable_width = self.rect.w - self.thumb_radius * 2.0;
        self.rect.x + self.thumb_radius + usable_width * self.normalized()
    }

    /// Y coordinate of the thumb center.
    fn thumb_y(&self) -> f32 {
        self.rect.y + self.rect.h * 0.5
    }

    /// Hit-test the thumb circle.
    pub fn hit_test_thumb(&self, x: f32, y: f32) -> bool {
        let dx = x - self.thumb_x();
        let dy = y - self.thumb_y();
        dx * dx + dy * dy <= self.thumb_radius * self.thumb_radius
    }

    /// Hit-test the track area.
    pub fn hit_test_track(&self, x: f32, y: f32) -> bool {
        let track_y = self.rect.y + (self.rect.h - self.track_height) * 0.5;
        x >= self.rect.x
            && x <= self.rect.x + self.rect.w
            && y >= track_y
            && y <= track_y + self.track_height
    }

    /// Convert an x coordinate to a value.
    fn x_to_value(&self, x: f32) -> f32 {
        let usable_width = self.rect.w - self.thumb_radius * 2.0;
        if usable_width <= 0.0 {
            return self.min;
        }
        let norm = ((x - self.rect.x - self.thumb_radius) / usable_width).clamp(0.0, 1.0);
        self.min + norm * (self.max - self.min)
    }
}

impl Default for Slider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Element trait
// ---------------------------------------------------------------------------

impl Element for Slider {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        let track_y = self.rect.y + (self.rect.h - self.track_height) * 0.5;
        let track_r = self.track_height * 0.5;

        // Background track
        let track_rrect = RoundedRect {
            rect: Rect {
                x: self.rect.x,
                y: track_y,
                w: self.rect.w,
                h: self.track_height,
            },
            radii: RoundedRadii {
                tl: track_r,
                tr: track_r,
                br: track_r,
                bl: track_r,
            },
        };
        canvas.rounded_rect(track_rrect, Brush::Solid(self.track_color), z);

        // Filled portion
        let fill_width = self.thumb_x() - self.rect.x;
        if fill_width > 0.0 {
            let fill_rrect = RoundedRect {
                rect: Rect {
                    x: self.rect.x,
                    y: track_y,
                    w: fill_width,
                    h: self.track_height,
                },
                radii: RoundedRadii {
                    tl: track_r,
                    tr: track_r,
                    br: track_r,
                    bl: track_r,
                },
            };
            canvas.rounded_rect(fill_rrect, Brush::Solid(self.fill_color), z + 1);
        }

        // Thumb
        let tx = self.thumb_x();
        let ty = self.thumb_y();
        canvas.ellipse(
            [tx, ty],
            [self.thumb_radius, self.thumb_radius],
            Brush::Solid(self.thumb_color),
            z + 2,
        );

        // Focus ring around thumb
        if self.focused {
            let focus_r = self.thumb_radius + 3.0;
            jag_surface::shapes::draw_ellipse(
                canvas,
                [tx, ty],
                [focus_r, focus_r],
                None,
                Some(2.0),
                Some(Brush::Solid(Color::rgba(63, 130, 246, 255))),
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

impl EventHandler for Slider {
    fn handle_mouse_click(&mut self, event: &MouseClickEvent) -> EventResult {
        if event.button != MouseButton::Left {
            return EventResult::Ignored;
        }
        match event.state {
            ElementState::Pressed => {
                if self.hit_test_thumb(event.x, event.y) {
                    self.dragging = true;
                    EventResult::Handled
                } else if self.hit_test_track(event.x, event.y) {
                    self.set_value(self.x_to_value(event.x));
                    self.dragging = true;
                    EventResult::Handled
                } else {
                    EventResult::Ignored
                }
            }
            ElementState::Released => {
                if self.dragging {
                    self.dragging = false;
                    EventResult::Handled
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    fn handle_keyboard(&mut self, event: &KeyboardEvent) -> EventResult {
        if event.state != ElementState::Pressed || !self.focused {
            return EventResult::Ignored;
        }
        match event.key {
            KeyCode::ArrowRight | KeyCode::ArrowUp => {
                self.set_value(self.value + self.step);
                EventResult::Handled
            }
            KeyCode::ArrowLeft | KeyCode::ArrowDown => {
                self.set_value(self.value - self.step);
                EventResult::Handled
            }
            KeyCode::Home => {
                self.set_value(self.min);
                EventResult::Handled
            }
            KeyCode::End => {
                self.set_value(self.max);
                EventResult::Handled
            }
            _ => EventResult::Ignored,
        }
    }

    fn handle_mouse_move(&mut self, event: &MouseMoveEvent) -> EventResult {
        if self.dragging {
            self.set_value(self.x_to_value(event.x));
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
        self.hit_test_thumb(x, y) || self.hit_test_track(x, y)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_new_defaults() {
        let s = Slider::new();
        assert_eq!(s.value, 0.0);
        assert_eq!(s.min, 0.0);
        assert_eq!(s.max, 100.0);
        assert!(!s.focused);
        assert!(!s.dragging);
    }

    #[test]
    fn slider_normalized() {
        let mut s = Slider::new();
        assert!((s.normalized() - 0.0).abs() < f32::EPSILON);
        s.value = 50.0;
        assert!((s.normalized() - 0.5).abs() < f32::EPSILON);
        s.value = 100.0;
        assert!((s.normalized() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn slider_set_value_clamped() {
        let mut s = Slider::new();
        s.set_value(150.0);
        assert_eq!(s.value, 100.0);
        s.set_value(-10.0);
        assert_eq!(s.value, 0.0);
    }

    #[test]
    fn slider_keyboard_step() {
        let mut s = Slider::new();
        s.focused = true;
        s.step = 10.0;
        s.value = 50.0;
        let right = KeyboardEvent {
            key: KeyCode::ArrowRight,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert_eq!(s.handle_keyboard(&right), EventResult::Handled);
        assert_eq!(s.value, 60.0);

        let left = KeyboardEvent {
            key: KeyCode::ArrowLeft,
            state: ElementState::Pressed,
            modifiers: Default::default(),
            text: None,
        };
        assert_eq!(s.handle_keyboard(&left), EventResult::Handled);
        assert_eq!(s.value, 50.0);
    }

    #[test]
    fn slider_focus() {
        let mut s = Slider::new();
        assert!(!s.is_focused());
        s.set_focused(true);
        assert!(s.is_focused());
    }
}
