use std::sync::{Arc, Mutex};

use jag_draw::{
    PassManager,
    RenderAllocator,
    Transform2D,
    wgpu, // import wgpu from engine-core to keep type identity
};

use crate::canvas::ImageFitMode;

mod cached;
mod core;
mod frame;
mod opacity;

/// Cached GPU resources from a previous `end_frame` call, enabling scroll-only
/// frames to skip the expensive IR walk, display list build, and GPU upload.
/// Instead, the cached buffers are re-rendered with a delta scroll offset applied
/// via the viewport uniform.
#[allow(clippy::type_complexity)]
pub struct CachedFrameData {
    /// The GPU scene (vertex/index buffers for opaque geometry).
    pub gpu_scene: jag_draw::GpuScene,
    /// Per-clip-region ranges in the opaque solid index buffer.
    pub solid_batches: Vec<jag_draw::SolidBatch>,
    /// The GPU scene for transparent (per-z-batch) geometry.
    pub transparent_gpu_scene: jag_draw::GpuScene,
    /// Per-z-index ranges in the transparent index buffer.
    pub transparent_batches: Vec<jag_draw::TransparentBatch>,
    /// Pre-rasterized glyph draws.
    pub glyph_draws: Vec<(
        [f32; 2],
        jag_draw::RasterizedGlyph,
        jag_draw::ColorLinPremul,
        i32,
        Option<jag_draw::Rect>,
    )>,
    /// Resolved SVG draws.
    pub svg_draws: Vec<(
        std::path::PathBuf,
        [f32; 2],
        [f32; 2],
        Option<jag_draw::SvgStyle>,
        i32,
        f32,
        Transform2D,
        Option<jag_draw::Rect>,
        Option<jag_draw::RoundedRectClipGpu>,
    )>,
    /// Resolved image draws.
    pub image_draws: Vec<(
        std::path::PathBuf,
        [f32; 2],
        [f32; 2],
        i32,
        f32,
        Option<jag_draw::Rect>,
        Option<jag_draw::RoundedRectClipGpu>,
    )>,
    /// CSS backdrop-filter blur draws.
    pub backdrop_blur_draws: Vec<jag_draw::BackdropBlurDraw>,
    /// External texture draws (e.g. Canvas3D, opacity group layers).
    pub external_texture_draws: Vec<jag_draw::ExtractedExternalTextureDraw>,
    /// Clear color used for this frame.
    pub clear: wgpu::Color,
    /// Whether the frame was rendered directly (vs intermediate texture).
    pub direct: bool,
    /// Frame dimensions.
    pub width: u32,
    pub height: u32,
    /// Scroll offset at the time the frame was built, for computing the delta.
    pub scroll_at_build: (f32, f32),
    /// Visual generation at build time.
    pub generation_at_build: u64,
    /// Viewport size at build time (for invalidation).
    pub viewport_size: (u32, u32),
    /// The hit index from the built frame (reused during scroll-only frames).
    pub hit_index: jag_draw::HitIndex,
}

/// Apply a 2D affine transform to a point
pub(crate) fn apply_transform_to_point(point: [f32; 2], transform: Transform2D) -> [f32; 2] {
    let [a, b, c, d, e, f] = transform.m;
    let x = point[0];
    let y = point[1];
    [a * x + c * y + e, b * x + d * y + f]
}

/// Storage for the last rendered raw image rect (used for hit testing WebViews).
static LAST_RAW_IMAGE_RECT: Mutex<Option<(f32, f32, f32, f32)>> = Mutex::new(None);

/// Set the last raw image rect (called during rendering).
pub(crate) fn set_last_raw_image_rect(x: f32, y: f32, w: f32, h: f32) {
    if let Ok(mut guard) = LAST_RAW_IMAGE_RECT.lock() {
        *guard = Some((x, y, w, h));
    }
}

/// Get the last raw image rect (for hit testing from FFI).
pub fn get_last_raw_image_rect() -> Option<(f32, f32, f32, f32)> {
    if let Ok(guard) = LAST_RAW_IMAGE_RECT.lock() {
        *guard
    } else {
        None
    }
}

