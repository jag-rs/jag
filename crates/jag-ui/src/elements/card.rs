//! Card element with optional title, shadow, and content slots.

use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect};
use jag_surface::Canvas;

use crate::event::{
    EventHandler, EventResult, KeyboardEvent, MouseClickEvent, MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// Layout information for a card's header and content areas.
#[derive(Debug, Clone, Copy)]
pub struct CardLayout {
    pub header: Rect,
    pub content: Rect,
}

/// A simple card with optional title, rounded background, and shadow.
pub struct Card {
    pub rect: Rect,
    /// Optional title displayed at the top of the card.
    pub title: Option<String>,
    /// Title font size.
    pub title_size: f32,
    /// Title text color.
    pub title_color: ColorLinPremul,
    /// Background fill color.
    pub bg: ColorLinPremul,
    /// Border color.
    pub border_color: ColorLinPremul,
    /// Border width (0 = no border).
    pub border_width: f32,
    /// Corner radius.
    pub radius: f32,
    /// Height reserved for the title/header area.
    pub header_height: f32,
    /// Whether to render a drop shadow.
    pub show_shadow: bool,
    /// Shadow color.
    pub shadow_color: ColorLinPremul,
    /// Shadow offset in pixels [x, y].
    pub shadow_offset: [f32; 2],
    /// Shadow spread (extra pixels in each direction).
    pub shadow_spread: f32,
}

impl Card {
    /// Create a card with default styling.
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            title: None,
            title_size: 16.0,
            title_color: ColorLinPremul::from_srgba_u8([20, 20, 20, 255]),
            bg: ColorLinPremul::from_srgba_u8([255, 255, 255, 255]),
            border_color: ColorLinPremul::from_srgba_u8([226, 232, 240, 255]),
            border_width: 1.0,
            radius: 8.0,
            header_height: 48.0,
            show_shadow: true,
            shadow_color: ColorLinPremul::from_srgba_u8([0, 0, 0, 30]),
            shadow_offset: [0.0, 2.0],
            shadow_spread: 4.0,
        }
    }

    /// Set the card title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Compute header and content rectangles inside the card.
    pub fn layout(&self) -> CardLayout {
        let has_title = self.title.is_some();
        let header_h = if has_title {
            self.header_height.clamp(0.0, self.rect.h)
        } else {
            0.0
        };
        let content_h = (self.rect.h - header_h).max(0.0);

        CardLayout {
            header: Rect {
                x: self.rect.x,
                y: self.rect.y,
                w: self.rect.w,
                h: header_h,
            },
            content: Rect {
                x: self.rect.x,
                y: self.rect.y + header_h,
                w: self.rect.w,
                h: content_h,
            },
        }
    }

    /// Hit-test: is `(x, y)` inside the card rect?
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

impl Element for Card {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        // Shadow (simple offset rectangle)
        if self.show_shadow {
            let shadow_rect = Rect {
                x: self.rect.x + self.shadow_offset[0] - self.shadow_spread,
                y: self.rect.y + self.shadow_offset[1] - self.shadow_spread,
                w: self.rect.w + self.shadow_spread * 2.0,
                h: self.rect.h + self.shadow_spread * 2.0,
            };
            let shadow_rrect = RoundedRect {
                rect: shadow_rect,
                radii: RoundedRadii {
                    tl: self.radius + self.shadow_spread,
                    tr: self.radius + self.shadow_spread,
                    br: self.radius + self.shadow_spread,
                    bl: self.radius + self.shadow_spread,
                },
            };
            canvas.rounded_rect(shadow_rrect, Brush::Solid(self.shadow_color), z);
        }

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

        let border_w = if self.border_width > 0.0 {
            Some(self.border_width)
        } else {
            None
        };
        let border_b = if self.border_width > 0.0 {
            Some(Brush::Solid(self.border_color))
        } else {
            None
        };

        jag_surface::shapes::draw_snapped_rounded_rectangle(
            canvas,
            rrect,
            Some(Brush::Solid(self.bg)),
            border_w,
            border_b,
            z + 1,
        );

        // Title text
        if let Some(ref title) = self.title {
            let layout = self.layout();
            let text_x = layout.header.x + 16.0;
            let text_y = layout.header.y + layout.header.h * 0.5 + self.title_size * 0.35;
            canvas.draw_text_run_weighted(
                [text_x, text_y],
                title.clone(),
                self.title_size,
                600.0,
                self.title_color,
                z + 2,
            );
        }
    }

    /// Cards are not focusable.
    fn focus_id(&self) -> Option<FocusId> {
        None
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait
// ---------------------------------------------------------------------------

impl EventHandler for Card {
    fn handle_mouse_click(&mut self, _event: &MouseClickEvent) -> EventResult {
        EventResult::Ignored
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
    fn card_defaults() {
        let card = Card::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 200.0,
        });
        assert!(card.title.is_none());
        assert!(card.show_shadow);
        assert!((card.radius - 8.0).abs() < f32::EPSILON);
    }

    #[test]
    fn card_with_title() {
        let card = Card::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 200.0,
        })
        .with_title("My Card");
        assert_eq!(card.title.as_deref(), Some("My Card"));
    }

    #[test]
    fn card_layout_with_title() {
        let card = Card::new(Rect {
            x: 10.0,
            y: 20.0,
            w: 300.0,
            h: 200.0,
        })
        .with_title("Header");
        let layout = card.layout();
        assert!((layout.header.h - 48.0).abs() < f32::EPSILON);
        assert!((layout.content.h - 152.0).abs() < f32::EPSILON);
        assert!((layout.content.y - 68.0).abs() < f32::EPSILON);
    }

    #[test]
    fn card_layout_without_title() {
        let card = Card::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 300.0,
            h: 200.0,
        });
        let layout = card.layout();
        assert!((layout.header.h).abs() < f32::EPSILON);
        assert!((layout.content.h - 200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn card_hit_test() {
        let card = Card::new(Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 80.0,
        });
        assert!(card.hit_test(50.0, 50.0));
        assert!(!card.hit_test(0.0, 0.0));
    }

    #[test]
    fn card_not_focusable() {
        let card = Card::new(Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        });
        assert!(card.focus_id().is_none());
    }
}
