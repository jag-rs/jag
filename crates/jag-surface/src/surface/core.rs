use std::sync::Arc;

use jag_draw::{HitIndex, PassManager, RenderAllocator, wgpu};

use super::{CachedFrameData, JagSurface, OverlayCallback};

impl JagSurface {
    /// Create a new surface wrapper using an existing device/queue and the chosen surface format.
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let pass = PassManager::new(device.clone(), surface_format);
        let allocator = RenderAllocator::new(device.clone());

        Self {
            device,
            queue,
            surface_format,
            pass,
            allocator,
            direct: false,
            preserve_surface: false,
            use_intermediate: true,
            logical_pixels: true,
            dpi_scale: 1.0,
            enable_smaa: false,
            ui_scale: 1.0,
            overlay: None,
            next_synthetic_external_texture_id: 0x7000_0000_0000_0000,
            frame_cache: None,
            frame_cache_enabled: true,
            pending_image_loads: false,
        }
    }

    /// Convenience: construct from shared device/queue handles.
    pub fn from_device_queue(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        Self::new(device, queue, surface_format)
    }

    pub fn device(&self) -> Arc<wgpu::Device> {
        self.device.clone()
    }
    pub fn queue(&self) -> Arc<wgpu::Queue> {
        self.queue.clone()
    }
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }
    pub fn pass_manager(&mut self) -> &mut PassManager {
        &mut self.pass
    }
    pub fn has_pending_image_loads(&self) -> bool {
        self.pending_image_loads
    }
    pub fn allocator_mut(&mut self) -> &mut RenderAllocator {
        &mut self.allocator
    }

    /// Choose whether to render directly to the surface (bypass compositor).
    pub fn set_direct(&mut self, direct: bool) {
        self.direct = direct;
    }
    /// Control whether to preserve existing contents on the surface.
    pub fn set_preserve_surface(&mut self, preserve: bool) {
        self.preserve_surface = preserve;
    }
    /// Choose whether to use an intermediate texture and blit to the surface.
    pub fn set_use_intermediate(&mut self, use_it: bool) {
        self.use_intermediate = use_it;
    }
    /// Enable or disable SMAA. Disabling skips the post-process filter to keep small text crisp.
    pub fn set_enable_smaa(&mut self, enable: bool) {
        self.enable_smaa = enable;
    }
    /// Enable or disable logical pixel interpretation.
    pub fn set_logical_pixels(&mut self, on: bool) {
        self.logical_pixels = on;
    }
    /// Set current DPI scale and propagate to passes before rendering.
    pub fn set_dpi_scale(&mut self, scale: f32) {
        self.dpi_scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
    }
    /// Set a global UI scale multiplier
    pub fn set_ui_scale(&mut self, s: f32) {
        self.ui_scale = if s.is_finite() { s } else { 1.0 };
    }
    /// Set an overlay callback for post-render passes
    pub fn set_overlay(&mut self, callback: OverlayCallback) {
        self.overlay = Some(callback);
    }
    /// Clear the overlay callback
    pub fn clear_overlay(&mut self) {
        self.overlay = None;
    }

    /// Set the GPU-side scroll offset (in logical pixels, typically negative).
    /// This is written into the viewport uniform so the GPU applies the
    /// scroll transform without rebuilding geometry.
    pub fn set_scroll_offset(&mut self, offset: [f32; 2]) {
        self.pass.set_scroll_offset(offset);
    }

    /// Get the current GPU-side scroll offset.
    pub fn scroll_offset(&self) -> [f32; 2] {
        self.pass.scroll_offset()
    }

    /// Access the cached frame data (if any) for scroll-only fast path decisions.
    pub fn frame_cache(&self) -> Option<&CachedFrameData> {
        self.frame_cache.as_ref()
    }

    /// Clear the frame cache (e.g., on resize or content change).
    pub fn clear_frame_cache(&mut self) {
        self.frame_cache = None;
    }

    /// Enable or disable retaining the completed frame for scroll-only replay.
    pub fn set_frame_cache_enabled(&mut self, enabled: bool) {
        self.frame_cache_enabled = enabled;
        if !enabled {
            self.clear_frame_cache();
        }
    }

    /// Update the scroll position, generation, and hit index on the most recent
    /// frame cache. Called by the renderer after `end_frame` to supply metadata
    /// that `end_frame` doesn't have direct access to.
    pub fn update_frame_cache_metadata(
        &mut self,
        scroll_at_build: (f32, f32),
        generation: u64,
        hit_index: HitIndex,
    ) {
        if let Some(ref mut cache) = self.frame_cache {
            cache.scroll_at_build = scroll_at_build;
            cache.generation_at_build = generation;
            cache.hit_index = hit_index;
        }
    }
}
