use bytemuck::{Pod, Zeroable};

use crate::allocator::OwnedBuffer;
use crate::display_list::ExternalTextureId;
use crate::scene::{Rect, TextRun, Transform2D};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
    pub color: [f32; 4],
    pub z_index: f32,
}

pub struct GpuScene {
    pub vertex: OwnedBuffer,
    pub index: OwnedBuffer,
    pub vertices: u32,
    pub indices: u32,
}

/// Extracted text draw from DisplayList
#[derive(Clone, Debug)]
pub struct ExtractedTextDraw {
    pub run: TextRun,
    pub z: i32,
    pub transform: Transform2D,
    /// Clip rect (in logical scene coordinates) inherited from the display list
    /// clip stack. `None` means no clipping.
    pub clip: Option<Rect>,
}

/// Extracted image draw from DisplayList (placeholder for future)
#[derive(Clone, Debug)]
pub struct ExtractedImageDraw {
    pub path: std::path::PathBuf,
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub z: i32,
    pub transform: Transform2D,
    pub opacity: f32,
}

/// Extracted SVG draw from DisplayList (placeholder for future)
#[derive(Clone, Debug)]
pub struct ExtractedSvgDraw {
    pub path: std::path::PathBuf,
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub z: i32,
    pub transform: Transform2D,
    pub opacity: f32,
}

/// Extracted external texture draw from DisplayList.
#[derive(Clone, Debug)]
pub struct ExtractedExternalTextureDraw {
    pub texture_id: ExternalTextureId,
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub z: i32,
    pub opacity: f32,
    pub premultiplied: bool,
}

/// A contiguous range inside the transparent index buffer for a given z-index.
#[derive(Clone, Copy, Debug)]
pub struct TransparentBatch {
    pub z: i32,
    pub index_start: u32,
    pub index_count: u32,
    /// Clip rect inherited from the display list clip stack.
    pub clip: Option<Rect>,
}

/// A contiguous range inside the opaque solid index buffer sharing a clip rect.
#[derive(Clone, Copy, Debug)]
pub struct SolidBatch {
    pub index_start: u32,
    pub index_count: u32,
    /// Clip rect inherited from the display list clip stack.
    pub clip: Option<Rect>,
}

/// Complete unified scene data extracted from DisplayList
pub struct UnifiedSceneData {
    pub gpu_scene: GpuScene,
    pub solid_batches: Vec<SolidBatch>,
    pub transparent_gpu_scene: GpuScene,
    pub transparent_batches: Vec<TransparentBatch>,
    pub text_draws: Vec<ExtractedTextDraw>,
    pub image_draws: Vec<ExtractedImageDraw>,
    pub svg_draws: Vec<ExtractedSvgDraw>,
    pub external_texture_draws: Vec<ExtractedExternalTextureDraw>,
    pub shadow_instances: Vec<crate::box_shadow::ShadowInstance>,
}
