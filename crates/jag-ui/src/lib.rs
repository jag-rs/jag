//! # jag-ui
//!
//! UI elements, widgets, and Taffy-based layout for jag-draw.
//!
//! Build UIs in Rust with composable elements (buttons, inputs, containers)
//! and automatic flex/grid layout.

pub mod elements;
pub mod event;
pub mod focus;
pub mod hit_region;
pub mod layout;
pub mod theme;
pub mod ui;
pub mod widgets;

pub use event::*;
pub use focus::{FocusDirection, FocusId, FocusManager, FocusResult};
pub use hit_region::HitRegionRegistry;
pub use layout::Layout;
pub use theme::{ElementColors, Theme, ThemeMode};
pub use ui::Ui;
