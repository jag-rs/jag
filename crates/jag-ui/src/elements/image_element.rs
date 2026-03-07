//! Image display element.

use jag_draw::{Brush, ColorLinPremul, Rect};
use jag_surface::{Canvas, ImageFitMode};

use crate::event::{
    EventHandler, EventResult, KeyboardEvent, MouseClickEvent, MouseMoveEvent, ScrollEvent,
};
use crate::focus::FocusId;

use super::Element;

/// How the image should fit within its bounding rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFit {
    /// Stretch to fill (may distort aspect ratio).
    Fill,
    /// Fit inside the rect maintaining aspect ratio (letterbox).
    Contain,
    /// Fill the rect maintaining aspect ratio (may crop).
    Cover,
}

impl Default for ImageFit {
    fn default() -> Self {
        Self::Contain
    }
}

impl ImageFit {
    /// Convert to `jag_surface::ImageFitMode`.
    fn to_fit_mode(self) -> ImageFitMode {
        match self {
            Self::Fill => ImageFitMode::Fill,
            Self::Contain => ImageFitMode::Contain,
            Self::Cover => ImageFitMode::Cover,
        }
    }
}

/// An element that displays an image from a file path, with a
/// configurable fit mode and a fallback tint color.
pub struct ImageElement {
    pub rect: Rect,
    /// Path to the image file.
    pub source: Option<String>,
    /// Fallback color shown when no source is set or loading fails.
    pub fallback_color: ColorLinPremul,
    /// How the image fits within the rect.
    pub fit: ImageFit,
}

impl ImageElement {
    /// Create an image element with the given source path.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 150.0,
            },
            source: Some(source.into()),
            fallback_color: ColorLinPremul::from_srgba_u8([200, 200, 200, 255]),
            fit: ImageFit::default(),
        }
    }

    /// Create an image element with a fallback color only (no image).
    pub fn placeholder(color: ColorLinPremul) -> Self {
        Self {
            rect: Rect {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 150.0,
            },
            source: None,
            fallback_color: color,
            fit: ImageFit::default(),
        }
    }

    /// Set the fit mode.
    pub fn with_fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    /// Hit-test.
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

impl Element for ImageElement {
    fn rect(&self) -> Rect {
        self.rect
    }

    fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    fn render(&self, canvas: &mut Canvas, z: i32) {
        if let Some(ref path) = self.source {
            canvas.draw_image(
                path.clone(),
                [self.rect.x, self.rect.y],
                [self.rect.w, self.rect.h],
                self.fit.to_fit_mode(),
                z,
            );
        } else {
            canvas.fill_rect(
                self.rect.x,
                self.rect.y,
                self.rect.w,
                self.rect.h,
                Brush::Solid(self.fallback_color),
                z,
            );
        }
    }

    fn focus_id(&self) -> Option<FocusId> {
        None
    }
}

// ---------------------------------------------------------------------------
// EventHandler trait
// ---------------------------------------------------------------------------

impl EventHandler for ImageElement {
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
    fn image_element_new() {
        let img = ImageElement::new("/path/to/image.png");
        assert_eq!(img.source.as_deref(), Some("/path/to/image.png"));
        assert_eq!(img.fit, ImageFit::Contain);
    }

    #[test]
    fn image_element_placeholder() {
        let color = ColorLinPremul::from_srgba_u8([128, 128, 128, 255]);
        let img = ImageElement::placeholder(color);
        assert!(img.source.is_none());
        assert_eq!(img.fallback_color, color);
    }

    #[test]
    fn image_element_with_fit() {
        let img = ImageElement::new("test.jpg").with_fit(ImageFit::Cover);
        assert_eq!(img.fit, ImageFit::Cover);
    }

    #[test]
    fn image_element_hit_test() {
        let mut img = ImageElement::new("test.png");
        img.rect = Rect {
            x: 10.0,
            y: 10.0,
            w: 100.0,
            h: 80.0,
        };
        assert!(img.hit_test(50.0, 50.0));
        assert!(!img.hit_test(0.0, 0.0));
    }

    #[test]
    fn image_element_not_focusable() {
        let img = ImageElement::new("test.png");
        assert!(img.focus_id().is_none());
    }
}
