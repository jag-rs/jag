//! `PassManager` methods (setup). Verbatim extraction from the
//! former monolithic `pass_manager.rs`; no logic changed.

use super::PassManager;
use crate::pipeline::{
    BackdropBlurRenderer, BackgroundRenderer, BasicSolidRenderer, Blitter, BlurRenderer,
    Compositor, OverlaySolidRenderer, ScrimSolidRenderer, ScrimStencilMaskRenderer,
    ScrimStencilRenderer, ShadowCompositeInstanceRenderer, ShadowCompositeRenderer,
    ShadowInstanceRenderer, SmaaRenderer, TextRenderer,
};
use std::sync::Arc;

impl PassManager {
    /// Choose the best offscreen format based on scene color space.
    ///
    /// - If the whole render target is sRGB, prefer Rgba8UnormSrgb so wgpu handles the
    ///   linear→sRGB conversion on write.
    /// - If the scene is linear-light, keep the offscreen target linear via Rgba8Unorm.
    fn choose_offscreen_format(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
    ) -> wgpu::TextureFormat {
        // WORKAROUND: Stay on 8-bit formats due to Metal blending issues with Rgba16Float.
        let prefer_srgb = target_format.is_srgb();
        let preferred = if prefer_srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        // Query capabilities directly to avoid invoking driver code that might throw
        // foreign exceptions on unsupported formats.
        let device_features = device.features();
        let supports = |format: wgpu::TextureFormat| {
            format
                .guaranteed_format_features(device_features)
                .allowed_usages
                .contains(wgpu::TextureUsages::RENDER_ATTACHMENT)
        };

        if supports(preferred) {
            preferred
        } else {
            let fallback = if prefer_srgb {
                wgpu::TextureFormat::Rgba8Unorm
            } else {
                wgpu::TextureFormat::Rgba8UnormSrgb
            };

            if supports(fallback) {
                fallback
            } else {
                // Last resort: keep the original target format.
                target_format
            }
        }
    }

