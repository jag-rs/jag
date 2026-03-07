//! jag-draw: GPU-accelerated 2D rendering engine.
#![allow(
    clippy::collapsible_if,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::get_first,
    clippy::unnecessary_cast,
    clippy::useless_conversion,
    clippy::manual_clamp,
    clippy::assign_op_pattern,
    clippy::needless_borrow,
    clippy::or_fun_call,
    clippy::collapsible_match,
    clippy::needless_range_loop,
    clippy::manual_ok_or,
    clippy::or_then_unwrap,
    clippy::map_unwrap_or,
    clippy::unnecessary_map_or,
    clippy::unwrap_or_default,
    clippy::manual_is_multiple_of,
    clippy::manual_slice_size_calculation,
    clippy::excessive_precision,
    clippy::identity_op,
    clippy::new_without_default,
    clippy::needless_update,
    clippy::empty_line_after_doc_comments,
    clippy::doc_lazy_continuation,
    clippy::field_reassign_with_default,
    clippy::option_map_or_none
)]

use std::sync::Arc;

use anyhow::Result;

/// Re-export wgpu for downstream crates while avoiding direct dependency leakage.
pub use wgpu;

mod allocator;
pub use allocator::{OwnedBuffer, OwnedTexture, RenderAllocator};

/// Top-level engine handle.
pub struct GraphicsEngine {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    allocator: RenderAllocator,
}

impl GraphicsEngine {
    /// Initialize the engine with an existing device/queue.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let device = Arc::new(device);
        let queue = Arc::new(queue);
        let allocator = RenderAllocator::new(device.clone());
        Self {
            device,
            queue,
            allocator,
        }
    }

    /// Get a clone of the device handle.
    pub fn device(&self) -> Arc<wgpu::Device> {
        self.device.clone()
    }

    /// Get a clone of the queue handle.
    pub fn queue(&self) -> Arc<wgpu::Queue> {
        self.queue.clone()
    }

    /// Access the allocator.
    pub fn allocator(&self) -> &RenderAllocator {
        &self.allocator
    }
    pub fn allocator_mut(&mut self) -> &mut RenderAllocator {
        &mut self.allocator
    }

    /// Placeholder render function to be expanded later.
    pub fn render_dummy(&self) -> Result<()> {
        // No-op for now.
        let _ = (&self.device, &self.queue);
        Ok(())
    }
}

/// Choose an sRGB surface format when available; otherwise, pick the first format.
pub fn choose_srgb_surface_format(
    adapter: &wgpu::Adapter,
    surface: &wgpu::Surface,
) -> wgpu::TextureFormat {
    let caps = surface.get_capabilities(adapter);
    caps.formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(caps.formats[0])
}

/// Create a surface configuration for the given size, favoring FIFO present mode when present.
pub fn make_surface_config(
    adapter: &wgpu::Adapter,
    surface: &wgpu::Surface,
    width: u32,
    height: u32,
) -> wgpu::SurfaceConfiguration {
    let caps = surface.get_capabilities(adapter);
    let format = choose_srgb_surface_format(adapter, surface);
    let present_mode = caps
        .present_modes
        .iter()
        .copied()
        .find(|m| *m == wgpu::PresentMode::Fifo)
        .unwrap_or(caps.present_modes[0]);
    let alpha_mode = caps
        .alpha_modes
        .iter()
        .copied()
        .find(|m| *m == wgpu::CompositeAlphaMode::Opaque)
        .unwrap_or(caps.alpha_modes[0]);
    wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width,
        height,
        present_mode,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 1,
    }
}

// Scene and display list (Phase 2)
mod color; // sRGB conversion helpers for ColorLinPremul
mod display_list;
mod dpi;
mod hit_test;
mod image_cache;
mod painter;
mod pass_manager;
mod pipeline;
mod scene;
mod svg;
mod text;
mod text_layout;
mod upload;

pub use display_list::*;
pub use dpi::*;
pub use hit_test::*;
pub use image_cache::*;
pub use painter::*;
pub use pass_manager::Background as RootBackground;
pub use pass_manager::*;
pub use pipeline::Blitter;
pub use pipeline::*;
pub use scene::*;
pub use svg::*;
pub use text::*;

/// Backward-compat alias for code that still references the old name.
pub type DetirTextProvider = JagTextProvider;
pub use text_layout::*;
pub use upload::*;

// Optional: PDF export backend using `pdf-writer`.
#[cfg(feature = "pdf_export")]
mod pdf_backend;
#[cfg(feature = "pdf_export")]
pub use pdf_backend::render_display_list_to_pdf;
