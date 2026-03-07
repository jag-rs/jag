use std::sync::Arc;

// use anyhow::Result;

use crate::allocator::{RenderAllocator, TexKey};
// use crate::display_list::{Command, DisplayList, Viewport};
use crate::pipeline::{
    BackgroundRenderer, BasicSolidRenderer, Blitter, BlurRenderer, Compositor,
    OverlaySolidRenderer, ScrimSolidRenderer, ScrimStencilMaskRenderer, ScrimStencilRenderer,
    ShadowCompositeRenderer, SmaaRenderer, TextRenderer,
};
use crate::scene::{BoxShadowSpec, RoundedRadii, RoundedRect};
use crate::upload::GpuScene;

/// Apply a 2D affine transform to a point
fn apply_transform_to_point(point: [f32; 2], transform: crate::Transform2D) -> [f32; 2] {
    let [a, b, c, d, e, f] = transform.m;
    let x = point[0];
    let y = point[1];
    [a * x + c * y + e, b * x + d * y + f]
}

fn u16_unorm_to_u8(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32767) / 65535) as u8
}

fn glyph_mask_for_atlas(
    mask: &crate::text::GlyphMask,
    force_grayscale: bool,
) -> (u32, u32, std::borrow::Cow<'_, [u8]>) {
    match mask {
        crate::text::GlyphMask::Color(m) => {
            (m.width, m.height, std::borrow::Cow::Borrowed(&m.data))
        }
        crate::text::GlyphMask::Subpixel(m) => match m.format {
            crate::text::MaskFormat::Rgba8 => {
                if !force_grayscale {
                    return (m.width, m.height, std::borrow::Cow::Borrowed(&m.data));
                }

                let mut out = Vec::with_capacity((m.width as usize) * (m.height as usize) * 4);
                for px in m.data.chunks_exact(4) {
                    let gray = ((u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3) as u8;
                    out.extend_from_slice(&[gray, gray, gray, 0]);
                }
                (m.width, m.height, std::borrow::Cow::Owned(out))
            }
            crate::text::MaskFormat::Rgba16 => {
                // Text atlas is RGBA8, so normalize 16-bit glyph masks to RGBA8 on upload.
                let mut out = Vec::with_capacity((m.width as usize) * (m.height as usize) * 4);
                for px in m.data.chunks_exact(8) {
                    let r = u16::from_le_bytes([px[0], px[1]]);
                    let g = u16::from_le_bytes([px[2], px[3]]);
                    let b = u16::from_le_bytes([px[4], px[5]]);
                    let (r8, g8, b8) = if force_grayscale {
                        let gray16 = ((u32::from(r) + u32::from(g) + u32::from(b)) / 3) as u16;
                        let gray8 = u16_unorm_to_u8(gray16);
                        (gray8, gray8, gray8)
                    } else {
                        (u16_unorm_to_u8(r), u16_unorm_to_u8(g), u16_unorm_to_u8(b))
                    };
                    out.extend_from_slice(&[r8, g8, b8, 0]);
                }
                (m.width, m.height, std::borrow::Cow::Owned(out))
            }
        },
    }
}

pub struct PassTargets {
    pub color: crate::OwnedTexture,
}

pub enum Background {
    Solid(crate::scene::ColorLinPremul),
    LinearGradient {
        start_uv: [f32; 2],
        end_uv: [f32; 2],
        stop0: (f32, crate::scene::ColorLinPremul),
        stop1: (f32, crate::scene::ColorLinPremul),
    },
}

pub struct PassManager {
    device: Arc<wgpu::Device>,
    pub solid_offscreen: BasicSolidRenderer,
    pub solid_direct: BasicSolidRenderer,
    pub transparent_solid_offscreen: BasicSolidRenderer,
    pub transparent_solid_direct: BasicSolidRenderer,
    pub solid_direct_no_msaa: BasicSolidRenderer,
    overlay_solid: OverlaySolidRenderer,
    scrim_solid: ScrimSolidRenderer,
    pub compositor: Compositor,
    pub blitter: Blitter,
    pub smaa: SmaaRenderer,
    scrim_mask: ScrimStencilMaskRenderer,
    scrim_stencil: ScrimStencilRenderer,
    // Shadow/blur pipelines and helpers
    pub mask_renderer: BasicSolidRenderer,
    pub blur_r8: BlurRenderer,
    pub shadow_comp: ShadowCompositeRenderer,
    pub text: TextRenderer,
    pub text_offscreen: TextRenderer,
    pub image: crate::pipeline::ImageRenderer,
    pub image_offscreen: crate::pipeline::ImageRenderer,
    pub svg_cache: crate::svg::SvgRasterCache,
    pub image_cache: crate::image_cache::ImageCache,
    offscreen_format: wgpu::TextureFormat,
    surface_format: wgpu::TextureFormat,
    vp_buffer: wgpu::Buffer,
    /// Scroll offset applied via the viewport uniform (GPU-side scroll).
    /// Set by the caller before `render_unified` to shift content without
    /// rebuilding geometry. Values are in logical pixels (negative = scrolled down/right).
    scroll_offset: [f32; 2],
    // Z-index uniform buffer for dynamic depth control (Phase 2)
    z_index_buffer: wgpu::Buffer,
    bg: BackgroundRenderer,
    bg_param_buffer: wgpu::Buffer,
    bg_stops_buffer: wgpu::Buffer,
    // Platform DPI scale factor (used for mac-specific radial centering fix)
    scale_factor: f32,
    // Additional UI scale multiplier for logical pixel mode
    ui_scale: f32,
    // When true, treat positions as logical pixels and scale by `scale_factor` centrally
    logical_pixels: bool,
    // Intermediate texture for Vello-style smooth resizing
    pub intermediate_texture: Option<crate::OwnedTexture>,
    smaa_edges: Option<crate::OwnedTexture>,
    smaa_weights: Option<crate::OwnedTexture>,
    // Depth texture for z-ordering across all element types
    depth_texture: Option<crate::OwnedTexture>,
    // Stencil texture for scrim cutouts
    scrim_stencil_tex: Option<crate::OwnedTexture>,
    // Reusable GPU resources for text rendering to avoid per-glyph allocations.
    text_mask_atlas: wgpu::Texture,
    // Note: This view is not directly read but must be kept alive for the bind group reference
    #[allow(dead_code)]
    text_mask_atlas_view: wgpu::TextureView,
    text_bind_group: wgpu::BindGroup,
    // Track atlas region used in previous frame for efficient clearing
    prev_atlas_max_x: u32,
    prev_atlas_max_y: u32,
    smaa_param_buffer: wgpu::Buffer,
    // Registry for externally-rendered textures (e.g., 3D viewports)
    external_textures:
        std::collections::HashMap<crate::display_list::ExternalTextureId, wgpu::TextureView>,
}

// Vertex structures for unified rendering
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct TextQuadVtx {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct ImageQuadVtx {
    pos: [f32; 2],
    uv: [f32; 2],
}

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
        let shadow_comp = ShadowCompositeRenderer::new(device.clone(), target_format);
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
            shadow_comp,
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
    fn create_group_z_bind_group(
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

    /// Render an image texture to the target at origin with size (in pixels, y-down).
    /// Expects `tex_view` to be created from an `Rgba8UnormSrgb` texture for proper sRGB sampling.
    pub fn draw_image_quad(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        origin: [f32; 2],
        size: [f32; 2],
        tex_view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) {
        // Update viewport uniform based on render target dimensions (+ logical pixel scale)
        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        let scale = [
            (2.0f32 / (width.max(1) as f32)) * logical,
            (-2.0f32 / (height.max(1) as f32)) * logical,
        ];
        let translate = [-1.0f32, 1.0f32];
        let vp_data: [f32; 8] = [
            scale[0],
            scale[1],
            translate[0],
            translate[1],
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        // debug log removed
        queue.write_buffer(&self.vp_buffer, 0, bytemuck::bytes_of(&vp_data));

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct QuadVtx {
            pos: [f32; 2],
            uv: [f32; 2],
        }
        let x = origin[0];
        let y = origin[1];
        let w = size[0].max(0.0);
        let h = size[1].max(0.0);
        let verts = [
            QuadVtx {
                pos: [x, y],
                uv: [0.0, 0.0],
            },
            QuadVtx {
                pos: [x + w, y],
                uv: [1.0, 0.0],
            },
            QuadVtx {
                pos: [x + w, y + h],
                uv: [1.0, 1.0],
            },
            QuadVtx {
                pos: [x, y + h],
                uv: [0.0, 1.0],
            },
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let vsize = (verts.len() * std::mem::size_of::<QuadVtx>()) as u64;
        let isize = (idx.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));
        }

        let vp_bg = self.image.vp_bind_group(&self.device, &self.vp_buffer);
        let z_bg = self.create_z_bind_group(0.0, queue);
        let tex_bg = self.image.tex_bind_group(&self.device, tex_view);
        let (params_bg, _params_buf) = self.image.params_bind_group(&self.device, 1.0, false);

        // Create depth texture for image rendering (1x)
        let depth_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image-depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_attachment = Some(wgpu::RenderPassDepthStencilAttachment {
            view: &depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load, // Preserve existing depth values
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("image-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: depth_attachment,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.image.record(
            &mut pass,
            &vp_bg,
            &z_bg,
            &tex_bg,
            &params_bg,
            &vbuf,
            &ibuf,
            idx.len() as u32,
        );
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
    pub fn request_image_load(&mut self, path: &std::path::Path) {
        self.image_cache.start_load(path);
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

    /// Toggle logical pixel mode.
    pub fn set_logical_pixels(&mut self, on: bool) {
        self.logical_pixels = on;
    }

    pub fn alloc_targets(
        &self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) -> PassTargets {
        let color = allocator.allocate_texture(TexKey {
            width,
            height,
            format: self.offscreen_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        PassTargets { color }
    }

    /// Allocate or reuse intermediate texture matching the surface size.
    /// This texture is used for Vello-style smooth resizing.
    ///
    /// Strategy: Always ensure texture matches exact size for MSAA resolve compatibility.
    /// We preserve content by using LoadOp::Load when rendering, not by keeping oversized textures.
    pub fn ensure_intermediate_texture(
        &mut self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) {
        let needs_realloc = match &self.intermediate_texture {
            Some(tex) => {
                // Reallocate if size doesn't match exactly
                // MSAA resolve requires exact size match between MSAA texture and resolve target
                tex.key.width != width || tex.key.height != height
            }
            None => true,
        };

        if needs_realloc {
            // Release old texture if it exists
            if let Some(old_tex) = self.intermediate_texture.take() {
                allocator.release_texture(old_tex);
            }

            // Allocate new intermediate texture with surface format at exact size
            let tex = allocator.allocate_texture(TexKey {
                width,
                height,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
            });
            self.intermediate_texture = Some(tex);
        }
    }

    /// Clear the intermediate texture with the specified color.
    /// This should be called before rendering to the intermediate texture.
    pub fn clear_intermediate_texture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        clear_color: wgpu::Color,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before clearing");

        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear-intermediate"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &intermediate.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
    }

    /// Blit the intermediate texture to the surface. This is a very fast operation
    /// that enables smooth window resizing (Vello-style).
    pub fn blit_to_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before blitting");

        let bg = self.blitter.bind_group(&self.device, &intermediate.view);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.blitter.record(&mut pass, &bg);
    }

    /// Ensure depth texture is allocated and matches the given dimensions.
    /// Depth texture is used for z-ordering across all element types (solids, text, images, SVGs).
    pub fn ensure_depth_texture(
        &mut self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) {
        let needs_realloc = match &self.depth_texture {
            Some(tex) => tex.key.width != width || tex.key.height != height,
            None => true,
        };

        if needs_realloc {
            // Release old texture if it exists
            if let Some(old_tex) = self.depth_texture.take() {
                allocator.release_texture(old_tex);
            }

            // Allocate new depth texture at exact size
            let tex = allocator.allocate_texture(TexKey {
                width,
                height,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            });
            self.depth_texture = Some(tex);
        }
    }

    /// Get the depth texture view for use in render passes.
    /// Panics if depth texture hasn't been allocated via ensure_depth_texture.
    pub fn depth_view(&self) -> &wgpu::TextureView {
        &self
            .depth_texture
            .as_ref()
            .expect("depth texture must be allocated before use")
            .view
    }

    fn ensure_scrim_stencil_texture(
        &mut self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) {
        let needs_realloc = match &self.scrim_stencil_tex {
            Some(tex) => tex.key.width != width || tex.key.height != height,
            None => true,
        };

        if needs_realloc {
            if let Some(old) = self.scrim_stencil_tex.take() {
                allocator.release_texture(old);
            }
            let tex = allocator.allocate_texture(TexKey {
                width,
                height,
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            });
            self.scrim_stencil_tex = Some(tex);
        }
    }

    fn ensure_smaa_textures(&mut self, allocator: &mut RenderAllocator, width: u32, height: u32) {
        let key = TexKey {
            width,
            height,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        };

        if self.smaa_edges.as_ref().map_or(true, |tex| tex.key != key) {
            if let Some(old) = self.smaa_edges.take() {
                allocator.release_texture(old);
            }
            self.smaa_edges = Some(allocator.allocate_texture(key));
        }

        if self
            .smaa_weights
            .as_ref()
            .map_or(true, |tex| tex.key != key)
        {
            if let Some(old) = self.smaa_weights.take() {
                allocator.release_texture(old);
            }
            self.smaa_weights = Some(allocator.allocate_texture(key));
        }
    }

    pub fn apply_smaa(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        allocator: &mut RenderAllocator,
        src_view: &wgpu::TextureView,
        dst_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        queue: &wgpu::Queue,
    ) {
        if width == 0 || height == 0 {
            return;
        }

        self.ensure_smaa_textures(allocator, width, height);
        let texel_size = [
            1.0f32 / width.max(1) as f32,
            1.0f32 / height.max(1) as f32,
            0.0,
            0.0,
        ];
        queue.write_buffer(&self.smaa_param_buffer, 0, bytemuck::bytes_of(&texel_size));

        let edges = self
            .smaa_edges
            .as_ref()
            .expect("SMAA edges texture must exist");
        let weights = self
            .smaa_weights
            .as_ref()
            .expect("SMAA weights texture must exist");

        let edge_bg = self
            .smaa
            .edge_bind_group(&self.device, src_view, &self.smaa_param_buffer);
        let blend_bg =
            self.smaa
                .blend_bind_group(&self.device, &edges.view, &self.smaa_param_buffer);
        let resolve_bg = self.smaa.resolve_bind_group(
            &self.device,
            src_view,
            &weights.view,
            &self.smaa_param_buffer,
        );

        // Pass 1: edge detect
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("smaa-edge-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &edges.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.smaa.record_edges(&mut pass, &edge_bg);
        }

        // Pass 2: blend weights
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("smaa-blend-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &weights.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.smaa.record_blend(&mut pass, &blend_bg);
        }

        // Pass 3: resolve onto the swapchain/offscreen destination
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("smaa-resolve-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.smaa.record_resolve(&mut pass, &resolve_bg);
        }
    }

    /// Draw a box shadow for a rounded rect using an R8 mask + separable Gaussian blur pipeline.
    /// This composes the tinted shadow beneath current content on the target view.
    pub fn draw_box_shadow(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        rrect: RoundedRect,
        spec: BoxShadowSpec,
        queue: &wgpu::Queue,
    ) {
        // --- 1) Calibrate parameters ---
        // Soften falloff: browsers feel closer to sigma ≈ blur_radius
        // Larger sigma reduces the "band" look and increases penumbra.
        let blur = spec.blur_radius.max(0.0);
        let sigma = if blur > 0.0 { blur } else { 0.5 };
        let spread = spec.spread.max(0.0);
        let create_tex = |label: &str| -> wgpu::Texture {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let mask_tex = create_tex("shadow-mask");
        let ping_tex = create_tex("shadow-ping");
        let mask_view = mask_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let ping_view = ping_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // Viewport for full target size (y-down)
        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        let scale = [
            (2.0f32 / (width.max(1) as f32)) * logical,
            (-2.0f32 / (height.max(1) as f32)) * logical,
        ];
        let translate = [-1.0f32, 1.0f32];
        let vp_data: [f32; 8] = [
            scale[0],
            scale[1],
            translate[0],
            translate[1],
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        // debug log removed
        queue.write_buffer(&self.vp_buffer, 0, bytemuck::bytes_of(&vp_data));

        let shadow_radii = RoundedRadii {
            tl: (rrect.radii.tl + spread).max(0.0),
            tr: (rrect.radii.tr + spread).max(0.0),
            br: (rrect.radii.br + spread).max(0.0),
            bl: (rrect.radii.bl + spread).max(0.0),
        };
        // Expand source to give blur room so the outer halo is broad enough.
        // Slightly higher multiplier works better with the wider blur support above.
        let expand = spread + 1.8 * sigma + 1.0;
        let mut rect = rrect.rect;
        rect.x = rect.x + spec.offset[0] - expand;
        rect.y = rect.y + spec.offset[1] - expand;
        rect.w = (rect.w + 2.0 * expand).max(0.0);
        rect.h = (rect.h + 2.0 * expand).max(0.0);
        let expanded = RoundedRect {
            rect,
            radii: shadow_radii,
        };
        // Render with white for the shadow shape
        // Build vertices/indices for expanded rounded rect (fill)
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Vtx {
            pos: [f32; 2],
            color: [f32; 4],
        }
        let mut vertices: Vec<Vtx> = Vec::new();
        let mut indices: Vec<u16> = Vec::new();
        let rect = expanded.rect;
        let tl = expanded.radii.tl.min(rect.w * 0.5).min(rect.h * 0.5);
        let tr = expanded.radii.tr.min(rect.w * 0.5).min(rect.h * 0.5);
        let br = expanded.radii.br.min(rect.w * 0.5).min(rect.h * 0.5);
        let bl = expanded.radii.bl.min(rect.w * 0.5).min(rect.h * 0.5);
        // Higher tessellation for smoother rounded corners (reduces polygonal artifacts before blur)
        let segs = 64u32;
        let mut ring: Vec<[f32; 2]> = Vec::new();
        fn arc_append(
            ring: &mut Vec<[f32; 2]>,
            c: [f32; 2],
            r: f32,
            start: f32,
            end: f32,
            segs: u32,
            include_start: bool,
        ) {
            if r <= 0.0 {
                return;
            }
            for i in 0..=segs {
                if i == 0 && !include_start {
                    continue;
                }
                let t = (i as f32) / (segs as f32);
                let ang = start + t * (end - start);
                let p = [c[0] + r * ang.cos(), c[1] - r * ang.sin()];
                ring.push(p);
            }
        }
        if tl > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + tl, rect.y + tl],
                tl,
                std::f32::consts::FRAC_PI_2,
                std::f32::consts::PI,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + 0.0, rect.y + 0.0]);
        }
        if bl > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + bl, rect.y + rect.h - bl],
                bl,
                std::f32::consts::PI,
                std::f32::consts::FRAC_PI_2 * 3.0,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + 0.0, rect.y + rect.h]);
        }
        if br > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + rect.w - br, rect.y + rect.h - br],
                br,
                std::f32::consts::FRAC_PI_2 * 3.0,
                std::f32::consts::TAU,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + rect.w, rect.y + rect.h]);
        }
        if tr > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + rect.w - tr, rect.y + tr],
                tr,
                0.0,
                std::f32::consts::FRAC_PI_2,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + rect.w, rect.y + 0.0]);
        }
        let center = [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5];
        let white = [1.0, 1.0, 1.0, 1.0];
        let base = vertices.len() as u16;
        vertices.push(Vtx {
            pos: center,
            color: white,
        });
        for p in ring.iter() {
            vertices.push(Vtx {
                pos: *p,
                color: white,
            });
        }
        let ring_len = (vertices.len() as u16) - base - 1;
        for i in 0..ring_len {
            let i0 = base;
            let i1 = base + 1 + i;
            let i2 = base + 1 + ((i + 1) % ring_len);
            indices.extend_from_slice(&[i0, i1, i2]);
        }
        // Create GPU buffers directly
        let vsize = (vertices.len() * std::mem::size_of::<Vtx>()) as u64;
        let isize = (indices.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow-mask-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow-mask-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&vertices));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&indices));
        }
        let gpu = crate::upload::GpuScene {
            vertex: crate::allocator::OwnedBuffer {
                buffer: vbuf,
                key: crate::allocator::BufKey {
                    size: vsize.max(4),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                },
            },
            index: crate::allocator::OwnedBuffer {
                buffer: ibuf,
                key: crate::allocator::BufKey {
                    size: isize.max(4),
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                },
            },
            vertices: vertices.len() as u32,
            indices: indices.len() as u32,
        };

        // Bind groups for viewport
        let vp_bg_mask = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vp-bg-mask"),
            layout: self.mask_renderer.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });
        // Render mask shape to R8 texture
        // Clear to BLACK, render WHITE for shadow shape
        // After blur: soft white blob. After cutout: white ring (shadow area)
        let _z_bg = self.create_z_bind_group(0.0, queue);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow-mask-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &mask_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.mask_renderer.record(&mut pass, &vp_bg_mask, &gpu);
        }

        // Horizontal blur (mask -> ping)
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BlurParams {
            dir: [f32; 2],
            texel: [f32; 2],
            sigma: f32,
            _pad: f32,
        }
        let texel = [
            1.0f32 / (width.max(1) as f32),
            1.0f32 / (height.max(1) as f32),
        ];
        let bp_h = BlurParams {
            dir: [1.0, 0.0],
            texel,
            sigma,
            _pad: 0.0,
        };
        queue.write_buffer(&self.blur_r8.param_buffer, 0, bytemuck::bytes_of(&bp_h));
        let bg_h = self.blur_r8.bind_group(&self.device, &mask_view);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow-blur-h"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &ping_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.blur_r8.record(&mut pass, &bg_h);
        }

        // Vertical blur (ping -> mask)
        let bp_v = BlurParams {
            dir: [0.0, 1.0],
            texel,
            sigma,
            _pad: 0.0,
        };
        queue.write_buffer(&self.blur_r8.param_buffer, 0, bytemuck::bytes_of(&bp_v));
        let bg_v = self.blur_r8.bind_group(&self.device, &ping_view);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow-blur-v"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &mask_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.blur_r8.record(&mut pass, &bg_v);
        }

        // Step 5: Cut out the ORIGINAL shape (at original position, no offset)
        // This prevents the shadow from showing through semi-transparent elements
        {
            let mut cutout_vertices: Vec<Vtx> = Vec::new();
            let mut cutout_indices: Vec<u16> = Vec::new();
            // Use ORIGINAL rect (no spread/offset) in full target space
            let rect = rrect.rect;
            let tl = rrect.radii.tl.min(rect.w * 0.5).min(rect.h * 0.5);
            let tr = rrect.radii.tr.min(rect.w * 0.5).min(rect.h * 0.5);
            let br = rrect.radii.br.min(rect.w * 0.5).min(rect.h * 0.5);
            let bl = rrect.radii.bl.min(rect.w * 0.5).min(rect.h * 0.5);
            let mut ring: Vec<[f32; 2]> = Vec::new();
            if tl > 0.0 {
                arc_append(
                    &mut ring,
                    [rect.x + tl, rect.y + tl],
                    tl,
                    std::f32::consts::FRAC_PI_2,
                    std::f32::consts::PI,
                    segs,
                    true,
                );
            } else {
                ring.push([rect.x, rect.y]);
            }
            if bl > 0.0 {
                arc_append(
                    &mut ring,
                    [rect.x + bl, rect.y + rect.h - bl],
                    bl,
                    std::f32::consts::PI,
                    std::f32::consts::FRAC_PI_2 * 3.0,
                    segs,
                    true,
                );
            } else {
                ring.push([rect.x, rect.y + rect.h]);
            }
            if br > 0.0 {
                arc_append(
                    &mut ring,
                    [rect.x + rect.w - br, rect.y + rect.h - br],
                    br,
                    std::f32::consts::FRAC_PI_2 * 3.0,
                    std::f32::consts::TAU,
                    segs,
                    true,
                );
            } else {
                ring.push([rect.x + rect.w, rect.y + rect.h]);
            }
            if tr > 0.0 {
                arc_append(
                    &mut ring,
                    [rect.x + rect.w - tr, rect.y + tr],
                    tr,
                    0.0,
                    std::f32::consts::FRAC_PI_2,
                    segs,
                    true,
                );
            } else {
                ring.push([rect.x + rect.w, rect.y]);
            }
            let center = [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5];
            // Use transparent (alpha=0) to clear the mask area
            // With premultiplied alpha: result = src * src.a + dst * (1 - src.a) = 0 * 0 + dst * 1 = dst
            // That won't work! We need alpha=1 to replace: result = src * 1 + dst * 0 = src
            // For R8, we want to write 0.0, so use black with alpha=1
            let clear_color = [0.0, 0.0, 0.0, 1.0];
            let base = cutout_vertices.len() as u16;
            cutout_vertices.push(Vtx {
                pos: center,
                color: clear_color,
            });
            for p in ring.iter() {
                cutout_vertices.push(Vtx {
                    pos: *p,
                    color: clear_color,
                });
            }
            let ring_len = (cutout_vertices.len() as u16) - base - 1;
            for i in 0..ring_len {
                let i0 = base;
                let i1 = base + 1 + i;
                let i2 = base + 1 + ((i + 1) % ring_len);
                cutout_indices.extend_from_slice(&[i0, i1, i2]);
            }

            let vsize = (cutout_vertices.len() * std::mem::size_of::<Vtx>()) as u64;
            let isize = (cutout_indices.len() * std::mem::size_of::<u16>()) as u64;
            let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shadow-cutout-vbuf"),
                size: vsize.max(4),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shadow-cutout-ibuf"),
                size: isize.max(4),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if vsize > 0 {
                queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&cutout_vertices));
            }
            if isize > 0 {
                queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&cutout_indices));
            }
            let cutout_gpu = crate::upload::GpuScene {
                vertex: crate::allocator::OwnedBuffer {
                    buffer: vbuf,
                    key: crate::allocator::BufKey {
                        size: vsize.max(4),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    },
                },
                index: crate::allocator::OwnedBuffer {
                    buffer: ibuf,
                    key: crate::allocator::BufKey {
                        size: isize.max(4),
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    },
                },
                vertices: cutout_vertices.len() as u32,
                indices: cutout_indices.len() as u32,
            };

            let _z_bg_cutout = self.create_z_bind_group(0.0, queue);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow-cutout"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &mask_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.mask_renderer
                .record(&mut pass, &vp_bg_mask, &cutout_gpu);
        }

        // Composite tinted shadow to target using premultiplied color
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct ShadowColor {
            color: [f32; 4],
        }
        let c = spec.color;
        let scol = ShadowColor {
            color: [c.r, c.g, c.b, c.a],
        };
        queue.write_buffer(&self.shadow_comp.color_buffer, 0, bytemuck::bytes_of(&scol));
        let bg = self.shadow_comp.bind_group(&self.device, &mask_view);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shadow-composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.shadow_comp.record(&mut pass, &bg);
        }

        // Temp textures are dropped at end of scope
    }

    /// Draw a simple overlay rectangle that darkens existing content without affecting depth.
    /// This is intended for UI overlays like modal scrims that should blend over the scene
    /// but not participate in depth testing.
    pub fn draw_overlay_rect(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        rect: crate::scene::Rect,
        color: crate::scene::ColorLinPremul,
        queue: &wgpu::Queue,
    ) {
        // Update viewport uniform based on render target dimensions (+ logical pixel scale)
        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        let scale = [
            (2.0f32 / (width.max(1) as f32)) * logical,
            (-2.0f32 / (height.max(1) as f32)) * logical,
        ];
        let translate = [-1.0f32, 1.0f32];
        let vp_data: [f32; 8] = [
            scale[0],
            scale[1],
            translate[0],
            translate[1],
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        queue.write_buffer(&self.vp_buffer, 0, bytemuck::bytes_of(&vp_data));

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct OverlayVtx {
            pos: [f32; 2],
            color: [f32; 4],
            z_index: f32,
        }

        let overlay_color = [color.r, color.g, color.b, color.a];
        let z_index = 0.0f32;
        let x = rect.x;
        let y = rect.y;
        let w = rect.w.max(0.0);
        let h = rect.h.max(0.0);

        // Skip degenerate rectangles
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        let verts = [
            OverlayVtx {
                pos: [x, y],
                color: overlay_color,
                z_index,
            },
            OverlayVtx {
                pos: [x + w, y],
                color: overlay_color,
                z_index,
            },
            OverlayVtx {
                pos: [x + w, y + h],
                color: overlay_color,
                z_index,
            },
            OverlayVtx {
                pos: [x, y + h],
                color: overlay_color,
                z_index,
            },
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

        let vsize = (verts.len() * std::mem::size_of::<OverlayVtx>()) as u64;
        let isize = (idx.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay-rect-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay-rect-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));
        }

        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay-vp-bg"),
            layout: self.overlay_solid.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Overlay pass: no depth attachment so the quad simply blends over existing content.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay-rect-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.overlay_solid
            .record(&mut pass, &vp_bg, &vbuf, &ibuf, idx.len() as u32);
    }

    /// Draw a full-viewport scrim rectangle that blends over existing content.
    /// Unlike draw_overlay_rect, this uses a depth buffer attachment but with:
    /// - depth_write_enabled = false (doesn't affect depth buffer)
    /// - depth_compare = Always (always passes depth test)
    /// This allows the scrim to render over all existing content while letting
    /// subsequent draws at higher z-index render on top of the scrim.
    ///
    /// NOTE: Scrim renders directly to target without MSAA or depth attachment.
    /// The scrim pipeline uses depth_compare=Always and depth_write_enabled=false.
    pub fn draw_scrim_rect(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        rect: crate::scene::Rect,
        color: crate::scene::ColorLinPremul,
        queue: &wgpu::Queue,
    ) {
        // Update viewport uniform based on render target dimensions (+ logical pixel scale)
        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        let scale = [
            (2.0f32 / (width.max(1) as f32)) * logical,
            (-2.0f32 / (height.max(1) as f32)) * logical,
        ];
        let translate = [-1.0f32, 1.0f32];
        let vp_data: [f32; 8] = [
            scale[0],
            scale[1],
            translate[0],
            translate[1],
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        queue.write_buffer(&self.vp_buffer, 0, bytemuck::bytes_of(&vp_data));

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct ScrimVtx {
            pos: [f32; 2],
            color: [f32; 4],
            z_index: f32,
        }

        let scrim_color = [color.r, color.g, color.b, color.a];
        // Use a middle z-index - the scrim pipeline ignores depth testing anyway
        let z_index = 0.5f32;
        let x = rect.x;
        let y = rect.y;
        let w = rect.w.max(0.0);
        let h = rect.h.max(0.0);

        // Skip degenerate rectangles
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        let verts = [
            ScrimVtx {
                pos: [x, y],
                color: scrim_color,
                z_index,
            },
            ScrimVtx {
                pos: [x + w, y],
                color: scrim_color,
                z_index,
            },
            ScrimVtx {
                pos: [x + w, y + h],
                color: scrim_color,
                z_index,
            },
            ScrimVtx {
                pos: [x, y + h],
                color: scrim_color,
                z_index,
            },
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

        let vsize = (verts.len() * std::mem::size_of::<ScrimVtx>()) as u64;
        let isize = (idx.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scrim-rect-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scrim-rect-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));
        }

        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scrim-vp-bg"),
            layout: self.scrim_solid.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Scrim pass: no depth attachment. The scrim pipeline is configured with
        // depth_compare=Always and depth_write_enabled=false, so depth isn't needed.
        // It simply blends over existing content without affecting any depth state.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scrim-rect-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.scrim_solid
            .record(&mut pass, &vp_bg, &vbuf, &ibuf, idx.len() as u32);
    }

    /// Draw a full scrim but cut out a rounded-rect hole via stencil.
    pub fn draw_scrim_with_cutout(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        allocator: &mut RenderAllocator,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        hole: RoundedRect,
        color: crate::scene::ColorLinPremul,
        queue: &wgpu::Queue,
    ) {
        self.ensure_scrim_stencil_texture(allocator, width, height);
        let stencil_tex = self
            .scrim_stencil_tex
            .as_ref()
            .expect("stencil texture must exist");

        // Update viewport uniform
        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        let scale = [
            (2.0f32 / (width.max(1) as f32)) * logical,
            (-2.0f32 / (height.max(1) as f32)) * logical,
        ];
        let translate = [-1.0f32, 1.0f32];
        let vp_data: [f32; 8] = [
            scale[0],
            scale[1],
            translate[0],
            translate[1],
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        queue.write_buffer(&self.vp_buffer, 0, bytemuck::bytes_of(&vp_data));

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Vtx {
            pos: [f32; 2],
            color: [f32; 4],
            z: f32,
        }

        // Tessellate filled rounded rect (copied from draw_filled_rounded_rect)
        let mut vertices: Vec<Vtx> = Vec::new();
        let mut indices: Vec<u16> = Vec::new();
        let rect = hole.rect;
        let tl = hole.radii.tl.min(rect.w * 0.5).min(rect.h * 0.5);
        let tr = hole.radii.tr.min(rect.w * 0.5).min(rect.h * 0.5);
        let br = hole.radii.br.min(rect.w * 0.5).min(rect.h * 0.5);
        let bl = hole.radii.bl.min(rect.w * 0.5).min(rect.h * 0.5);
        let segs = 32u32;
        let mut ring: Vec<[f32; 2]> = Vec::new();
        fn arc_append(
            ring: &mut Vec<[f32; 2]>,
            c: [f32; 2],
            r: f32,
            start: f32,
            end: f32,
            segs: u32,
            include_start: bool,
        ) {
            if r <= 0.0 {
                return;
            }
            for i in 0..=segs {
                if i == 0 && !include_start {
                    continue;
                }
                let t = (i as f32) / (segs as f32);
                let ang = start + t * (end - start);
                let p = [c[0] + r * ang.cos(), c[1] - r * ang.sin()];
                ring.push(p);
            }
        }
        if tl > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + tl, rect.y + tl],
                tl,
                std::f32::consts::FRAC_PI_2,
                std::f32::consts::PI,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + 0.0, rect.y + 0.0]);
        }
        if bl > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + bl, rect.y + rect.h - bl],
                bl,
                std::f32::consts::PI,
                std::f32::consts::FRAC_PI_2 * 3.0,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + 0.0, rect.y + rect.h]);
        }
        if br > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + rect.w - br, rect.y + rect.h - br],
                br,
                std::f32::consts::FRAC_PI_2 * 3.0,
                std::f32::consts::TAU,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + rect.w, rect.y + rect.h]);
        }
        if tr > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + rect.w - tr, rect.y + tr],
                tr,
                0.0,
                std::f32::consts::FRAC_PI_2,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + rect.w, rect.y + 0.0]);
        }

        // Triangulate fan
        let center = [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5];
        vertices.push(Vtx {
            pos: center,
            color: [color.r, color.g, color.b, color.a],
            z: 0.5,
        });
        for p in ring.iter() {
            vertices.push(Vtx {
                pos: *p,
                color: [color.r, color.g, color.b, color.a],
                z: 0.5,
            });
        }
        // Triangle fan around center
        for i in 1..(vertices.len() - 1) {
            indices.extend_from_slice(&[0, i as u16, (i as u16) + 1]);
        }
        if vertices.len() > 2 {
            indices.extend_from_slice(&[0, (vertices.len() - 1) as u16, 1]);
        }

        // Ensure index byte length is 4-byte aligned for write_buffer
        if indices.len() % 2 != 0 {
            indices.push(*indices.last().unwrap_or(&0));
        }

        let vsize = (vertices.len() * std::mem::size_of::<Vtx>()) as u64;
        let isize = (indices.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scrim-hole-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scrim-hole-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&vertices));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&indices));
        }

        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scrim-stencil-vp-bg"),
            layout: self.scrim_mask.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Pass 1: write stencil = 1 inside hole (color writes disabled)
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scrim-stencil-mask-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &stencil_tex.view,
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_stencil_reference(1);
            self.scrim_mask
                .record(&mut pass, &vp_bg, &vbuf, &ibuf, indices.len() as u32);
        }

        // Fullscreen quad for scrim (cover entire viewport)
        let quad = [
            Vtx {
                pos: [0.0, 0.0],
                color: [color.r, color.g, color.b, color.a],
                z: 0.5,
            },
            Vtx {
                pos: [width as f32, 0.0],
                color: [color.r, color.g, color.b, color.a],
                z: 0.5,
            },
            Vtx {
                pos: [width as f32, height as f32],
                color: [color.r, color.g, color.b, color.a],
                z: 0.5,
            },
            Vtx {
                pos: [0.0, height as f32],
                color: [color.r, color.g, color.b, color.a],
                z: 0.5,
            },
        ];
        let quad_idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let qvbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scrim-fullscreen-vbuf"),
            size: (quad.len() * std::mem::size_of::<Vtx>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let qibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scrim-fullscreen-ibuf"),
            size: (quad_idx.len() * std::mem::size_of::<u16>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&qvbuf, 0, bytemuck::cast_slice(&quad));
        queue.write_buffer(&qibuf, 0, bytemuck::cast_slice(&quad_idx));

        let vp_bg_scrim = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scrim-stencil-vp-bg-scrim"),
            layout: self.scrim_stencil.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Pass 2: draw scrim where stencil == 0 (outside hole)
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scrim-stencil-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &stencil_tex.view,
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_stencil_reference(0);
            self.scrim_stencil.record(
                &mut pass,
                &vp_bg_scrim,
                &qvbuf,
                &qibuf,
                quad_idx.len() as u32,
            );
        }
    }

    /// Draw a filled rounded rectangle directly onto the target using the solid_direct pipeline.
    /// Uses premultiplied linear color.
    pub fn draw_filled_rounded_rect(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        rrect: RoundedRect,
        color: crate::scene::ColorLinPremul,
        queue: &wgpu::Queue,
    ) {
        // Update viewport uniform
        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        let scale = [
            (2.0f32 / (width.max(1) as f32)) * logical,
            (-2.0f32 / (height.max(1) as f32)) * logical,
        ];
        let translate = [-1.0f32, 1.0f32];
        let vp_data: [f32; 8] = [
            scale[0],
            scale[1],
            translate[0],
            translate[1],
            0.0,
            0.0,
            0.0,
            0.0,
        ];
        // debug log removed
        queue.write_buffer(&self.vp_buffer, 0, bytemuck::bytes_of(&vp_data));

        // Tessellate rounded rect fill
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Vtx {
            pos: [f32; 2],
            color: [f32; 4],
        }
        let mut vertices: Vec<Vtx> = Vec::new();
        let mut indices: Vec<u16> = Vec::new();
        let rect = rrect.rect;
        let tl = rrect.radii.tl.min(rect.w * 0.5).min(rect.h * 0.5);
        let tr = rrect.radii.tr.min(rect.w * 0.5).min(rect.h * 0.5);
        let br = rrect.radii.br.min(rect.w * 0.5).min(rect.h * 0.5);
        let bl = rrect.radii.bl.min(rect.w * 0.5).min(rect.h * 0.5);
        let segs = 64u32;
        let mut ring: Vec<[f32; 2]> = Vec::new();
        fn arc_append(
            ring: &mut Vec<[f32; 2]>,
            c: [f32; 2],
            r: f32,
            start: f32,
            end: f32,
            segs: u32,
            include_start: bool,
        ) {
            if r <= 0.0 {
                return;
            }
            for i in 0..=segs {
                if i == 0 && !include_start {
                    continue;
                }
                let t = (i as f32) / (segs as f32);
                let ang = start + t * (end - start);
                let p = [c[0] + r * ang.cos(), c[1] - r * ang.sin()];
                ring.push(p);
            }
        }
        if tl > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + tl, rect.y + tl],
                tl,
                std::f32::consts::FRAC_PI_2,
                std::f32::consts::PI,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + 0.0, rect.y + 0.0]);
        }
        if bl > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + bl, rect.y + rect.h - bl],
                bl,
                std::f32::consts::PI,
                std::f32::consts::FRAC_PI_2 * 3.0,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + 0.0, rect.y + rect.h]);
        }
        if br > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + rect.w - br, rect.y + rect.h - br],
                br,
                std::f32::consts::FRAC_PI_2 * 3.0,
                std::f32::consts::TAU,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + rect.w, rect.y + rect.h]);
        }
        if tr > 0.0 {
            arc_append(
                &mut ring,
                [rect.x + rect.w - tr, rect.y + tr],
                tr,
                0.0,
                std::f32::consts::FRAC_PI_2,
                segs,
                true,
            );
        } else {
            ring.push([rect.x + rect.w, rect.y + 0.0]);
        }
        let center = [rect.x + rect.w * 0.5, rect.y + rect.h * 0.5];
        let col = [color.r, color.g, color.b, color.a];
        let base = vertices.len() as u16;
        vertices.push(Vtx {
            pos: center,
            color: col,
        });
        for p in ring.iter() {
            vertices.push(Vtx {
                pos: *p,
                color: col,
            });
        }
        let ring_len = (vertices.len() as u16) - base - 1;
        for i in 0..ring_len {
            let i0 = base;
            let i1 = base + 1 + i;
            let i2 = base + 1 + ((i + 1) % ring_len);
            indices.extend_from_slice(&[i0, i1, i2]);
        }

        // Create GPU buffers
        let vsize = (vertices.len() * std::mem::size_of::<Vtx>()) as u64;
        let isize = (indices.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rounded-rect-fill-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rounded-rect-fill-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&vertices));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&indices));
        }
        let gpu = crate::upload::GpuScene {
            vertex: crate::allocator::OwnedBuffer {
                buffer: vbuf,
                key: crate::allocator::BufKey {
                    size: vsize.max(4),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                },
            },
            index: crate::allocator::OwnedBuffer {
                buffer: ibuf,
                key: crate::allocator::BufKey {
                    size: isize.max(4),
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                },
            },
            vertices: vertices.len() as u32,
            indices: indices.len() as u32,
        };

        // Bind viewport
        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vp-bg-direct-no-msaa"),
            layout: self.solid_direct_no_msaa.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Render directly to target without MSAA to preserve existing content through blending
        // MSAA+resolve doesn't apply blend state correctly for layered rendering
        let _z_bg = self.create_z_bind_group(0.0, queue);

        // Add depth attachment (using 1x since this is non-MSAA rendering)
        let depth_attachment = self.depth_texture.as_ref().map(|tex| {
            wgpu::RenderPassDepthStencilAttachment {
                view: &tex.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load, // Preserve existing depth
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rounded-rect-fill-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: depth_attachment,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.solid_direct_no_msaa.record(&mut pass, &vp_bg, &gpu);
    }

    pub fn render_solids_to_offscreen(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        vp_bg: &wgpu::BindGroup,
        targets: &PassTargets,
        scene: &GpuScene,
        clear_color: wgpu::Color,
        queue: &wgpu::Queue,
    ) {
        // Depth attachment for offscreen rendering (1x)
        let depth_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("solid-depth-offscreen"),
            size: wgpu::Extent3d {
                width: targets.color.key.width,
                height: targets.color.key.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let _z_bg = self.create_z_bind_group(0.0, queue);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("solid-offscreen-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.color.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.solid_offscreen.record(&mut pass, vp_bg, scene);
    }

    pub fn composite_to_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        offscreen: &PassTargets,
        clear: Option<wgpu::Color>,
    ) {
        let bg = self
            .compositor
            .bind_group(&self.device, &offscreen.color.view);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: match clear {
                        Some(c) => wgpu::LoadOp::Clear(c),
                        None => wgpu::LoadOp::Load,
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.compositor.record(&mut pass, &bg);
    }

    /// Paint background to intermediate texture instead of directly to surface.
    /// This enables smooth resizing when combined with blit_to_surface.
    pub fn paint_root_to_intermediate(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bg: &Background,
        queue: &wgpu::Queue,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before painting");
        self.paint_root(encoder, &intermediate.view, bg, queue);
    }

    pub fn paint_root(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        bg: &Background,
        queue: &wgpu::Queue,
    ) {
        // If solid, do a minimal clear pass
        if let Background::Solid(c) = bg {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bg-solid-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: c.r as f64,
                            g: c.g as f64,
                            b: c.b as f64,
                            a: c.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            return;
        }

        // For gradient, draw fullscreen triangle
        let (start_uv, end_uv, stop0, stop1) = match bg {
            Background::LinearGradient {
                start_uv,
                end_uv,
                stop0,
                stop1,
            } => (*start_uv, *end_uv, *stop0, *stop1),
            _ => unreachable!(),
        };
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgParams {
            start: [f32; 2],
            end: [f32; 2],
            center: [f32; 2],
            radius: f32,
            stop_count: u32,
            mode: u32,
            _pad: u32,
        }
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }

        let params = BgParams {
            start: start_uv,
            end: end_uv,
            center: [0.5, 0.5],
            radius: 1.0,
            stop_count: 2,
            mode: 1,
            _pad: 0,
        };
        let c0 = stop0.1;
        let c1 = stop1.1;
        let stops = [
            Stop {
                pos: stop0.0,
                _pad0: [0.0; 3],
                color: [c0.r, c0.g, c0.b, c0.a],
            },
            Stop {
                pos: stop1.0,
                _pad0: [0.0; 3],
                color: [c1.r, c1.g, c1.b, c1.a],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
        ];

        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind"),
            layout: self.bg.bgl(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.bg_param_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bg_stops_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bg-grad-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.bg.record(&mut pass, &bg_bind);
    }

    /// Paint linear gradient to intermediate texture.
    pub fn paint_root_linear_gradient_multi_to_intermediate(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        start_uv: [f32; 2],
        end_uv: [f32; 2],
        stops_in: &[(f32, crate::scene::ColorLinPremul)],
        queue: &wgpu::Queue,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before painting");
        self.paint_root_linear_gradient_multi(
            encoder,
            &intermediate.view,
            start_uv,
            end_uv,
            stops_in,
            queue,
        );
    }

    /// Multi-stop linear gradient background
    pub fn paint_root_linear_gradient_multi(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        start_uv: [f32; 2],
        end_uv: [f32; 2],
        stops_in: &[(f32, crate::scene::ColorLinPremul)],
        queue: &wgpu::Queue,
    ) {
        // Normalize and sort stops for deterministic evaluation
        let mut sorted: Vec<(f32, crate::scene::ColorLinPremul)> = stops_in
            .iter()
            .map(|(p, c)| (p.clamp(0.0, 1.0), *c))
            .collect();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let count = sorted.len().min(8).max(2) as u32;
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgParams {
            start_end: [f32; 4],
            center_radius_stop: [f32; 4],
            flags: [f32; 4],
        }
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }
        let mut stops: [Stop; 8] = [Stop {
            pos: 0.0,
            _pad0: [0.0; 3],
            color: [0.0; 4],
        }; 8];
        for (i, (p, c)) in sorted.iter().take(8).enumerate() {
            stops[i] = Stop {
                pos: *p,
                _pad0: [0.0; 3],
                color: [c.r, c.g, c.b, c.a],
            };
        }
        let debug_flag = std::env::var("DEBUG_RADIAL")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let params = BgParams {
            start_end: [start_uv[0], start_uv[1], end_uv[0], end_uv[1]],
            center_radius_stop: [0.5, 0.5, 1.0, count as f32],
            flags: [1.0, if debug_flag { 1.0 } else { 0.0 }, 0.0, 0.0],
        };
        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind-linear"),
            layout: self.bg.bgl(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.bg_param_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bg_stops_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bg-linear-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.bg.record(&mut pass, &bg_bind);
    }

    /// Paint radial gradient to intermediate texture.
    pub fn paint_root_radial_gradient_multi_to_intermediate(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        center_uv: [f32; 2],
        radius: f32,
        stops_in: &[(f32, crate::scene::ColorLinPremul)],
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before painting");
        self.paint_root_radial_gradient_multi(
            encoder,
            &intermediate.view,
            center_uv,
            radius,
            stops_in,
            queue,
            width,
            height,
        );
    }

    /// Multi-stop radial gradient background
    pub fn paint_root_radial_gradient_multi(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        center_uv: [f32; 2],
        radius: f32,
        stops_in: &[(f32, crate::scene::ColorLinPremul)],
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) {
        // Normalize and sort stops for deterministic evaluation
        let mut sorted: Vec<(f32, crate::scene::ColorLinPremul)> = stops_in
            .iter()
            .map(|(p, c)| (p.clamp(0.0, 1.0), *c))
            .collect();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let count = sorted.len().min(8).max(2) as u32;
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgParams {
            start_end: [f32; 4],
            center_radius_stop: [f32; 4],
            flags: [f32; 4],
        }
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }
        let mut stops: [Stop; 8] = [Stop {
            pos: 0.0,
            _pad0: [0.0; 3],
            color: [0.0; 4],
        }; 8];
        for (i, (p, c)) in sorted.iter().take(8).enumerate() {
            stops[i] = Stop {
                pos: *p,
                _pad0: [0.0; 3],
                color: [c.r, c.g, c.b, c.a],
            };
        }
        let debug_flag = std::env::var("DEBUG_RADIAL")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let aspect_ratio = (width.max(1) as f32) / (height.max(1) as f32);
        if debug_flag {
            // debug logging removed
        }
        // macOS-specific DPI correction: Only adjust for centered fullscreen radials.
        // When center ~ [0.5,0.5], divide center and radius by scale factor to correct
        // for retina scaling differences in UV sampling. No-op elsewhere.
        let mut adj_center = center_uv;
        let mut adj_radius = radius;
        #[cfg(target_os = "macos")]
        {
            let sf = self.scale_factor.max(1.0);
            // Within ~1e-3 of exact center counts as centered
            if (adj_center[0] - 0.5).abs() < 1e-3 && (adj_center[1] - 0.5).abs() < 1e-3 {
                adj_center = [adj_center[0] / sf, adj_center[1] / sf];
                adj_radius = adj_radius / sf;
                if debug_flag {
                    // debug logging removed
                }
            }
        }
        let params = BgParams {
            start_end: [0.0, 0.0, 1.0, 1.0],
            center_radius_stop: [adj_center[0], adj_center[1], adj_radius, count as f32],
            flags: [2.0, if debug_flag { 1.0 } else { 0.0 }, aspect_ratio, 0.0],
        };
        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind-radial"),
            layout: self.bg.bgl(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.bg_param_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bg_stops_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bg-radial-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.bg.record(&mut pass, &bg_bind);
    }

    /// Convenience: paint a solid background color directly to the surface.
    pub fn paint_root_color(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        color: crate::scene::ColorLinPremul,
        queue: &wgpu::Queue,
    ) {
        // Draw solid via the background fullscreen shader to avoid sRGB clear vs blit inconsistencies.
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgParams {
            start_end: [f32; 4],
            center_radius_stop: [f32; 4],
            flags: [f32; 4],
        }
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }
        let params = BgParams {
            start_end: [0.0, 0.0, 1.0, 1.0],
            center_radius_stop: [0.5, 0.5, 1.0, 1.0],
            flags: [0.0, 0.0, 0.0, 0.0], // mode = 0 => solid
        };
        let stops: [Stop; 1] = [Stop {
            pos: 0.0,
            _pad0: [0.0; 3],
            color: [color.r, color.g, color.b, color.a],
        }];
        // Write uniforms (only first stop used for solid mode)
        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind-solid"),
            layout: self.bg.bgl(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.bg_param_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bg_stops_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bg-solid-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.bg.record(&mut pass, &bg_bind);
    }

    /// Convenience: paint a simple 2-stop linear gradient to the surface.
    pub fn paint_root_gradient(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        start_uv: [f32; 2],
        end_uv: [f32; 2],
        stop0: (f32, crate::scene::ColorLinPremul),
        stop1: (f32, crate::scene::ColorLinPremul),
        queue: &wgpu::Queue,
    ) {
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgData {
            start_end: [f32; 4],
            center_radius_stop: [f32; 4],
            flags: [f32; 4],
        }
        let c0 = stop0.1;
        let c1 = stop1.1;
        // Reuse the multi-stop layout by writing two stops into the stops buffer
        let debug_flag = std::env::var("DEBUG_RADIAL")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let params = BgData {
            start_end: [start_uv[0], start_uv[1], end_uv[0], end_uv[1]],
            center_radius_stop: [0.5, 0.5, 1.0, 2.0],
            flags: [1.0, if debug_flag { 1.0 } else { 0.0 }, 0.0, 0.0],
        };
        // Populate first two stops in the stop buffer for the simple gradient helper
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }
        let stops: [Stop; 2] = [
            Stop {
                pos: stop0.0,
                _pad0: [0.0; 3],
                color: [c0.r, c0.g, c0.b, c0.a],
            },
            Stop {
                pos: stop1.0,
                _pad0: [0.0; 3],
                color: [c1.r, c1.g, c1.b, c1.a],
            },
        ];
        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind"),
            layout: self.bg.bgl(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.bg_param_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bg_stops_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bg-grad-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.bg.record(&mut pass, &bg_bind);
    }

    /// Unified rendering: Render all draw types (solids, text, images, SVGs) in a single pass
    /// with proper z-ordering. This is Phase 3 of the depth buffer implementation.
    ///
    /// This method interleaves all draw calls based on z-index for optimal z-ordering performance.
    /// Draw calls are batched by material type when possible for efficiency.
    pub fn render_unified(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        allocator: &mut RenderAllocator,
        surface_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        scene: &GpuScene,
        transparent_scene: &GpuScene,
        transparent_batches: &[crate::upload::TransparentBatch],
        glyph_draws: &[(
            [f32; 2],
            crate::text::RasterizedGlyph,
            crate::ColorLinPremul,
            i32,
        )], // (origin, glyph, color, z)
        svg_draws: &[(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            Option<crate::SvgStyle>,
            i32,
            crate::Transform2D,
            Option<crate::Rect>,
        )],
        image_draws: &[(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            i32,
            Option<crate::Rect>,
        )],
        external_texture_draws: &[crate::upload::ExtractedExternalTextureDraw],
        clear: wgpu::Color,
        direct: bool,
        queue: &wgpu::Queue,
        preserve_surface: bool,
    ) {
        // Update viewport uniform. `logical` is the combined DPI/UI scale factor
        // applied to all scene coordinates when mapping to device pixels.
        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        let inv_logical = if logical.is_finite() && logical > 0.0 {
            1.0 / logical
        } else {
            1.0
        };
        let scale = [
            (2.0f32 / (width.max(1) as f32)) * logical,
            (-2.0f32 / (height.max(1) as f32)) * logical,
        ];
        let translate = [-1.0f32, 1.0f32];
        let vp_data: [f32; 8] = [
            scale[0],
            scale[1],
            translate[0],
            translate[1],
            self.scroll_offset[0],
            self.scroll_offset[1],
            0.0, // padding
            0.0, // padding
        ];
        let data = bytemuck::bytes_of(&vp_data);
        queue.write_buffer(&self.vp_buffer, 0, data);
        let transparent_text_z: std::collections::HashSet<i32> =
            transparent_batches.iter().map(|b| b.z).collect();

        // Ensure depth buffer matches current render size (1x sample)
        self.ensure_depth_texture(allocator, width.max(1), height.max(1));

        // Create viewport bind groups
        let vp_bg_off = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vp-bg-offscreen"),
            layout: self.solid_offscreen.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });
        if direct {
            let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("vp-bg-direct-local"),
                layout: self.solid_direct.viewport_bgl(),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.vp_buffer.as_entire_binding(),
                }],
            });

            // Create z-index bind group before render pass (must outlive the pass)
            let _z_bg = self.create_z_bind_group(0.0, queue);

            // Pre-fetch (and lazily load) all image views before render pass (to avoid mutable borrow conflicts)
            let mut image_views: Vec<(
                wgpu::TextureView,
                [f32; 2],
                [f32; 2],
                f32,
                Option<crate::Rect>,
            )> = Vec::new();
            for (path, origin, size, z, clip) in image_draws.iter() {
                let tex_opt =
                    if let Some(view) = self.try_get_image_view(std::path::Path::new(path)) {
                        Some(view)
                    } else {
                        self.load_image_to_view(std::path::Path::new(path), queue)
                    };
                if let Some((tex_view, _w, _h)) = tex_opt {
                    image_views.push((tex_view, *origin, *size, *z as f32, *clip));
                }
            }

            // Pre-rasterize all SVGs before render pass (to avoid mutable borrow conflicts)
            let mut svg_views: Vec<(
                wgpu::TextureView,
                [f32; 2],
                [f32; 2],
                f32,
                Option<crate::Rect>,
            )> = Vec::new();
            for (path, origin, max_size, style, _z, transform, clip) in svg_draws.iter() {
                if let Some((_view, w, h)) =
                    self.rasterize_svg_to_view(std::path::Path::new(path), 1.0, *style, queue)
                {
                    let base_w = w.max(1) as f32;
                    let base_h = h.max(1) as f32;
                    let scale = (max_size[0] / base_w).min(max_size[1] / base_h).max(0.0);

                    if let Some((view_scaled, _sw, _sh)) =
                        self.rasterize_svg_to_view(std::path::Path::new(path), scale, *style, queue)
                    {
                        let draw_w = base_w * scale;
                        let draw_h = base_h * scale;
                        let transformed_origin = apply_transform_to_point(*origin, *transform);
                        svg_views.push((
                            view_scaled,
                            transformed_origin,
                            [draw_w, draw_h],
                            *_z as f32,
                            *clip,
                        ));
                    }
                }
            }

            // Group text by z-index for proper depth rendering
            // eprintln!("🎨 render_unified received {} glyph_draws", glyph_draws.len());
            let mut text_by_z: std::collections::HashMap<
                i32,
                Vec<(
                    usize,
                    [f32; 2],
                    &crate::text::RasterizedGlyph,
                    &crate::ColorLinPremul,
                )>,
            > = std::collections::HashMap::new();
            for (idx, (origin, glyph, color, z)) in glyph_draws.iter().enumerate() {
                text_by_z
                    .entry(*z)
                    .or_insert_with(Vec::new)
                    .push((idx, *origin, glyph, color));
            }
            // eprintln!("🎨 Grouped text into {} z-index groups", text_by_z.len());

            // Prepare text rendering data before render pass
            let mut text_groups = if !glyph_draws.is_empty() {
                // Clear the atlas region used in the previous frame (efficient partial clear)
                if self.prev_atlas_max_x > 0 && self.prev_atlas_max_y > 0 {
                    let clear_width = self.prev_atlas_max_x.min(4096);
                    let clear_height = self.prev_atlas_max_y.min(4096);
                    let clear_size = (clear_width * clear_height * 4) as usize;
                    let clear_data = vec![0u8; clear_size];
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &self.text_mask_atlas,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &clear_data,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(clear_width * 4),
                            rows_per_image: Some(clear_height),
                        },
                        wgpu::Extent3d {
                            width: clear_width,
                            height: clear_height,
                            depth_or_array_layers: 1,
                        },
                    );
                }

                let mut atlas_cursor_x = 0u32;
                let mut atlas_cursor_y = 0u32;
                let mut next_row_height = 0u32;
                let mut atlas_max_x = 0u32;
                let mut atlas_max_y = 0u32;
                let mut all_text_groups: Vec<(i32, Vec<TextQuadVtx>)> = Vec::new();

                // Process each z-index group
                for (z_index, glyphs) in text_by_z.iter() {
                    let mut vertices: Vec<TextQuadVtx> = Vec::new();
                    let force_grayscale = transparent_text_z.contains(z_index);
                    // eprintln!("      🔠 Processing z={} with {} glyphs", z_index, glyphs.len());

                    let mut local_idx = 0;
                    for (_idx, origin, glyph, color) in glyphs.iter() {
                        let (w, h, data) = glyph_mask_for_atlas(&glyph.mask, force_grayscale);
                        if local_idx == 0 {
                            // eprintln!("        🔤 First glyph: origin=[{:.1}, {:.1}], size=[{}, {}], color=[{:.3}, {:.3}, {:.3}, {:.3}]",
                            //     origin[0], origin[1], w, h, color.r, color.g, color.b, color.a);
                        }
                        local_idx += 1;

                        if atlas_cursor_x + w >= 4096 {
                            atlas_cursor_x = 0;
                            atlas_cursor_y += next_row_height;
                            next_row_height = 0;
                        }
                        next_row_height = next_row_height.max(h);

                        // Track maximum atlas region used for clearing next frame
                        atlas_max_x = atlas_max_x.max(atlas_cursor_x + w);
                        atlas_max_y = atlas_max_y.max(atlas_cursor_y + h);

                        queue.write_texture(
                            wgpu::ImageCopyTexture {
                                texture: &self.text_mask_atlas,
                                mip_level: 0,
                                origin: wgpu::Origin3d {
                                    x: atlas_cursor_x,
                                    y: atlas_cursor_y,
                                    z: 0,
                                },
                                aspect: wgpu::TextureAspect::All,
                            },
                            data.as_ref(),
                            wgpu::ImageDataLayout {
                                offset: 0,
                                bytes_per_row: Some(w * 4),
                                rows_per_image: Some(h),
                            },
                            wgpu::Extent3d {
                                width: w,
                                height: h,
                                depth_or_array_layers: 1,
                            },
                        );

                        let u0 = atlas_cursor_x as f32 / 4096.0;
                        let v0 = atlas_cursor_y as f32 / 4096.0;
                        let u1 = (atlas_cursor_x + w) as f32 / 4096.0;
                        let v1 = (atlas_cursor_y + h) as f32 / 4096.0;

                        // Glyph masks are rasterized at *physical* pixel size. To avoid
                        // scaling them again during composition, convert their size into
                        // logical scene units so that the subsequent logical->device
                        // scaling in the viewport transform maps them 1:1 to the atlas.
                        let quad_w = (w as f32) * inv_logical;
                        let quad_h = (h as f32) * inv_logical;

                        if local_idx == 1 {
                            // eprintln!("        📐 Atlas pos: cursor=({}, {}), uv=[{:.4}, {:.4}] to [{:.4}, {:.4}]",
                            //     atlas_cursor_x, atlas_cursor_y, u0, v0, u1, v1);
                        }

                        vertices.extend_from_slice(&[
                            TextQuadVtx {
                                pos: [origin[0], origin[1]],
                                uv: [u0, v0],
                                color: [color.r, color.g, color.b, color.a],
                            },
                            TextQuadVtx {
                                pos: [origin[0] + quad_w, origin[1]],
                                uv: [u1, v0],
                                color: [color.r, color.g, color.b, color.a],
                            },
                            TextQuadVtx {
                                pos: [origin[0] + quad_w, origin[1] + quad_h],
                                uv: [u1, v1],
                                color: [color.r, color.g, color.b, color.a],
                            },
                            TextQuadVtx {
                                pos: [origin[0], origin[1] + quad_h],
                                uv: [u0, v1],
                                color: [color.r, color.g, color.b, color.a],
                            },
                        ]);

                        atlas_cursor_x += w;
                    }

                    // Store vertices for this z-index group
                    if !vertices.is_empty() {
                        all_text_groups.push((*z_index, vertices));
                    }
                }

                // Create buffers and bind groups for each text group
                // eprintln!("🔧 all_text_groups.len() = {}", all_text_groups.len());
                let mut text_resources: Vec<(
                    i32,
                    wgpu::Buffer,
                    wgpu::Buffer,
                    u32,
                    wgpu::BindGroup,
                    wgpu::Buffer,
                )> = Vec::new();
                for (z_index, vertices) in all_text_groups {
                    // eprintln!(
                    //     "  🛠️  Creating resources for z={}, vertices={}",
                    //     z_index,
                    //     vertices.len()
                    // );
                    let quad_count = vertices.len() / 4;
                    let mut indices: Vec<u16> = Vec::with_capacity(quad_count * 6);
                    for i in 0..quad_count {
                        let base = (i * 4) as u16;
                        indices.extend_from_slice(&[
                            base,
                            base + 1,
                            base + 2,
                            base,
                            base + 2,
                            base + 3,
                        ]);
                    }

                    // Create vertex buffer for this group
                    let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("text-vertex-buffer-group"),
                        size: (vertices.len() * std::mem::size_of::<TextQuadVtx>()) as u64,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });

                    // Create index buffer for this group
                    let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("text-index-buffer-group"),
                        size: (indices.len() * std::mem::size_of::<u16>()) as u64,
                        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });

                    queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&vertices));
                    queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&indices));

                    // Create z bind group for this text group
                    // Pass z_index as float directly - shader will convert to depth
                    // eprintln!("    💎 z={} (passing as z-index to shader)", z_index);
                    let (z_bg, z_buf) = self.create_group_z_bind_group(z_index as f32, queue);

                    text_resources.push((z_index, vbuf, ibuf, indices.len() as u32, z_bg, z_buf));
                }

                // Store atlas usage for next frame's clearing
                self.prev_atlas_max_x = atlas_max_x;
                self.prev_atlas_max_y = atlas_max_y;

                text_resources
            } else {
                Vec::new()
            };

            // Sort text groups by z-index (back to front)
            text_groups.sort_by_key(|(z, _, _, _, _, _)| *z);

            // Create text bind groups before render pass so they live long enough
            let vp_bg_text = self.text.vp_bind_group(&self.device, &self.vp_buffer);

            // Prepare image resources (collect all buffers and bind groups so they live long enough)
            let mut image_resources: Vec<(
                wgpu::Buffer,
                wgpu::Buffer,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::Buffer,
                wgpu::Buffer,
                Option<crate::Rect>,
            )> = Vec::new();
            let mut image_z_vals: Vec<i32> = Vec::new();
            for (tex_view, origin, size, z_val, clip) in image_views.iter() {
                let verts = [
                    ImageQuadVtx {
                        pos: [origin[0], origin[1]],
                        uv: [0.0, 0.0],
                    },
                    ImageQuadVtx {
                        pos: [origin[0] + size[0], origin[1]],
                        uv: [1.0, 0.0],
                    },
                    ImageQuadVtx {
                        pos: [origin[0] + size[0], origin[1] + size[1]],
                        uv: [1.0, 1.0],
                    },
                    ImageQuadVtx {
                        pos: [origin[0], origin[1] + size[1]],
                        uv: [0.0, 1.0],
                    },
                ];
                let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

                let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("image-vbuf-unified"),
                    size: (verts.len() * std::mem::size_of::<ImageQuadVtx>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("image-ibuf-unified"),
                    size: (idx.len() * std::mem::size_of::<u16>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
                queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));

                let vp_bg_img = self.image.vp_bind_group(&self.device, &self.vp_buffer);
                // Pass z_index as float directly - shader will convert to depth
                let (z_bg_img, z_buf_img) = self.create_group_z_bind_group(*z_val as f32, queue);
                let tex_bg = self.image.tex_bind_group(&self.device, tex_view);
                let (params_bg, params_buf) =
                    self.image.params_bind_group(&self.device, 1.0, false);

                image_z_vals.push(*z_val as i32);
                image_resources.push((
                    vbuf, ibuf, vp_bg_img, z_bg_img, tex_bg, params_bg, z_buf_img, params_buf,
                    *clip,
                ));
            }

            // Prepare SVG resources
            let mut svg_z_vals: Vec<i32> = Vec::new();
            let mut svg_resources: Vec<(
                wgpu::Buffer,
                wgpu::Buffer,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::Buffer,
                wgpu::Buffer,
                Option<crate::Rect>,
            )> = Vec::new();
            for (view_scaled, origin, size, z_val, clip) in svg_views.iter() {
                let verts = [
                    ImageQuadVtx {
                        pos: [origin[0], origin[1]],
                        uv: [0.0, 0.0],
                    },
                    ImageQuadVtx {
                        pos: [origin[0] + size[0], origin[1]],
                        uv: [1.0, 0.0],
                    },
                    ImageQuadVtx {
                        pos: [origin[0] + size[0], origin[1] + size[1]],
                        uv: [1.0, 1.0],
                    },
                    ImageQuadVtx {
                        pos: [origin[0], origin[1] + size[1]],
                        uv: [0.0, 1.0],
                    },
                ];
                let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

                let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("svg-vbuf-unified"),
                    size: (verts.len() * std::mem::size_of::<ImageQuadVtx>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("svg-ibuf-unified"),
                    size: (idx.len() * std::mem::size_of::<u16>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
                queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));

                let vp_bg_svg = self.image.vp_bind_group(&self.device, &self.vp_buffer);
                // Pass z_index as float directly - shader will convert to depth
                let (z_bg_svg, z_buf_svg) = self.create_group_z_bind_group(*z_val as f32, queue);
                let tex_bg = self.image.tex_bind_group(&self.device, view_scaled);
                let (params_bg, params_buf) =
                    self.image.params_bind_group(&self.device, 1.0, false);

                svg_z_vals.push(*z_val as i32);
                svg_resources.push((
                    vbuf, ibuf, vp_bg_svg, z_bg_svg, tex_bg, params_bg, z_buf_svg, params_buf,
                    *clip,
                ));
            }

            // Prepare external texture resources (e.g., 3D viewports)
            let mut ext_z_vals: Vec<i32> = Vec::new();
            let mut ext_resources: Vec<(
                wgpu::Buffer,
                wgpu::Buffer,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::BindGroup,
                wgpu::Buffer,
                wgpu::Buffer,
            )> = Vec::new();
            for etd in external_texture_draws.iter() {
                let Some(tex_view) = self.external_textures.get(&etd.texture_id) else {
                    continue;
                };
                let verts = [
                    ImageQuadVtx {
                        pos: [etd.origin[0], etd.origin[1]],
                        uv: [0.0, 0.0],
                    },
                    ImageQuadVtx {
                        pos: [etd.origin[0] + etd.size[0], etd.origin[1]],
                        uv: [1.0, 0.0],
                    },
                    ImageQuadVtx {
                        pos: [etd.origin[0] + etd.size[0], etd.origin[1] + etd.size[1]],
                        uv: [1.0, 1.0],
                    },
                    ImageQuadVtx {
                        pos: [etd.origin[0], etd.origin[1] + etd.size[1]],
                        uv: [0.0, 1.0],
                    },
                ];
                let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

                let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ext-tex-vbuf-unified"),
                    size: (verts.len() * std::mem::size_of::<ImageQuadVtx>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("ext-tex-ibuf-unified"),
                    size: (idx.len() * std::mem::size_of::<u16>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
                queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));

                let vp_bg_ext = self.image.vp_bind_group(&self.device, &self.vp_buffer);
                let (z_bg_ext, z_buf_ext) = self.create_group_z_bind_group(etd.z as f32, queue);
                let tex_bg = self.image.tex_bind_group(&self.device, tex_view);
                let (params_bg, params_buf) =
                    self.image
                        .params_bind_group(&self.device, etd.opacity, etd.premultiplied);

                ext_z_vals.push(etd.z);
                ext_resources.push((
                    vbuf, ibuf, vp_bg_ext, z_bg_ext, tex_bg, params_bg, z_buf_ext, params_buf,
                ));
            }

            // Build depth attachment after all mutable borrows on self are finished
            let depth_attachment = Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: if preserve_surface {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(1.0)
                    },
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            });

            // Begin unified render pass (after all resource preparation)
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("unified-render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: if preserve_surface {
                            wgpu::LoadOp::Load
                        } else {
                            wgpu::LoadOp::Clear(clear)
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth_attachment,
                occlusion_query_set: None,
                timestamp_writes: None,
            });

            // Render solids first (they're already sorted by z-index in the scene)
            self.solid_direct.record(&mut pass, &vp_bg, scene);

            // Unified z-sorted rendering: interleave ALL draw types (transparent
            // solids, text, images, SVGs, external textures) by z-index so that
            // depth ordering is correct across element types. Without this,
            // images/SVGs rendered in a flat batch after transparent solids could
            // appear on top of higher-z transparent overlays (like a dock scrim).
            #[derive(Clone, Copy)]
            enum DrawItem {
                TransparentBatch(usize),
                TextGroup(usize),
                Image(usize),
                Svg(usize),
                ExternalTexture(usize),
            }
            let mut all_items: Vec<(i32, DrawItem)> = Vec::new();
            for (i, batch) in transparent_batches.iter().enumerate() {
                all_items.push((batch.z, DrawItem::TransparentBatch(i)));
            }
            for (i, (z, _, _, _, _, _)) in text_groups.iter().enumerate() {
                all_items.push((*z, DrawItem::TextGroup(i)));
            }
            for (i, z) in image_z_vals.iter().enumerate() {
                all_items.push((*z, DrawItem::Image(i)));
            }
            for (i, z) in svg_z_vals.iter().enumerate() {
                all_items.push((*z, DrawItem::Svg(i)));
            }
            for (i, z) in ext_z_vals.iter().enumerate() {
                all_items.push((*z, DrawItem::ExternalTexture(i)));
            }
            // Stable sort preserves relative order within same z-index
            all_items.sort_by_key(|(z, _)| *z);

            for &(_, item) in all_items.iter() {
                match item {
                    DrawItem::TransparentBatch(i) => {
                        let batch = &transparent_batches[i];
                        self.transparent_solid_direct.record_index_range(
                            &mut pass,
                            &vp_bg,
                            transparent_scene,
                            batch.index_start,
                            batch.index_count,
                        );
                    }
                    DrawItem::TextGroup(i) => {
                        let (_z, vbuf, ibuf, index_count, z_bg, _z_buf) = &text_groups[i];
                        if *index_count > 0 {
                            pass.set_pipeline(&self.text.pipeline);
                            pass.set_bind_group(0, &vp_bg_text, &[]);
                            pass.set_bind_group(1, z_bg, &[]);
                            pass.set_bind_group(2, &self.text_bind_group, &[]);
                            pass.set_vertex_buffer(0, vbuf.slice(..));
                            pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                            pass.draw_indexed(0..*index_count, 0, 0..1);
                        }
                    }
                    DrawItem::Image(i) => {
                        let (vbuf, ibuf, vp_bg_img, z_bg_img, tex_bg, params_bg, _, _, clip) =
                            &image_resources[i];
                        if let Some(c) = clip {
                            pass.set_scissor_rect(
                                c.x.max(0.0) as u32,
                                c.y.max(0.0) as u32,
                                (c.w.max(1.0) as u32)
                                    .min(width.saturating_sub(c.x.max(0.0) as u32)),
                                (c.h.max(1.0) as u32)
                                    .min(height.saturating_sub(c.y.max(0.0) as u32)),
                            );
                        }
                        self.image.record(
                            &mut pass, vp_bg_img, z_bg_img, tex_bg, params_bg, vbuf, ibuf, 6,
                        );
                        if clip.is_some() {
                            pass.set_scissor_rect(0, 0, width, height);
                        }
                    }
                    DrawItem::Svg(i) => {
                        let (vbuf, ibuf, vp_bg_svg, z_bg_svg, tex_bg, params_bg, _, _, clip) =
                            &svg_resources[i];
                        if let Some(c) = clip {
                            pass.set_scissor_rect(
                                c.x.max(0.0) as u32,
                                c.y.max(0.0) as u32,
                                (c.w.max(1.0) as u32)
                                    .min(width.saturating_sub(c.x.max(0.0) as u32)),
                                (c.h.max(1.0) as u32)
                                    .min(height.saturating_sub(c.y.max(0.0) as u32)),
                            );
                        }
                        self.image.record(
                            &mut pass, vp_bg_svg, z_bg_svg, tex_bg, params_bg, vbuf, ibuf, 6,
                        );
                        if clip.is_some() {
                            pass.set_scissor_rect(0, 0, width, height);
                        }
                    }
                    DrawItem::ExternalTexture(i) => {
                        let (vbuf, ibuf, vp_bg_ext, z_bg_ext, tex_bg, params_bg, _, _) =
                            &ext_resources[i];
                        self.image.record(
                            &mut pass, vp_bg_ext, z_bg_ext, tex_bg, params_bg, vbuf, ibuf, 6,
                        );
                    }
                }
            }

            // NOW drop the pass - all rendering complete
            drop(pass);
            // eprintln!("✨ Render pass completed successfully (DIRECT)");
            return;
        }

        // Offscreen path - unified rendering to offscreen target
        let targets = self.alloc_targets(allocator, width.max(1), height.max(1));

        // Pre-fetch (and lazily load) all image views before render pass (to avoid mutable borrow conflicts)
        let mut image_views_off: Vec<(
            wgpu::TextureView,
            [f32; 2],
            [f32; 2],
            f32,
            Option<crate::Rect>,
        )> = Vec::new();
        // eprintln!("🔍 Pre-fetching {} images for unified offscreen render", image_draws.len());
        for (path, origin, size, z, clip) in image_draws.iter() {
            // eprintln!("  📦 Image at z={}: {:?}", z, path.file_name().unwrap_or_default());
            let tex_opt = if let Some(view) = self.try_get_image_view(std::path::Path::new(path)) {
                Some(view)
            } else {
                self.load_image_to_view(std::path::Path::new(path), queue)
            };
            if let Some((tex_view, _w, _h)) = tex_opt {
                image_views_off.push((tex_view, *origin, *size, *z as f32, *clip));
            }
        }

        // Pre-rasterize all SVGs before creating render pass (to avoid mutable borrow conflicts)
        let mut svg_views_off: Vec<(
            wgpu::TextureView,
            [f32; 2],
            [f32; 2],
            f32,
            Option<crate::Rect>,
        )> = Vec::new();
        for (path, origin, max_size, style, _z, transform, clip) in svg_draws.iter() {
            if let Some((_view, w, h)) =
                self.rasterize_svg_to_view(std::path::Path::new(path), 1.0, *style, queue)
            {
                let base_w = w.max(1) as f32;
                let base_h = h.max(1) as f32;
                let scale = (max_size[0] / base_w).min(max_size[1] / base_h).max(0.0);

                if let Some((view_scaled, _sw, _sh)) =
                    self.rasterize_svg_to_view(std::path::Path::new(path), scale, *style, queue)
                {
                    // Use logical size, not rasterized pixel dimensions (see note above).
                    let draw_w = base_w * scale;
                    let draw_h = base_h * scale;
                    let transformed_origin = apply_transform_to_point(*origin, *transform);
                    svg_views_off.push((
                        view_scaled,
                        transformed_origin,
                        [draw_w, draw_h],
                        *_z as f32,
                        *clip,
                    ));
                }
            }
        }

        // Group text by z-index for proper depth rendering (offscreen path)
        let mut text_by_z_off: std::collections::HashMap<
            i32,
            Vec<(
                usize,
                [f32; 2],
                &crate::text::RasterizedGlyph,
                &crate::ColorLinPremul,
            )>,
        > = std::collections::HashMap::new();
        for (idx, (origin, glyph, color, z)) in glyph_draws.iter().enumerate() {
            text_by_z_off
                .entry(*z)
                .or_insert_with(Vec::new)
                .push((idx, *origin, glyph, color));
        }

        // Prepare text rendering data (same as direct path)
        let mut text_groups_off = if !glyph_draws.is_empty() {
            // Clear the atlas region used in the previous frame (efficient partial clear)
            // Note: Atlas is shared between direct and offscreen paths, so clear here too
            if self.prev_atlas_max_x > 0 && self.prev_atlas_max_y > 0 {
                let clear_width = self.prev_atlas_max_x.min(4096);
                let clear_height = self.prev_atlas_max_y.min(4096);
                let clear_size = (clear_width * clear_height * 4) as usize;
                let clear_data = vec![0u8; clear_size];
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: &self.text_mask_atlas,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &clear_data,
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(clear_width * 4),
                        rows_per_image: Some(clear_height),
                    },
                    wgpu::Extent3d {
                        width: clear_width,
                        height: clear_height,
                        depth_or_array_layers: 1,
                    },
                );
            }

            let mut atlas_cursor_x = 0u32;
            let mut atlas_cursor_y = 0u32;
            let mut next_row_height = 0u32;
            let mut atlas_max_x = 0u32;
            let mut atlas_max_y = 0u32;
            let mut all_text_groups: Vec<(i32, Vec<TextQuadVtx>)> = Vec::new();

            // Process each z-index group
            for (z_index, glyphs) in text_by_z_off.iter() {
                let mut vertices: Vec<TextQuadVtx> = Vec::new();
                let force_grayscale = transparent_text_z.contains(z_index);

                for (_idx, origin, glyph, color) in glyphs.iter() {
                    let (w, h, data) = glyph_mask_for_atlas(&glyph.mask, force_grayscale);

                    if atlas_cursor_x + w >= 4096 {
                        atlas_cursor_x = 0;
                        atlas_cursor_y += next_row_height;
                        next_row_height = 0;
                    }
                    next_row_height = next_row_height.max(h);

                    // Track maximum atlas region used for clearing next frame
                    atlas_max_x = atlas_max_x.max(atlas_cursor_x + w);
                    atlas_max_y = atlas_max_y.max(atlas_cursor_y + h);

                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &self.text_mask_atlas,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: atlas_cursor_x,
                                y: atlas_cursor_y,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        data.as_ref(),
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(w * 4),
                            rows_per_image: Some(h),
                        },
                        wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                    );

                    let u0 = atlas_cursor_x as f32 / 4096.0;
                    let v0 = atlas_cursor_y as f32 / 4096.0;
                    let u1 = (atlas_cursor_x + w) as f32 / 4096.0;
                    let v1 = (atlas_cursor_y + h) as f32 / 4096.0;

                    // Convert glyph bitmap size from physical pixels into logical
                    // scene units so that the viewport scale maps them back to
                    // physical pixels without additional filtering.
                    let quad_w = (w as f32) * inv_logical;
                    let quad_h = (h as f32) * inv_logical;

                    vertices.extend_from_slice(&[
                        TextQuadVtx {
                            pos: [origin[0], origin[1]],
                            uv: [u0, v0],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0] + quad_w, origin[1]],
                            uv: [u1, v0],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0] + quad_w, origin[1] + quad_h],
                            uv: [u1, v1],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0], origin[1] + quad_h],
                            uv: [u0, v1],
                            color: [color.r, color.g, color.b, color.a],
                        },
                    ]);

                    atlas_cursor_x += w;
                }

                // Store vertices for this z-index group
                if !vertices.is_empty() {
                    all_text_groups.push((*z_index, vertices));
                }
            }

            // Create buffers and bind groups for each text group
            let mut text_resources: Vec<(
                i32,
                wgpu::Buffer,
                wgpu::Buffer,
                u32,
                wgpu::BindGroup,
                wgpu::Buffer,
            )> = Vec::new();
            for (z_index, vertices) in all_text_groups {
                let quad_count = vertices.len() / 4;
                let mut indices: Vec<u16> = Vec::with_capacity(quad_count * 6);
                for i in 0..quad_count {
                    let base = (i * 4) as u16;
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }

                // Create vertex buffer for this group
                let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("text-vertex-buffer-group-off"),
                    size: (vertices.len() * std::mem::size_of::<TextQuadVtx>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                // Create index buffer for this group
                let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("text-index-buffer-group-off"),
                    size: (indices.len() * std::mem::size_of::<u16>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&vertices));
                queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&indices));

                // Create z bind group for this text group
                // Pass z_index as float directly - shader will convert to depth
                let (z_bg, z_buf) = self.create_group_z_bind_group(z_index as f32, queue);

                text_resources.push((z_index, vbuf, ibuf, indices.len() as u32, z_bg, z_buf));
            }

            // Store atlas usage for next frame's clearing
            self.prev_atlas_max_x = atlas_max_x;
            self.prev_atlas_max_y = atlas_max_y;

            text_resources
        } else {
            Vec::new()
        };

        // Sort text groups by z-index (back to front)
        text_groups_off.sort_by_key(|(z, _, _, _, _, _)| *z);

        // Create text bind groups (use offscreen text renderer for offscreen rendering)
        let vp_bg_text_off = self
            .text_offscreen
            .vp_bind_group(&self.device, &self.vp_buffer);

        // Prepare image resources (offscreen: use image_offscreen to match format)
        let mut image_z_vals_off: Vec<i32> = Vec::new();
        let mut image_resources_off: Vec<(
            wgpu::Buffer,
            wgpu::Buffer,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::Buffer,
            wgpu::Buffer,
            Option<crate::Rect>,
        )> = Vec::new();
        for (tex_view, origin, size, z_val, clip) in image_views_off.iter() {
            let verts = [
                ImageQuadVtx {
                    pos: [origin[0], origin[1]],
                    uv: [0.0, 0.0],
                },
                ImageQuadVtx {
                    pos: [origin[0] + size[0], origin[1]],
                    uv: [1.0, 0.0],
                },
                ImageQuadVtx {
                    pos: [origin[0] + size[0], origin[1] + size[1]],
                    uv: [1.0, 1.0],
                },
                ImageQuadVtx {
                    pos: [origin[0], origin[1] + size[1]],
                    uv: [0.0, 1.0],
                },
            ];
            let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

            let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("image-vbuf-unified-offscreen"),
                size: (verts.len() * std::mem::size_of::<ImageQuadVtx>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("image-ibuf-unified-offscreen"),
                size: (idx.len() * std::mem::size_of::<u16>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));

            let vp_bg_img = self
                .image_offscreen
                .vp_bind_group(&self.device, &self.vp_buffer);
            // Pass z_index as float directly - shader will convert to depth
            let (z_bg_img, z_buf_img) = self.create_group_z_bind_group(*z_val as f32, queue);
            let tex_bg = self.image_offscreen.tex_bind_group(&self.device, tex_view);
            let (params_bg, params_buf) =
                self.image_offscreen
                    .params_bind_group(&self.device, 1.0, false);

            image_z_vals_off.push(*z_val as i32);
            image_resources_off.push((
                vbuf, ibuf, vp_bg_img, z_bg_img, tex_bg, params_bg, z_buf_img, params_buf, *clip,
            ));
        }

        // Prepare SVG resources (offscreen: use image_offscreen to match format)
        let mut svg_z_vals_off: Vec<i32> = Vec::new();
        let mut svg_resources_off: Vec<(
            wgpu::Buffer,
            wgpu::Buffer,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::Buffer,
            wgpu::Buffer,
            Option<crate::Rect>,
        )> = Vec::new();
        for (view_scaled, origin, size, z_val, clip) in svg_views_off.iter() {
            let verts = [
                ImageQuadVtx {
                    pos: [origin[0], origin[1]],
                    uv: [0.0, 0.0],
                },
                ImageQuadVtx {
                    pos: [origin[0] + size[0], origin[1]],
                    uv: [1.0, 0.0],
                },
                ImageQuadVtx {
                    pos: [origin[0] + size[0], origin[1] + size[1]],
                    uv: [1.0, 1.0],
                },
                ImageQuadVtx {
                    pos: [origin[0], origin[1] + size[1]],
                    uv: [0.0, 1.0],
                },
            ];
            let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

            let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("svg-vbuf-unified-offscreen"),
                size: (verts.len() * std::mem::size_of::<ImageQuadVtx>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("svg-ibuf-unified-offscreen"),
                size: (idx.len() * std::mem::size_of::<u16>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));

            let vp_bg_svg = self
                .image_offscreen
                .vp_bind_group(&self.device, &self.vp_buffer);
            // Pass z_index as float directly - shader will convert to depth
            let (z_bg_svg, z_buf_svg) = self.create_group_z_bind_group(*z_val as f32, queue);
            let tex_bg = self
                .image_offscreen
                .tex_bind_group(&self.device, view_scaled);
            let (params_bg, params_buf) =
                self.image_offscreen
                    .params_bind_group(&self.device, 1.0, false);

            svg_z_vals_off.push(*z_val as i32);
            svg_resources_off.push((
                vbuf, ibuf, vp_bg_svg, z_bg_svg, tex_bg, params_bg, z_buf_svg, params_buf, *clip,
            ));
        }

        // Prepare external texture resources (offscreen: use image_offscreen to match format)
        let mut ext_z_vals_off: Vec<i32> = Vec::new();
        let mut ext_resources_off: Vec<(
            wgpu::Buffer,
            wgpu::Buffer,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::BindGroup,
            wgpu::Buffer,
            wgpu::Buffer,
        )> = Vec::new();
        for etd in external_texture_draws.iter() {
            let Some(tex_view) = self.external_textures.get(&etd.texture_id) else {
                continue;
            };
            let verts = [
                ImageQuadVtx {
                    pos: [etd.origin[0], etd.origin[1]],
                    uv: [0.0, 0.0],
                },
                ImageQuadVtx {
                    pos: [etd.origin[0] + etd.size[0], etd.origin[1]],
                    uv: [1.0, 0.0],
                },
                ImageQuadVtx {
                    pos: [etd.origin[0] + etd.size[0], etd.origin[1] + etd.size[1]],
                    uv: [1.0, 1.0],
                },
                ImageQuadVtx {
                    pos: [etd.origin[0], etd.origin[1] + etd.size[1]],
                    uv: [0.0, 1.0],
                },
            ];
            let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

            let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ext-tex-vbuf-unified-offscreen"),
                size: (verts.len() * std::mem::size_of::<ImageQuadVtx>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ext-tex-ibuf-unified-offscreen"),
                size: (idx.len() * std::mem::size_of::<u16>()) as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));

            let vp_bg_ext = self
                .image_offscreen
                .vp_bind_group(&self.device, &self.vp_buffer);
            let (z_bg_ext, z_buf_ext) = self.create_group_z_bind_group(etd.z as f32, queue);
            let tex_bg = self.image_offscreen.tex_bind_group(&self.device, tex_view);
            let (params_bg, params_buf) = self.image_offscreen.params_bind_group(
                &self.device,
                etd.opacity,
                etd.premultiplied,
            );

            ext_z_vals_off.push(etd.z);
            ext_resources_off.push((
                vbuf, ibuf, vp_bg_ext, z_bg_ext, tex_bg, params_bg, z_buf_ext, params_buf,
            ));
        }

        let depth_attachment = Some(wgpu::RenderPassDepthStencilAttachment {
            view: self.depth_view(),
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        });

        let _z_bg = self.create_z_bind_group(0.0, queue);

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("unified-offscreen-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.color.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: depth_attachment,
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        // Render solids first
        // eprintln!("🟢 OFFSCREEN PATH: Rendering {} solid vertices", scene.vertices);
        self.solid_offscreen.record(&mut pass, &vp_bg_off, scene);

        // Unified z-sorted rendering (offscreen path): interleave ALL draw types
        // by z-index for correct depth ordering across element types.
        {
            #[derive(Clone, Copy)]
            enum DrawItemOff {
                TransparentBatch(usize),
                TextGroup(usize),
                Image(usize),
                Svg(usize),
                ExternalTexture(usize),
            }
            let mut all_items: Vec<(i32, DrawItemOff)> = Vec::new();
            for (i, batch) in transparent_batches.iter().enumerate() {
                all_items.push((batch.z, DrawItemOff::TransparentBatch(i)));
            }
            for (i, (z, _, _, _, _, _)) in text_groups_off.iter().enumerate() {
                all_items.push((*z, DrawItemOff::TextGroup(i)));
            }
            for (i, z) in image_z_vals_off.iter().enumerate() {
                all_items.push((*z, DrawItemOff::Image(i)));
            }
            for (i, z) in svg_z_vals_off.iter().enumerate() {
                all_items.push((*z, DrawItemOff::Svg(i)));
            }
            for (i, z) in ext_z_vals_off.iter().enumerate() {
                all_items.push((*z, DrawItemOff::ExternalTexture(i)));
            }
            all_items.sort_by_key(|(z, _)| *z);

            for &(_, item) in all_items.iter() {
                match item {
                    DrawItemOff::TransparentBatch(i) => {
                        let batch = &transparent_batches[i];
                        self.transparent_solid_offscreen.record_index_range(
                            &mut pass,
                            &vp_bg_off,
                            transparent_scene,
                            batch.index_start,
                            batch.index_count,
                        );
                    }
                    DrawItemOff::TextGroup(i) => {
                        let (_z, vbuf, ibuf, index_count, z_bg, _z_buf) = &text_groups_off[i];
                        if *index_count > 0 {
                            pass.set_pipeline(&self.text_offscreen.pipeline);
                            pass.set_bind_group(0, &vp_bg_text_off, &[]);
                            pass.set_bind_group(1, z_bg, &[]);
                            pass.set_bind_group(2, &self.text_bind_group, &[]);
                            pass.set_vertex_buffer(0, vbuf.slice(..));
                            pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                            pass.draw_indexed(0..*index_count, 0, 0..1);
                        }
                    }
                    DrawItemOff::Image(i) => {
                        let (vbuf, ibuf, vp_bg_img, z_bg_img, tex_bg, params_bg, _, _, clip) =
                            &image_resources_off[i];
                        if let Some(c) = clip {
                            pass.set_scissor_rect(
                                c.x.max(0.0) as u32,
                                c.y.max(0.0) as u32,
                                (c.w.max(1.0) as u32)
                                    .min(width.saturating_sub(c.x.max(0.0) as u32)),
                                (c.h.max(1.0) as u32)
                                    .min(height.saturating_sub(c.y.max(0.0) as u32)),
                            );
                        }
                        self.image_offscreen.record(
                            &mut pass, vp_bg_img, z_bg_img, tex_bg, params_bg, vbuf, ibuf, 6,
                        );
                        if clip.is_some() {
                            pass.set_scissor_rect(0, 0, width, height);
                        }
                    }
                    DrawItemOff::Svg(i) => {
                        let (vbuf, ibuf, vp_bg_svg, z_bg_svg, tex_bg, params_bg, _, _, clip) =
                            &svg_resources_off[i];
                        if let Some(c) = clip {
                            pass.set_scissor_rect(
                                c.x.max(0.0) as u32,
                                c.y.max(0.0) as u32,
                                (c.w.max(1.0) as u32)
                                    .min(width.saturating_sub(c.x.max(0.0) as u32)),
                                (c.h.max(1.0) as u32)
                                    .min(height.saturating_sub(c.y.max(0.0) as u32)),
                            );
                        }
                        self.image_offscreen.record(
                            &mut pass, vp_bg_svg, z_bg_svg, tex_bg, params_bg, vbuf, ibuf, 6,
                        );
                        if clip.is_some() {
                            pass.set_scissor_rect(0, 0, width, height);
                        }
                    }
                    DrawItemOff::ExternalTexture(i) => {
                        let (vbuf, ibuf, vp_bg_ext, z_bg_ext, tex_bg, params_bg, _, _) =
                            &ext_resources_off[i];
                        self.image_offscreen.record(
                            &mut pass, vp_bg_ext, z_bg_ext, tex_bg, params_bg, vbuf, ibuf, 6,
                        );
                    }
                }
            }
        }

        // Drop the pass to complete offscreen rendering
        drop(pass);

        // Composite offscreen target to surface
        self.composite_to_surface(encoder, surface_view, &targets, Some(clear));
        allocator.release_texture(targets.color);
    }
}