    pub fn new(device: Arc<wgpu::Device>, target_format: wgpu::TextureFormat) -> Self {
        // Try Rgba16Float for better gradient quality, fallback to Rgba8Unorm if not supported
        let offscreen_format = Self::choose_offscreen_format(&device, target_format);
        // MSAA>1 interacts poorly with our current depth setup (depth textures
        // are 1×). Keep sample count at 1 and rely on SMAA / pixel snapping
        // for edge smoothing to avoid crashes and validation errors.
        let msaa_count = 1;
        let solid_offscreen = BasicSolidRenderer::new(device.clone(), offscreen_format, msaa_count);
        let solid_direct = BasicSolidRenderer::new(device.clone(), target_format, msaa_count);
        let transparent_solid_offscreen = BasicSolidRenderer::new_with_depth_state(
            device.clone(),
            offscreen_format,
            msaa_count,
            false,
            wgpu::CompareFunction::LessEqual,
        );
        let transparent_solid_direct = BasicSolidRenderer::new_with_depth_state(
            device.clone(),
            target_format,
            msaa_count,
            false,
            wgpu::CompareFunction::LessEqual,
        );
        let solid_direct_no_msaa = BasicSolidRenderer::new(device.clone(), target_format, 1);
        let overlay_solid = OverlaySolidRenderer::new(device.clone(), target_format);
        let scrim_solid = ScrimSolidRenderer::new(device.clone(), target_format);
        let compositor = Compositor::new(device.clone(), target_format);
        let blitter = Blitter::new(device.clone(), target_format);
        let smaa = SmaaRenderer::new(device.clone(), target_format);
        let scrim_mask = ScrimStencilMaskRenderer::new(device.clone(), target_format);
        let scrim_stencil = ScrimStencilRenderer::new(device.clone(), target_format);
        // Shadow/blur pipelines
        let mask_renderer =
            BasicSolidRenderer::new(device.clone(), wgpu::TextureFormat::R8Unorm, 1);
        let blur_r8 = BlurRenderer::new(device.clone(), wgpu::TextureFormat::R8Unorm);
        let backdrop_blur = BackdropBlurRenderer::new(device.clone(), offscreen_format);
        let shadow_comp = ShadowCompositeRenderer::new(device.clone(), target_format);
        let shadow_offscreen =
            ShadowInstanceRenderer::new(device.clone(), offscreen_format, msaa_count);
        let shadow_direct = ShadowInstanceRenderer::new(device.clone(), target_format, msaa_count);
        let shadow_composite =
            ShadowCompositeInstanceRenderer::new(device.clone(), offscreen_format, msaa_count);
        let text = TextRenderer::new(device.clone(), target_format);
        let text_offscreen = TextRenderer::new(device.clone(), offscreen_format);
        let image = crate::pipeline::ImageRenderer::new(device.clone(), target_format);
        let image_offscreen = crate::pipeline::ImageRenderer::new(device.clone(), offscreen_format);
        let svg_cache = crate::svg::SvgRasterCache::new(device.clone());
        let image_cache = crate::image_cache::ImageCache::new(device.clone());
        let bg = BackgroundRenderer::new(device.clone(), target_format);
        let vp_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport-uniform"),
            size: 32, // [scale.x, scale.y, translate.x, translate.y, scroll.x, scroll.y, pad, pad]
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Z-index uniform buffer for dynamic depth control (Phase 2)
        let z_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("z-index-uniform"),
            size: 4, // Single f32 value
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg_param_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("background-params"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bg_stops_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("background-stops"),
            size: 256, // 8 stops x 32 bytes
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let smaa_param_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("smaa-params"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        // Text pipeline GPU resources
        let text_mask_atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("text-mask-atlas"),
            size: wgpu::Extent3d {
                width: 4096,
                height: 4096,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // Use RGBA8 so we can store RGB subpixel coverage masks directly.
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let text_mask_atlas_view =
            text_mask_atlas.create_view(&wgpu::TextureViewDescriptor::default());
        let text_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("text-mask-bgl"),
            layout: &text.tex_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&text_mask_atlas_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&text.sampler),
                },
            ],
        });
        // Defaults: always interpret author coords as logical pixels and scale by DPI.
        let logical_default = true;
        let ui_scale = 1.0;
        Self {
            device,
            solid_offscreen,
            solid_direct,
            transparent_solid_offscreen,
            transparent_solid_direct,
            solid_direct_no_msaa,
            overlay_solid,
            scrim_solid,
            compositor,
            blitter,
            smaa,
            scrim_mask,
            scrim_stencil,
            mask_renderer,
            blur_r8,
            backdrop_blur,
            shadow_comp,
            shadow_offscreen,
            shadow_direct,
            shadow_composite,
            text,
            text_offscreen,
            image,
            image_offscreen,
            svg_cache,
            image_cache,
            offscreen_format,
            surface_format: target_format,
            vp_buffer,
            scroll_offset: [0.0, 0.0],
            shadow_instances: Vec::new(),
            z_index_buffer,
            bg,
            bg_param_buffer,
            bg_stops_buffer,
            scale_factor: 1.0,
            ui_scale,
            logical_pixels: logical_default,
            intermediate_texture: None,
            smaa_edges: None,
            smaa_weights: None,
            depth_texture: None,
            text_mask_atlas,
            text_mask_atlas_view,
            text_bind_group,
            text_atlas_upload: Vec::new(),
            prev_atlas_max_x: 0,
            prev_atlas_max_y: 0,
            smaa_param_buffer,
            scrim_stencil_tex: None,
            external_textures: std::collections::HashMap::new(),
        }
    }

    /// Expose the device for scenes that need to create textures.
    pub fn device(&self) -> Arc<wgpu::Device> {
        self.device.clone()
    }

    /// Register an externally-rendered texture for compositing in the current frame.
    pub fn register_external_texture(
        &mut self,
        id: crate::display_list::ExternalTextureId,
        view: wgpu::TextureView,
    ) {
        self.external_textures.insert(id, view);
    }

    /// Clear all registered external textures (call after frame).
    pub fn clear_external_textures(&mut self) {
        self.external_textures.clear();
    }

    /// Create a z-index bind group for the given z-index value.
    /// This is used for dynamic depth control in Phase 2.
    pub fn create_z_bind_group(&self, z_index: f32, queue: &wgpu::Queue) -> wgpu::BindGroup {
        queue.write_buffer(&self.z_index_buffer, 0, bytemuck::bytes_of(&z_index));
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("z-index-bg"),
            layout: self.solid_direct.z_index_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.z_index_buffer.as_entire_binding(),
            }],
        })
    }

    /// Create a z-index bind group backed by a dedicated uniform buffer for this draw group.
    /// This avoids sharing a single z-index uniform across multiple groups, which would cause
    /// all draws to use the last-written z value (breaking per-group z-ordering).
    pub(crate) fn create_group_z_bind_group(
        &self,
        z_index: f32,
        queue: &wgpu::Queue,
    ) -> (wgpu::BindGroup, wgpu::Buffer) {
        let z_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("z-index-group-buffer"),
            size: std::mem::size_of::<f32>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&z_buf, 0, bytemuck::bytes_of(&z_index));
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("z-index-bg-group"),
            layout: self.solid_direct.z_index_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: z_buf.as_entire_binding(),
            }],
        });
        (bg, z_buf)
    }

    /// Rasterize an SVG file to a cached texture for the given scale.
    /// Returns a texture view and its pixel dimensions on success.
    /// Optional style parameter allows overriding fill, stroke, and stroke-width.
    pub fn rasterize_svg_to_view(
        &mut self,
        path: &std::path::Path,
        scale: f32,
        style: Option<crate::svg::SvgStyle>,
        queue: &wgpu::Queue,
    ) -> Option<(wgpu::TextureView, u32, u32)> {
        let svg_style = style.unwrap_or_default();
        let (tex, w, h) = self
            .svg_cache
            .get_or_rasterize(path, scale, svg_style, queue)?;
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        Some((view, w, h))
    }

    /// Load a raster image (PNG/JPEG/GIF/WebP) from disk to a cached GPU texture.
    /// Returns a texture view and its pixel dimensions on success.
    pub fn load_image_to_view(
        &mut self,
        path: &std::path::Path,
        queue: &wgpu::Queue,
    ) -> Option<(wgpu::TextureView, u32, u32)> {
        let (tex, w, h) = self.image_cache.get_or_load(path, queue)?;
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        Some((view, w, h))
    }

    /// Try to get an image from cache without blocking. Returns None if not ready.
    pub fn try_get_image_view(
        &mut self,
        path: &std::path::Path,
    ) -> Option<(wgpu::TextureView, u32, u32)> {
        let (tex, w, h) = self.image_cache.get(path)?;
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        Some((view, w, h))
    }

    /// Request an image to be loaded. Marks it as loading if not already in cache.
    pub fn request_image_load(&mut self, path: &std::path::Path) -> bool {
        self.image_cache.start_load(path)
    }

    /// Upload any image decodes that completed on background threads.
    pub fn poll_image_loads(&mut self, queue: &wgpu::Queue) -> bool {
        self.image_cache.poll_decoded(queue)
    }

    /// Check if an image is ready in the cache.
    pub fn is_image_ready(&self, path: &std::path::Path) -> bool {
        self.image_cache.is_ready(path)
    }

    /// Store a pre-loaded image texture in the cache.
    pub fn store_loaded_image(
        &mut self,
        path: &std::path::Path,
        tex: Arc<wgpu::Texture>,
        width: u32,
        height: u32,
    ) {
        self.image_cache.store_ready(path, tex, width, height);
    }

    /// Get a cached texture directly (for updating pixel data in-place).
    /// Returns the Arc<Texture> and dimensions if found.
    pub fn get_cached_texture(
        &mut self,
        path: &std::path::Path,
    ) -> Option<(Arc<wgpu::Texture>, u32, u32)> {
        self.image_cache.get(path)
    }

    /// Set the platform DPI scale factor. On macOS this is used to correct
    /// radial gradient centering when using normalized UVs for fullscreen fills.
    pub fn set_scale_factor(&mut self, sf: f32) {
        if sf.is_finite() && sf > 0.0 {
            self.scale_factor = sf;
        } else {
            self.scale_factor = 1.0;
        }
    }

    /// Set author-controlled UI scale multiplier (applies in logical mode).
    pub fn set_ui_scale(&mut self, s: f32) {
        let s = if s.is_finite() { s } else { 1.0 };
        self.ui_scale = s.clamp(0.25, 4.0);
    }

    /// Set the GPU-side scroll offset (in logical pixels, typically negative).
    /// This value is written into the viewport uniform so the GPU applies the
    /// scroll transform without rebuilding geometry.
    pub fn set_scroll_offset(&mut self, offset: [f32; 2]) {
        self.scroll_offset = offset;
    }

    /// Get the current GPU-side scroll offset.
    pub fn scroll_offset(&self) -> [f32; 2] {
        self.scroll_offset
    }

    /// Set the analytic box-shadow instances for the next `render_unified`.
    /// Empty disables the shadow pass; non-empty draws them after the opaque
    /// solids and before the z-sorted transparent interleave.
    pub fn set_shadow_instances(&mut self, instances: &[crate::ShadowInstance]) {
        self.shadow_instances = instances.to_vec();
    }

    /// Toggle logical pixel mode.
    pub fn set_logical_pixels(&mut self, on: bool) {
        self.logical_pixels = on;
    }
}
