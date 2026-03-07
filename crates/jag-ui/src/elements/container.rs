//! Scrollable container element.

use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::KeyboardEvent;
use crate::event::{EventHandler, EventResult, MouseClickEvent, MouseMoveEvent, ScrollEvent};
use crate::focus::FocusId;

use super::Element;

/// A container that can hold children, with optional background, border,
/// padding, and scroll support.
///
/// Containers are not focusable. They handle scroll events to update
/// their internal scroll offset, and provide a `content_rect()` for
/// callers to position children.
pub struct Container {
    pub rect: Rect,
    /// Background color (transparent by default).
    pub bg: Option<ColorLinPremul>,
    /// Border color.
    pub border_color: ColorLinPremul,
    /// Border width in logical pixels (0 = no border).
    pub border_width: f32,
    /// Corner radius for background and border.
    pub radius: f32,
    /// Padding: [top, right, bottom, left].
    pub padding: [f32; 4],
    /// Current horizontal scroll offset.
    pub scroll_x: f32,
    /// Current vertical scroll offset.
    pub scroll_y: f32,
    /// Total content width (may exceed rect width).
    pub content_width: f32,
    /// Total content height (may exceed rect height).
    pub content_height: f32,
}

impl Container {
    /// Create a container with default styling.
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            bg: None,
            border_color: ColorLinPremul::from_srgba_u8([200, 200, 200, 255]),
            border_width: 0.0,
            radius: 0.0,
            padding: [0.0; 4],
            scroll_x: 0.0,
            scroll_y: 0.0,
            content_width: 0.0,
            content_height: 0.0,
        }
    }

    /// The inner content rectangle (rect minus padding).
    pub fn content_rect(&self) -> Rect {
        let pad_top = self.padding[0];
        let pad_right = self.padding[1];
        let pad_bottom = self.padding[2];
        let pad_left = self.padding[3];
        Rect {
            x: self.rect.x + pad_left,
            y: self.rect.y + pad_top,
            w: (self.rect.w - pad_left - pad_right).max(0.0),
            h: (self.rect.h - pad_top - pad_bottom).max(0.0),
        }
    }

    /// Maximum horizontal scroll value (zero when content fits).
    pub fn max_scroll_x(&self) -> f32 {
        let inner_w = self.content_rect().w;
        (self.content_width - inner_w).max(0.0)
    }

    /// Maximum vertical scroll value (zero when content fits).
    pub fn max_scroll_y(&self) -> f32 {
        let inner_h = self.content_rect().h;
        (self.content_height - inner_h).max(0.0)
    }

    /// Clamp scroll offsets to valid ranges.
    pub fn clamp_scroll(&mut self) {
        self.scroll_x = self.scroll_x.clamp(0.0, self.max_scroll_x());
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll_y());
    }

    /// Hit-test: is `(x, y)` inside the container rect?
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

impl Element for Container {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        // Background
        if let Some(bg) = self.bg {
            if self.radius > 0.0 {
                let rrect = RoundedRect {
                    rect: self.rect,
                    radii: RoundedRadii {
                        tl: self.radius,
                        tr: self.radius,
                        br: self.radius,
                        bl: self.radius,
                    },
                };
                canvas.rounded_rect(rrect, Brush::Solid(bg), z);
            } else {
                canvas.fill_rect(
                    self.rect.x,
                    self.rect.y,
                    self.rect.w,
                    self.rect.h,
                    Brush::Solid(bg),
                    z,
                );
            }
        }

        // Border
        if self.border_width > 0.0 {
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
                None,
                Some(self.border_width),
                Some(Brush::Solid(self.border_color)),
                z + 1,
            );
        }
    }

    /// Containers are not focusable.
    fn focus_id(&self) -> Option<FocusId> {
        None
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait
// ---------------------------------------------------------------------------

impl EventHandler for Container {
    fn handle_mouse_click(&mut self, _event: &MouseClickEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_keyboard(&mut self, _event: &KeyboardEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_mouse_move(&mut self, _event: &MouseMoveEvent) -> EventResult {
        EventResult::Ignored
    }

    fn handle_scroll(&mut self, event: &ScrollEvent) -> EventResult {
        if !self.hit_test(event.x, event.y) {
            return EventResult::Ignored;
        }
        self.scroll_x += event.delta_x;
        self.scroll_y += event.delta_y;
        self.clamp_scroll();
        EventResult::Handled
    }

    fn is_focused(&self) -> bool {
        false
    }

    fn set_focused(&mut self, _focused: bool) {
        // Containers are not focusable.
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
    fn container_content_rect_with_padding() {
        let mut c = Container::new(Rect {
            x: 10.0,
            y: 20.0,
            w: 200.0,
            h: 100.0,
        });
        c.padding = [5.0, 10.0, 5.0, 10.0];
        let cr = c.content_rect();
        assert!((cr.x - 20.0).abs() < f32::EPSILON);
        assert!((cr.y - 25.0).abs() < f32::EPSILON);
        assert!((cr.w - 180.0).abs() < f32::EPSILON);
        assert!((cr.h - 90.0).abs() < f32::EPSILON);
    }

    #[test]
    fn container_scroll_clamp() {
        let mut c = Container::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        c.content_width = 200.0;
        c.content_height = 300.0;
        c.scroll_x = 500.0;
        c.scroll_y = 500.0;
        c.clamp_scroll();
        assert!((c.scroll_x - 100.0).abs() < f32::EPSILON);
        assert!((c.scroll_y - 200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn container_not_focusable() {
        let c = Container::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 50.0,
            h: 50.0,
        });
        assert!(c.focus_id().is_none());
        assert!(!c.is_focused());
    }

    #[test]
    fn container_hit_test() {
        let c = Container::new(Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 80.0,
        });
        assert!(c.hit_test(50.0, 50.0));
        assert!(!c.hit_test(0.0, 0.0));
    }

    #[test]
    fn container_scroll_event_handling() {
        let mut c = Container::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        c.content_height = 300.0;
        let evt = ScrollEvent {
            x: 50.0,
            y: 50.0,
            delta_x: 0.0,
            delta_y: 30.0,
        };
        let result = c.handle_scroll(&evt);
        assert_eq!(result, EventResult::Handled);
        assert!((c.scroll_y - 30.0).abs() < f32::EPSILON);
    }
}
