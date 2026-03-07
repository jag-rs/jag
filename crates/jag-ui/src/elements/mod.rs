//! Core UI elements.
//!
//! All elements implement the [`Element`] trait which combines rendering,
//! layout (via [`Rect`]), and event handling (via [`EventHandler`]).

pub mod alert;
pub mod badge;
pub mod button;
pub mod card;
pub mod checkbox;
pub mod container;
pub mod date_picker;
pub mod image_element;
pub mod input_box;
pub mod link;
pub mod modal;
pub mod radio;
pub mod select;
pub mod slider;
pub mod table;
pub mod text;
pub mod text_align;
pub mod text_area;
pub mod toggle_switch;

pub use alert::{Alert, AlertSeverity};
pub use badge::Badge;
pub use button::{Button, ButtonLabelAlign};
pub use card::Card;
pub use checkbox::Checkbox;
pub use container::Container;
pub use date_picker::DatePicker;
pub use image_element::{ImageElement, ImageFit};
pub use input_box::InputBox;
pub use link::Link;
pub use modal::{Modal, ModalButton, ModalClickResult};
pub use radio::Radio;
pub use select::Select;
pub use slider::Slider;
pub use table::Table;
pub use text::Text;
pub use text_align::TextAlign;
pub use text_area::TextArea;
pub use toggle_switch::ToggleSwitch;

use jag_draw::Rect;
use jag_surface::Canvas;

use crate::event::EventHandler;
use crate::focus::FocusId;

/// Trait implemented by all renderable UI elements.
///
/// Designed for object safety so that external crates (e.g., a future
/// `jag-media`) can implement it for their own element types.
pub trait Element: EventHandler {
    /// The bounding rectangle of this element in logical coordinates.
    fn rect(&self) -> Rect;

    /// Update the bounding rectangle (typically called by the layout engine).
    fn set_rect(&mut self, rect: Rect);

    /// Paint this element onto `canvas` at the given z-index.
    fn render(&self, canvas: &mut Canvas, z: i32);

    /// The focus identifier for this element, if it is focusable.
    fn focus_id(&self) -> Option<FocusId>;
}
