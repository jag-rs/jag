//! GPU render pipelines, one module per renderer family. Split from a single
//! ~2200-line file (behavior-preserving) to satisfy the repo file-size limit
//! and to give the analytic box-shadow renderer a home. Public API is
//! unchanged: every renderer is re-exported, so `crate::pipeline::*` resolves
//! exactly as before.

mod background_blur;
mod color_filter;
mod composite;
mod drop_shadow_filter;
mod scrim_stencil;
mod shadow;
mod shadow_composite_instance;
mod smaa;
mod solid;
mod text_image;

pub use background_blur::*;
pub use color_filter::*;
pub use composite::*;
pub use drop_shadow_filter::*;
pub use scrim_stencil::*;
pub use shadow::*;
pub use shadow_composite_instance::*;
pub use smaa::*;
pub use solid::*;
pub use text_image::*;
