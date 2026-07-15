use std::sync::Arc;

use jag_draw::{
    BackdropBlurDraw, ColorLinPremul, Painter, Rect, RoundedRect, TextProvider, Transform2D,
    Viewport,
};

mod helpers;
mod images_state;
mod masks;
mod shapes;
mod text;
mod text_extra;

#[cfg(test)]
mod tests;

/// Rounded-rect clip region in device pixels, passed to the image shader
/// for SDF-based fragment discard.
#[derive(Debug, Clone, Copy)]
pub struct RoundedRectClip {
    /// Bounding rect in device pixels.
    pub rect: Rect,
    /// Per-corner radii in device pixels (tl, tr, br, bl).
    pub radii: [f32; 4],
}

/// How an image should fit within its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFitMode {
    /// Stretch to fill (may distort aspect ratio)
    Fill,
    /// Fit inside maintaining aspect ratio (letterbox/pillarbox)
    Contain,
    /// Fill maintaining aspect ratio (may crop edges)
    Cover,
}

impl Default for ImageFitMode {
    fn default() -> Self {
        Self::Contain
    }
}

/// Builder for a single frame’s draw commands. Wraps `Painter` and adds canvas helpers.
pub struct Canvas {
    pub(crate) viewport: Viewport,
    pub(crate) painter: Painter,
    pub(crate) clear_color: Option<ColorLinPremul>,
    pub(crate) text_provider: Option<Arc<dyn TextProvider + Send + Sync>>, // optional high-level text shaper
    pub(crate) glyph_draws: Vec<(
        [f32; 2],
        jag_draw::RasterizedGlyph,
        ColorLinPremul,
        i32,
        Option<Rect>,
    )>, // low-level glyph masks with z-index + clip
    pub(crate) svg_draws: Vec<(
        std::path::PathBuf,
        [f32; 2],
        [f32; 2],
        Option<jag_draw::SvgStyle>,
        i32,
        f32,
        Transform2D,
        Option<Rect>,
        Option<RoundedRectClip>,
    )>, // (path, origin, max_size, style, z, opacity, transform, device_clip, rounded_clip)
    pub(crate) image_draws: Vec<(
        std::path::PathBuf,
        [f32; 2],
        [f32; 2],
        ImageFitMode,
        i32,
        f32,
        Transform2D,
        Option<Rect>,
        Option<RoundedRectClip>,
    )>, // (path, origin, size, fit, z, opacity, transform, device_clip, rounded_clip)
    pub(crate) backdrop_blur_draws: Vec<BackdropBlurDraw>,
    /// Raw pixel data draws: (pixels_rgba, src_width, src_height, origin, dst_size, z, transform)
    pub(crate) raw_image_draws: Vec<RawImageDraw>,
    pub(crate) dpi_scale: f32, // DPI scale factor for text rendering
    // Effective clip stack in device coordinates for direct text rendering.
    // Each entry is the intersection of all active clips at that depth.
    pub(crate) clip_stack: Vec<Option<Rect>>,
    // Parallel stack: optional rounded-rect clip for the current depth.
    // When present, image draws receive this for SDF-based corner clipping.
    pub(crate) rounded_clip_stack: Vec<Option<RoundedRectClip>>,
    // Overlay rectangles that render without depth testing (for modal scrims).
    // These are rendered in a separate pass after the main scene.
    pub(crate) overlay_draws: Vec<(Rect, ColorLinPremul)>,
    // Scrim draws that blend over content but allow z-ordered content to render on top.
    // Supports either a full-rect scrim or a scrim with a rounded-rect cutout via stencil.
    pub(crate) scrim_draws: Vec<ScrimDraw>,
    // Effective opacity for side-channel draws (SVGs/images) that are not
    // emitted through the display-list command stream.
    pub(crate) opacity_stack: Vec<f32>,
    pub(crate) generated_mask_textures: Vec<GeneratedMaskTexture>,
    pub(crate) url_mask_textures: Vec<UrlMaskTexture>,
    pub(crate) next_generated_mask_texture_id: u64,
}

pub(crate) struct UrlMaskTexture {
    pub id: jag_draw::ExternalTextureId,
    pub path: std::path::PathBuf,
}

pub(crate) struct GeneratedMaskTexture {
    pub id: jag_draw::ExternalTextureId,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Scrim drawing modes.
#[derive(Clone, Copy)]
pub enum ScrimDraw {
    Rect(Rect, ColorLinPremul),
    Cutout {
        hole: RoundedRect,
        color: ColorLinPremul,
    },
}

/// Raw image draw request for rendering pixel data directly.
#[derive(Clone)]
pub struct RawImageDraw {
    /// BGRA pixel data (4 bytes per pixel) - matches CEF native format
    pub pixels: Vec<u8>,
    /// Source image width
    pub src_width: u32,
    /// Source image height
    pub src_height: u32,
    /// Destination origin in scene coordinates
    pub origin: [f32; 2],
    /// Destination size in scene coordinates
    pub dst_size: [f32; 2],
    /// Z-index for depth ordering
    pub z: i32,
    /// Transform at draw time
    pub transform: Transform2D,
    /// Dirty rectangles for partial update (x, y, w, h) - empty = full frame
    pub dirty_rects: Vec<(u32, u32, u32, u32)>,
    /// Device-space clip rect for GPU scissor clipping (None = no clip).
    pub clip: Option<Rect>,
}

impl Canvas {
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Get the current transform from the painter's transform stack.
    pub fn current_transform(&self) -> Transform2D {
        self.painter.current_transform()
    }

    fn current_opacity(&self) -> f32 {
        self.opacity_stack.last().copied().unwrap_or(1.0)
    }
}
