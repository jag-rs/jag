//! # Jag
//!
//! GPU-accelerated 2D rendering and UI toolkit.
//!
//! Jag provides layered crates:
//! - `jag::draw` — Low-level GPU 2D renderer
//! - `jag::ui` — UI elements, widgets, and layout
//! - `jag::surface` — Canvas-style drawing API

pub use jag_draw as draw;
pub use jag_surface as surface;
pub use jag_ui as ui;