/// Overlay callback signature: called after main rendering with full PassManager access.
/// Allows scenes to draw overlays (like SVG ticks) directly to the surface.
pub type OverlayCallback = Box<
    dyn FnMut(
        &mut PassManager,
        &mut wgpu::CommandEncoder,
        &wgpu::TextureView,
        &wgpu::Queue,
        u32,
        u32,
    ),
>;

/// Calculate the actual render origin and size for an image based on fit mode.
/// Returns (origin, size) where the image should be drawn.
pub(crate) fn calculate_image_fit(
    origin: [f32; 2],
    bounds: [f32; 2],
    img_w: f32,
    img_h: f32,
    fit: ImageFitMode,
) -> ([f32; 2], [f32; 2]) {
    match fit {
        ImageFitMode::Fill => {
            // Stretch to fill - use bounds as-is
            (origin, bounds)
        }
        ImageFitMode::Contain => {
            // Fit inside maintaining aspect ratio
            let bounds_aspect = bounds[0] / bounds[1];
            let img_aspect = img_w / img_h;

            let (render_w, render_h) = if img_aspect > bounds_aspect {
                // Image is wider - fit to width
                (bounds[0], bounds[0] / img_aspect)
            } else {
                // Image is taller - fit to height
                (bounds[1] * img_aspect, bounds[1])
            };

            // Center within bounds
            let offset_x = (bounds[0] - render_w) * 0.5;
            let offset_y = (bounds[1] - render_h) * 0.5;

            (
                [origin[0] + offset_x, origin[1] + offset_y],
                [render_w, render_h],
            )
        }
        ImageFitMode::Cover => {
            // Fill maintaining aspect ratio (may crop)
            let bounds_aspect = bounds[0] / bounds[1];
            let img_aspect = img_w / img_h;

            let (render_w, render_h) = if img_aspect > bounds_aspect {
                // Image is wider - fit to height
                (bounds[1] * img_aspect, bounds[1])
            } else {
                // Image is taller - fit to width
                (bounds[0], bounds[0] / img_aspect)
            };

            // Center within bounds (will be clipped)
            let offset_x = (bounds[0] - render_w) * 0.5;
            let offset_y = (bounds[1] - render_h) * 0.5;

            (
                [origin[0] + offset_x, origin[1] + offset_y],
                [render_w, render_h],
            )
        }
    }
}

/// High-level canvas-style wrapper over Painter + PassManager.
///
/// Typical flow:
/// - let mut canvas = surface.begin_frame(w, h);
/// - canvas.clear(color);
/// - canvas.draw calls ...
/// - surface.end_frame(frame, canvas);
pub struct JagSurface {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    surface_format: wgpu::TextureFormat,
    pass: PassManager,
    allocator: RenderAllocator,
    /// When true, render directly to the surface; otherwise render offscreen then composite.
    direct: bool,
    /// When true, preserve existing surface content (LoadOp::Load) instead of clearing.
    preserve_surface: bool,
    /// When true, render solids to an intermediate texture and blit to the surface.
    /// This matches the demo-app default and is often more robust across platforms during resize.
    use_intermediate: bool,
    /// When true, positions are interpreted as logical pixels and scaled by dpi_scale in PassManager.
    logical_pixels: bool,
    /// Current DPI scale factor (e.g., 2.0 on Retina).
    dpi_scale: f32,
    /// When true, run SMAA resolve; when false, favor a direct blit for crisper text.
    enable_smaa: bool,
    /// Additional UI scale multiplier
    ui_scale: f32,
    /// Optional overlay callback for post-render passes (e.g., SVG overlays)
    overlay: Option<OverlayCallback>,
    /// Monotonic allocator for internally-generated external texture IDs
    /// (used for opacity group compositing layers).
    next_synthetic_external_texture_id: u64,
    /// Cached frame data from the most recent `end_frame` call, enabling
    /// scroll-only frames to skip the IR walk and GPU upload.
    frame_cache: Option<CachedFrameData>,
    /// Whether `end_frame` should retain a full GPU copy of the frame for
    /// scroll-only replay.
    frame_cache_enabled: bool,
    pending_image_loads: bool,
}
