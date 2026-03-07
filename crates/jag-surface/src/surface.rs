use std::sync::{Arc, Mutex};

use anyhow::Result;

use jag_draw::{
    ColorLinPremul,
    Command,
    DisplayList,
    ExternalTextureId,
    HitIndex,
    Painter,
    PassManager,
    Rect,
    RenderAllocator,
    Transform2D,
    Viewport,
    wgpu, // import wgpu from engine-core to keep type identity
};

use crate::canvas::{Canvas, ImageFitMode};

/// Cached GPU resources from a previous `end_frame` call, enabling scroll-only
/// frames to skip the expensive IR walk, display list build, and GPU upload.
/// Instead, the cached buffers are re-rendered with a delta scroll offset applied
/// via the viewport uniform.
#[allow(clippy::type_complexity)]
pub struct CachedFrameData {
    /// The GPU scene (vertex/index buffers for opaque geometry).
    pub gpu_scene: jag_draw::GpuScene,
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
    )>,
    /// Resolved SVG draws.
    pub svg_draws: Vec<(
        std::path::PathBuf,
        [f32; 2],
        [f32; 2],
        Option<jag_draw::SvgStyle>,
        i32,
        Transform2D,
        Option<jag_draw::Rect>,
    )>,
    /// Resolved image draws.
    pub image_draws: Vec<(
        std::path::PathBuf,
        [f32; 2],
        [f32; 2],
        i32,
        Option<jag_draw::Rect>,
    )>,
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
fn apply_transform_to_point(point: [f32; 2], transform: Transform2D) -> [f32; 2] {
    let [a, b, c, d, e, f] = transform.m;
    let x = point[0];
    let y = point[1];
    [a * x + c * y + e, b * x + d * y + f]
}

/// Storage for the last rendered raw image rect (used for hit testing WebViews).
static LAST_RAW_IMAGE_RECT: Mutex<Option<(f32, f32, f32, f32)>> = Mutex::new(None);

/// Set the last raw image rect (called during rendering).
fn set_last_raw_image_rect(x: f32, y: f32, w: f32, h: f32) {
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
fn calculate_image_fit(
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
}

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

    fn allocate_synthetic_external_texture_id(&mut self) -> ExternalTextureId {
        let id = ExternalTextureId(self.next_synthetic_external_texture_id);
        self.next_synthetic_external_texture_id =
            self.next_synthetic_external_texture_id.wrapping_add(1);
        id
    }

    fn opacity_group_z(commands: &[Command]) -> Option<i32> {
        commands.iter().filter_map(Command::z_index).min()
    }

    fn collect_opacity_group(commands: &[Command], start_idx: usize) -> (Vec<Command>, usize) {
        let mut depth = 1usize;
        let mut i = start_idx;
        let mut group = Vec::new();

        while i < commands.len() {
            match &commands[i] {
                Command::PushOpacity(_) => {
                    depth += 1;
                    group.push(commands[i].clone());
                }
                Command::PopOpacity => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return (group, i + 1);
                    }
                    group.push(Command::PopOpacity);
                }
                _ => group.push(commands[i].clone()),
            }
            i += 1;
        }

        (group, i)
    }

    fn build_glyph_draws_from_text_draws(
        &self,
        text_draws: &[jag_draw::ExtractedTextDraw],
        provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Vec<(
        [f32; 2],
        jag_draw::RasterizedGlyph,
        jag_draw::ColorLinPremul,
        i32,
    )> {
        let Some(provider) = provider else {
            return Vec::new();
        };

        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };
        let snap = |v: f32| -> f32 { (v * sf).round() / sf };

        let mut glyph_draws = Vec::new();
        for text_draw in text_draws {
            let run = &text_draw.run;
            let [a, b, c, d, e, f] = text_draw.transform.m;

            let origin_x = a * run.pos[0] + c * run.pos[1] + e;
            let origin_y = b * run.pos[0] + d * run.pos[1] + f;

            let sx = (a * a + b * b).sqrt();
            let sy = (c * c + d * d).sqrt();
            let mut s = if sx.is_finite() && sy.is_finite() {
                if sx > 0.0 && sy > 0.0 {
                    (sx + sy) * 0.5
                } else {
                    sx.max(sy).max(1.0)
                }
            } else {
                1.0
            };
            if !s.is_finite() || s <= 0.0 {
                s = 1.0;
            }

            let logical_size = (run.size * s).max(1.0);
            let physical_size = (logical_size * sf).max(1.0);
            let run_for_provider = jag_draw::TextRun {
                text: run.text.clone(),
                pos: [0.0, 0.0],
                size: physical_size,
                color: run.color,
                weight: run.weight,
                style: run.style,
                family: run.family.clone(),
            };

            let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &run_for_provider);
            for g in glyphs.iter() {
                let mut origin = [origin_x + g.offset[0] / sf, origin_y + g.offset[1] / sf];
                if logical_size <= 15.0 {
                    origin[0] = snap(origin[0]);
                    origin[1] = snap(origin[1]);
                }
                // Opacity groups are rendered into intermediate layers; LCD/subpixel text
                // can ghost when composited again. Force grayscale AA in this path.
                glyph_draws.push((
                    origin,
                    Self::grayscale_glyph_for_compositing(g),
                    run.color,
                    text_draw.z,
                ));
            }
        }

        glyph_draws
    }

    fn grayscale_glyph_for_compositing(
        glyph: &jag_draw::RasterizedGlyph,
    ) -> jag_draw::RasterizedGlyph {
        use jag_draw::{GlyphMask, MaskFormat, SubpixelMask};

        let mask = match &glyph.mask {
            GlyphMask::Color(c) => GlyphMask::Color(c.clone()),
            GlyphMask::Subpixel(m) => match m.format {
                MaskFormat::Rgba8 => {
                    let mut out = Vec::with_capacity(m.data.len());
                    for px in m.data.chunks_exact(4) {
                        let gray =
                            ((u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3) as u8;
                        out.extend_from_slice(&[gray, gray, gray, 0]);
                    }
                    GlyphMask::Subpixel(SubpixelMask {
                        width: m.width,
                        height: m.height,
                        format: MaskFormat::Rgba8,
                        data: out,
                    })
                }
                MaskFormat::Rgba16 => {
                    let mut out = Vec::with_capacity(m.data.len());
                    for px in m.data.chunks_exact(8) {
                        let r = u16::from_le_bytes([px[0], px[1]]);
                        let g = u16::from_le_bytes([px[2], px[3]]);
                        let b = u16::from_le_bytes([px[4], px[5]]);
                        let gray = ((u32::from(r) + u32::from(g) + u32::from(b)) / 3) as u16;
                        let gb = gray.to_le_bytes();
                        out.extend_from_slice(&[gb[0], gb[1], gb[0], gb[1], gb[0], gb[1], 0, 0]);
                    }
                    GlyphMask::Subpixel(SubpixelMask {
                        width: m.width,
                        height: m.height,
                        format: MaskFormat::Rgba16,
                        data: out,
                    })
                }
            },
        };

        jag_draw::RasterizedGlyph {
            offset: glyph.offset,
            mask,
        }
    }

    fn render_opacity_group_layer(
        &mut self,
        viewport: Viewport,
        commands: Vec<Command>,
        text_provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Result<ExternalTextureId> {
        let mut group_list = DisplayList { viewport, commands };
        group_list.sort_by_z();

        let group_scene =
            jag_draw::upload_display_list_unified(&mut self.allocator, &self.queue, &group_list)?;
        let group_glyphs =
            self.build_glyph_draws_from_text_draws(&group_scene.text_draws, text_provider);

        let mut group_svgs: Vec<_> = group_scene
            .svg_draws
            .iter()
            .map(|draw| {
                (
                    crate::resolve_asset_path(&draw.path),
                    draw.origin,
                    draw.size,
                    None,
                    draw.z,
                    Transform2D::identity(),
                    None, // no clip for opacity group internals
                )
            })
            .collect();
        group_svgs.sort_by_key(|(_, _, _, _, z, _, _)| *z);

        let mut group_images: Vec<(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            i32,
            Option<jag_draw::Rect>,
        )> = Vec::new();
        for draw in &group_scene.image_draws {
            let resolved_path = crate::resolve_asset_path(&draw.path);
            if self
                .pass
                .load_image_to_view(&resolved_path, &self.queue)
                .is_some()
            {
                group_images.push((resolved_path, draw.origin, draw.size, draw.z, None));
            }
        }
        group_images.sort_by_key(|(_, _, _, z, _)| *z);

        let width = viewport.width.max(1);
        let height = viewport.height.max(1);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("opacity-group-layer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let layer_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("opacity-group-encoder"),
            });
        // Opacity groups render in their own coordinate space with no scroll.
        let saved_scroll = self.pass.scroll_offset();
        self.pass.set_scroll_offset([0.0, 0.0]);
        self.pass.render_unified(
            &mut encoder,
            &mut self.allocator,
            &layer_view,
            width,
            height,
            &group_scene.gpu_scene,
            &group_scene.transparent_gpu_scene,
            &group_scene.transparent_batches,
            &group_glyphs,
            &group_svgs,
            &group_images,
            &group_scene.external_texture_draws,
            wgpu::Color::TRANSPARENT,
            true,
            &self.queue,
            false,
        );
        self.pass.set_scroll_offset(saved_scroll);
        self.queue.submit(std::iter::once(encoder.finish()));

        let tex_id = self.allocate_synthetic_external_texture_id();
        self.pass.register_external_texture(tex_id, layer_view);
        Ok(tex_id)
    }

    fn flatten_opacity_groups(
        &mut self,
        commands: &[Command],
        viewport: Viewport,
        text_provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Result<Vec<Command>> {
        let mut out: Vec<Command> = Vec::new();
        let mut i = 0usize;
        while i < commands.len() {
            match &commands[i] {
                Command::PushOpacity(opacity) => {
                    let (raw_group, next_i) = Self::collect_opacity_group(commands, i + 1);
                    let flattened_group =
                        self.flatten_opacity_groups(&raw_group, viewport, text_provider)?;

                    // Preserve hit-only regions outside the composited layer.
                    for cmd in flattened_group.iter() {
                        match cmd {
                            Command::HitRegionRect { .. }
                            | Command::HitRegionRoundedRect { .. }
                            | Command::HitRegionEllipse { .. } => out.push(cmd.clone()),
                            _ => {}
                        }
                    }

                    let layer_opacity = opacity.clamp(0.0, 1.0);
                    if layer_opacity > 0.0
                        && let Some(z) = Self::opacity_group_z(&flattened_group)
                    {
                        // DrawExternalTexture coordinates are interpreted in logical units
                        // by PassManager when logical pixel mode is enabled.
                        let logical_scale = jag_draw::logical_multiplier(
                            self.logical_pixels,
                            self.dpi_scale,
                            self.ui_scale,
                        );
                        let logical_w = (viewport.width as f32) / logical_scale;
                        let logical_h = (viewport.height as f32) / logical_scale;
                        let tex_id = self.render_opacity_group_layer(
                            viewport,
                            flattened_group,
                            text_provider,
                        )?;
                        out.push(Command::DrawExternalTexture {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                w: logical_w,
                                h: logical_h,
                            },
                            texture_id: tex_id,
                            z,
                            transform: Transform2D::identity(),
                            opacity: layer_opacity,
                            premultiplied: true,
                        });
                    }
                    i = next_i;
                }
                Command::PopOpacity => {
                    // Ignore unmatched pops.
                    i += 1;
                }
                _ => {
                    out.push(commands[i].clone());
                    i += 1;
                }
            }
        }
        Ok(out)
    }

    /// Pre-allocate intermediate texture at the given size.
    /// This should be called after surface reconfiguration to avoid jitter.
    pub fn prepare_for_resize(&mut self, width: u32, height: u32) {
        self.pass
            .ensure_intermediate_texture(&mut self.allocator, width, height);
    }

    /// Begin a canvas frame of the given size (in pixels).
    pub fn begin_frame(&self, width: u32, height: u32) -> Canvas {
        let vp = Viewport { width, height };
        Canvas {
            viewport: vp,
            painter: Painter::begin_frame(vp),
            clear_color: None,
            text_provider: None,
            glyph_draws: Vec::new(),
            svg_draws: Vec::new(),
            image_draws: Vec::new(),
            raw_image_draws: Vec::new(),
            dpi_scale: self.dpi_scale,
            clip_stack: vec![None],
            overlay_draws: Vec::new(),
            scrim_draws: Vec::new(),
        }
    }

    /// Finish the frame by rendering accumulated commands to the provided surface texture.
    pub fn end_frame(&mut self, frame: wgpu::SurfaceTexture, canvas: Canvas) -> Result<()> {
        // Keep passes in sync with DPI/logical settings
        self.pass.set_scale_factor(self.dpi_scale);
        self.pass.set_logical_pixels(self.logical_pixels);
        self.pass.set_ui_scale(self.ui_scale);

        // Determine the render target: prefer intermediate when SMAA or Vello-style resizing is on.
        let use_intermediate = self.enable_smaa || self.use_intermediate;

        let text_provider = canvas.text_provider.clone();

        // Build final display list from painter
        let mut list = canvas.painter.finish();
        let width = canvas.viewport.width.max(1);
        let height = canvas.viewport.height.max(1);

        if list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, Command::PushOpacity(_) | Command::PopOpacity))
        {
            let flattened =
                self.flatten_opacity_groups(&list.commands, list.viewport, text_provider.as_ref())?;
            list.commands = flattened;
        }

        // Sort display list by z-index to ensure proper layering
        list.sort_by_z();

        // Create target view
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let scene_view = if use_intermediate {
            self.pass
                .ensure_intermediate_texture(&mut self.allocator, width, height);
            let scene_target = self
                .pass
                .intermediate_texture
                .as_ref()
                .expect("intermediate render target not allocated");
            scene_target
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default())
        } else {
            frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default())
        };

        // Command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("jag-surface-encoder"),
            });

        // Clear color or transparent
        let clear = canvas.clear_color.unwrap_or(ColorLinPremul {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });
        let clear_wgpu = wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: clear.a as f64,
        };

        // Ensure depth texture is allocated for z-ordering (Phase 1 of depth buffer implementation)
        self.pass
            .ensure_depth_texture(&mut self.allocator, width, height);

        // Extract unified scene data (solids + text/image/svg draws) from the display list.
        let unified_scene =
            jag_draw::upload_display_list_unified(&mut self.allocator, &self.queue, &list)?;

        // Sort SVG draws by z-index and resolve paths for app bundle
        let mut svg_draws: Vec<_> = canvas
            .svg_draws
            .iter()
            .map(|(path, origin, max_size, style, z, transform, clip)| {
                let resolved_path = crate::resolve_asset_path(path);
                (
                    resolved_path,
                    *origin,
                    *max_size,
                    *style,
                    *z,
                    *transform,
                    *clip,
                )
            })
            .collect();
        svg_draws.sort_by_key(|(_, _, _, _, z, _, _)| *z);

        // Sort image draws by z-index and prepare simplified data (for unified pass)
        let mut image_draws = canvas.image_draws.clone();
        image_draws.sort_by_key(|(_, _, _, _, z, _, _)| *z);

        // Convert image draws to simplified format (path, origin, size, z, clip)
        // Apply transforms and fit calculations here. We synchronously load images
        // via PassManager so that they appear on the very first frame, without
        // requiring a scroll/resize to trigger a second redraw.
        //
        // NOTE: Origins in `canvas.image_draws` are already in logical coordinates;
        // they will be scaled by PassManager via logical_pixels/dpi.
        let mut prepared_images: Vec<(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            i32,
            Option<jag_draw::Rect>,
        )> = Vec::new();
        for (path, origin, size, fit, z, transform, clip) in image_draws.iter() {
            // Resolve path to check app bundle resources
            let resolved_path = crate::resolve_asset_path(path);

            // Synchronously load (or fetch from cache) to ensure the texture
            // is available for this frame. This mirrors the demo-app unified
            // path and avoids images only appearing after a later redraw.
            if let Some((tex_view, img_w, img_h)) =
                self.pass.load_image_to_view(&resolved_path, &self.queue)
            {
                drop(tex_view); // Only need dimensions here
                let transformed_origin = apply_transform_to_point(*origin, *transform);
                let (render_origin, render_size) = calculate_image_fit(
                    transformed_origin,
                    *size,
                    img_w as f32,
                    img_h as f32,
                    *fit,
                );
                prepared_images.push((
                    resolved_path.clone(),
                    render_origin,
                    render_size,
                    *z,
                    *clip,
                ));
            }
        }

        // Process raw image draws (e.g., WebView CEF pixels)
        // Optimizations:
        // 1. Always reuse textures - only recreate if size changes
        // 2. Use BGRA format to match CEF native output (no CPU conversion)
        // 3. Support dirty rect partial uploads
        for (i, raw_draw) in canvas.raw_image_draws.iter().enumerate() {
            if raw_draw.src_width == 0 || raw_draw.src_height == 0 {
                continue;
            }

            // Use a fixed path for webview texture - reused across frames
            let raw_path = std::path::PathBuf::from(format!("__webview_texture_{}__", i));

            // If pixels are empty, reuse cached texture from previous frame
            let has_new_pixels = !raw_draw.pixels.is_empty();

            // Check if we need a new texture (only if size changed)
            let need_new_texture =
                if let Some((_, cached_w, cached_h)) = self.pass.try_get_image_view(&raw_path) {
                    // Only recreate if dimensions changed - always reuse otherwise
                    cached_w != raw_draw.src_width || cached_h != raw_draw.src_height
                } else {
                    true
                };

            // Create texture only when needed (first time or size change)
            if need_new_texture && has_new_pixels {
                // Create texture with BGRA format to match CEF's native output
                // This eliminates CPU-side BGRA->RGBA conversion
                let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("cef-webview-texture"),
                    size: wgpu::Extent3d {
                        width: raw_draw.src_width,
                        height: raw_draw.src_height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    // BGRA format matches CEF native output - no conversion needed
                    format: wgpu::TextureFormat::Bgra8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });

                // Store in image cache for reuse
                self.pass.store_loaded_image(
                    &raw_path,
                    Arc::new(texture),
                    raw_draw.src_width,
                    raw_draw.src_height,
                );
            }

            // Upload pixels only when we have new data
            if has_new_pixels {
                if let Some((tex, _, _)) = self.pass.get_cached_texture(&raw_path) {
                    // Always upload full frame - CEF provides complete buffer even for partial updates.
                    // The dirty_rects are informational but the buffer is always complete.
                    // This ensures no flickering from partial/stale data.
                    self.queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &tex,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &raw_draw.pixels,
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(raw_draw.src_width * 4),
                            rows_per_image: Some(raw_draw.src_height),
                        },
                        wgpu::Extent3d {
                            width: raw_draw.src_width,
                            height: raw_draw.src_height,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }

            // Skip rendering if no cached texture exists (no pixels uploaded yet)
            if self.pass.try_get_image_view(&raw_path).is_none() {
                continue;
            }

            // Apply the canvas transform to the origin - same as regular images.
            // The origin from draw_raw_image is in local (viewport) coordinates and
            // needs to be transformed to screen coordinates.
            let transformed_origin = apply_transform_to_point(raw_draw.origin, raw_draw.transform);

            // Store the transformed rect for hit testing (accessible via get_last_raw_image_rect)
            set_last_raw_image_rect(
                transformed_origin[0],
                transformed_origin[1],
                raw_draw.dst_size[0],
                raw_draw.dst_size[1],
            );

            prepared_images.push((
                raw_path,
                transformed_origin,
                raw_draw.dst_size,
                raw_draw.z,
                raw_draw.clip,
            ));
        }

        // Merge glyphs supplied explicitly via Canvas (draw_text_run/draw_text_direct)
        // with text runs extracted from the display list (e.g., hyperlinks) for
        // unified text rendering.
        let mut glyph_draws = canvas.glyph_draws.clone();

        if let Some(ref provider) = canvas.text_provider {
            // Use the same snapping strategy as direct text paths so small
            // text (e.g., 13–15px) lands cleanly on device pixels.
            let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
                self.dpi_scale
            } else {
                1.0
            };
            let snap = |v: f32| -> f32 { (v * sf).round() / sf };

            for text_draw in &unified_scene.text_draws {
                let run = &text_draw.run;
                let [a, b, c, d, e, f] = text_draw.transform.m;

                // Transform the run origin (baseline-left) into world coordinates.
                let origin_x = a * run.pos[0] + c * run.pos[1] + e;
                let origin_y = b * run.pos[0] + d * run.pos[1] + f;

                // Infer uniform scale from the linear part of the transform so
                // text respects any explicit scaling in the display list.
                let sx = (a * a + b * b).sqrt();
                let sy = (c * c + d * d).sqrt();
                let mut s = if sx.is_finite() && sy.is_finite() {
                    if sx > 0.0 && sy > 0.0 {
                        (sx + sy) * 0.5
                    } else {
                        sx.max(sy).max(1.0)
                    }
                } else {
                    1.0
                };
                if !s.is_finite() || s <= 0.0 {
                    s = 1.0;
                }

                // Rasterize at *physical* pixel size (scaled by DPI) to match
                // the direct canvas text path. PassManager assumes glyph bitmaps
                // are at physical resolution and divides quad sizes by DPI.
                let logical_size = (run.size * s).max(1.0);
                let physical_size = (logical_size * sf).max(1.0);
                let run_for_provider = jag_draw::TextRun {
                    text: run.text.clone(),
                    pos: [0.0, 0.0],
                    size: physical_size,
                    color: run.color,
                    weight: run.weight,
                    style: run.style,
                    family: run.family.clone(),
                };

                // Rasterize glyphs for this run and push into glyph_draws.
                // Provider offsets are in *physical* pixels (proportional to physical_size).
                // Convert back into logical coordinates so PassManager's DPI scaling
                // keeps geometry and text aligned.
                let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &run_for_provider);
                for g in glyphs.iter() {
                    let mut origin = [origin_x + g.offset[0] / sf, origin_y + g.offset[1] / sf];
                    if logical_size <= 15.0 {
                        origin[0] = snap(origin[0]);
                        origin[1] = snap(origin[1]);
                    }
                    glyph_draws.push((origin, g.clone(), run.color, text_draw.z));
                }
            }
        }

        // Unified solids + text/images/SVGs pass
        let preserve_surface = self.preserve_surface;
        let direct = self.direct || !use_intermediate;
        self.pass.render_unified(
            &mut encoder,
            &mut self.allocator,
            &scene_view,
            width,
            height,
            &unified_scene.gpu_scene,
            &unified_scene.transparent_gpu_scene,
            &unified_scene.transparent_batches,
            &glyph_draws,
            &svg_draws,
            &prepared_images,
            &unified_scene.external_texture_draws,
            clear_wgpu,
            direct,
            &self.queue,
            preserve_surface,
        );

        // Cache the frame data for scroll-only fast path reuse.
        // The GPU buffers inside `unified_scene` persist as long as this
        // cache entry is alive, allowing `render_cached_frame` to re-render
        // without rebuilding the display list or re-uploading geometry.
        self.frame_cache = Some(CachedFrameData {
            gpu_scene: unified_scene.gpu_scene,
            transparent_gpu_scene: unified_scene.transparent_gpu_scene,
            transparent_batches: unified_scene.transparent_batches,
            glyph_draws,
            svg_draws,
            image_draws: prepared_images,
            external_texture_draws: unified_scene.external_texture_draws,
            clear: clear_wgpu,
            direct,
            width,
            height,
            scroll_at_build: (0.0, 0.0), // Set by caller via set_cache_scroll_at_build
            generation_at_build: 0,      // Set by caller
            viewport_size: (width, height),
            hit_index: HitIndex::default(), // Set by caller
        });

        // Render scrims; support both simple rects and stencil cutouts.
        for scrim in &canvas.scrim_draws {
            match scrim {
                crate::ScrimDraw::Rect(rect, color) => {
                    self.pass.draw_scrim_rect(
                        &mut encoder,
                        &scene_view,
                        width,
                        height,
                        *rect,
                        *color,
                        &self.queue,
                    );
                }
                crate::ScrimDraw::Cutout { hole, color } => {
                    self.pass.draw_scrim_with_cutout(
                        &mut encoder,
                        &mut self.allocator,
                        &scene_view,
                        width,
                        height,
                        *hole,
                        *color,
                        &self.queue,
                    );
                }
            }
        }

        // Render overlay rectangles (modal scrims) without depth testing.
        // These blend over the entire scene without blocking text.
        for (rect, color) in &canvas.overlay_draws {
            self.pass.draw_overlay_rect(
                &mut encoder,
                &scene_view,
                width,
                height,
                *rect,
                *color,
                &self.queue,
            );
        }

        // Call overlay callback last so overlays (e.g., devtools, debug UI)
        // are guaranteed to draw above all other content.
        if let Some(ref mut overlay_fn) = self.overlay {
            overlay_fn(
                &mut self.pass,
                &mut encoder,
                &scene_view,
                &self.queue,
                width,
                height,
            );
        }

        // Resolve to the swapchain: SMAA when enabled, otherwise a nearest-neighbor blit for sharper text.
        if use_intermediate {
            if self.enable_smaa {
                self.pass.apply_smaa(
                    &mut encoder,
                    &mut self.allocator,
                    &scene_view,
                    &view,
                    width,
                    height,
                    &self.queue,
                );
            } else {
                self.pass.blit_to_surface(&mut encoder, &view);
            }
        }

        // Submit and present
        let cb = encoder.finish();
        self.queue.submit(std::iter::once(cb));
        frame.present();
        Ok(())
    }

    /// Re-render using the internally-cached frame data with an updated GPU
    /// scroll offset. This is the scroll-only fast path.
    ///
    /// `scroll_delta` is the negated difference between the current scroll
    /// position and the position when the frame was originally built.
    pub fn render_cached_frame_from_internal(
        &mut self,
        frame: wgpu::SurfaceTexture,
        scroll_delta: [f32; 2],
    ) -> Result<()> {
        let cache = self
            .frame_cache
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no cached frame data"))?;

        self.pass.set_scale_factor(self.dpi_scale);
        self.pass.set_logical_pixels(self.logical_pixels);
        self.pass.set_ui_scale(self.ui_scale);

        let use_intermediate = self.enable_smaa || self.use_intermediate;
        let width = cache.width;
        let height = cache.height;
        let clear = cache.clear;
        let direct = cache.direct;

        // Set the scroll delta as GPU uniform
        self.pass.set_scroll_offset(scroll_delta);

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let scene_view = if use_intermediate {
            self.pass
                .ensure_intermediate_texture(&mut self.allocator, width, height);
            let scene_target = self
                .pass
                .intermediate_texture
                .as_ref()
                .expect("intermediate render target not allocated");
            scene_target
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default())
        } else {
            frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default())
        };

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("jag-surface-cached-encoder"),
            });

        self.pass
            .ensure_depth_texture(&mut self.allocator, width, height);

        // Re-borrow the cache after the mutable borrows above are done.
        let cache = self.frame_cache.as_ref().unwrap();
        self.pass.render_unified(
            &mut encoder,
            &mut self.allocator,
            &scene_view,
            width,
            height,
            &cache.gpu_scene,
            &cache.transparent_gpu_scene,
            &cache.transparent_batches,
            &cache.glyph_draws,
            &cache.svg_draws,
            &cache.image_draws,
            &cache.external_texture_draws,
            clear,
            direct,
            &self.queue,
            false, // don't preserve surface
        );

        // Resolve to swapchain
        if use_intermediate {
            if self.enable_smaa {
                self.pass.apply_smaa(
                    &mut encoder,
                    &mut self.allocator,
                    &scene_view,
                    &view,
                    width,
                    height,
                    &self.queue,
                );
            } else {
                self.pass.blit_to_surface(&mut encoder, &view);
            }
        }

        let cb = encoder.finish();
        self.queue.submit(std::iter::once(cb));
        frame.present();

        // Reset scroll offset for subsequent full rebuilds
        self.pass.set_scroll_offset([0.0, 0.0]);

        Ok(())
    }

    /// Finish a frame by rendering to an offscreen texture and returning the
    /// pixel data as an RGBA byte vector. This is the headless equivalent of
    /// [`end_frame`] and does not require a window or surface.
    ///
    /// Returns `(width, height, pixels)` where `pixels` is tightly-packed
    /// RGBA with 4 bytes per pixel (`width * height * 4` total).
    pub fn end_frame_headless(&mut self, canvas: Canvas) -> Result<(u32, u32, Vec<u8>)> {
        // Keep passes in sync with DPI/logical settings
        self.pass.set_scale_factor(self.dpi_scale);
        self.pass.set_logical_pixels(self.logical_pixels);
        self.pass.set_ui_scale(self.ui_scale);

        // Build final display list from painter
        let text_provider = canvas.text_provider.clone();

        // Build final display list from painter
        let mut list = canvas.painter.finish();
        let width = canvas.viewport.width.max(1);
        let height = canvas.viewport.height.max(1);

        if list
            .commands
            .iter()
            .any(|cmd| matches!(cmd, Command::PushOpacity(_) | Command::PopOpacity))
        {
            let flattened =
                self.flatten_opacity_groups(&list.commands, list.viewport, text_provider.as_ref())?;
            list.commands = flattened;
        }

        // Sort display list by z-index to ensure proper layering
        list.sort_by_z();

        // Clear color or transparent
        let clear = canvas.clear_color.unwrap_or(ColorLinPremul {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 0.0,
        });
        let clear_wgpu = wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: clear.a as f64,
        };

        // Ensure depth texture for z-ordering
        self.pass
            .ensure_depth_texture(&mut self.allocator, width, height);

        // Extract unified scene data
        let unified_scene =
            jag_draw::upload_display_list_unified(&mut self.allocator, &self.queue, &list)?;

        // Process text draws from display list via text provider
        let mut glyph_draws = canvas.glyph_draws.clone();
        if let Some(ref provider) = canvas.text_provider {
            let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
                self.dpi_scale
            } else {
                1.0
            };
            let snap = |v: f32| -> f32 { (v * sf).round() / sf };

            for text_draw in &unified_scene.text_draws {
                let run = &text_draw.run;
                let [a, b, c, d, e, f] = text_draw.transform.m;

                let origin_x = a * run.pos[0] + c * run.pos[1] + e;
                let origin_y = b * run.pos[0] + d * run.pos[1] + f;

                let sx = (a * a + b * b).sqrt();
                let sy = (c * c + d * d).sqrt();
                let mut s = if sx.is_finite() && sy.is_finite() {
                    if sx > 0.0 && sy > 0.0 {
                        (sx + sy) * 0.5
                    } else {
                        sx.max(sy).max(1.0)
                    }
                } else {
                    1.0
                };
                if !s.is_finite() || s <= 0.0 {
                    s = 1.0;
                }

                let logical_size = (run.size * s).max(1.0);
                let physical_size = (logical_size * sf).max(1.0);
                let run_for_provider = jag_draw::TextRun {
                    text: run.text.clone(),
                    pos: [0.0, 0.0],
                    size: physical_size,
                    color: run.color,
                    weight: run.weight,
                    style: run.style,
                    family: run.family.clone(),
                };

                let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &run_for_provider);
                for g in glyphs.iter() {
                    let mut origin = [origin_x + g.offset[0] / sf, origin_y + g.offset[1] / sf];
                    if logical_size <= 15.0 {
                        origin[0] = snap(origin[0]);
                        origin[1] = snap(origin[1]);
                    }
                    glyph_draws.push((origin, g.clone(), run.color, text_draw.z));
                }
            }
        }

        // Sort and resolve SVG draws
        let mut svg_draws: Vec<_> = canvas
            .svg_draws
            .iter()
            .map(|(path, origin, max_size, style, z, transform, clip)| {
                let resolved_path = crate::resolve_asset_path(path);
                (
                    resolved_path,
                    *origin,
                    *max_size,
                    *style,
                    *z,
                    *transform,
                    *clip,
                )
            })
            .collect();
        svg_draws.sort_by_key(|(_, _, _, _, z, _, _)| *z);

        // Sort and prepare image draws
        let mut image_draws = canvas.image_draws.clone();
        image_draws.sort_by_key(|(_, _, _, _, z, _, _)| *z);

        let mut prepared_images: Vec<(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            i32,
            Option<jag_draw::Rect>,
        )> = Vec::new();
        for (path, origin, size, fit, z, transform, clip) in image_draws.iter() {
            let resolved_path = crate::resolve_asset_path(path);
            if let Some((tex_view, img_w, img_h)) =
                self.pass.load_image_to_view(&resolved_path, &self.queue)
            {
                drop(tex_view);
                let transformed_origin = apply_transform_to_point(*origin, *transform);
                let (render_origin, render_size) = calculate_image_fit(
                    transformed_origin,
                    *size,
                    img_w as f32,
                    img_h as f32,
                    *fit,
                );
                prepared_images.push((
                    resolved_path.clone(),
                    render_origin,
                    render_size,
                    *z,
                    *clip,
                ));
            }
        }

        // Create offscreen render target
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("headless-render-target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("headless-encoder"),
            });

        // Render unified pass directly to the offscreen texture
        self.pass.render_unified(
            &mut encoder,
            &mut self.allocator,
            &texture_view,
            width,
            height,
            &unified_scene.gpu_scene,
            &unified_scene.transparent_gpu_scene,
            &unified_scene.transparent_batches,
            &glyph_draws,
            &svg_draws,
            &prepared_images,
            &unified_scene.external_texture_draws,
            clear_wgpu,
            true, // direct rendering (no intermediate)
            &self.queue,
            false, // don't preserve surface
        );

        // Copy rendered texture to a CPU-readable buffer
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width * bytes_per_pixel;
        let padded_bytes_per_row = (unpadded_bytes_per_row + 255) & !255;
        let buffer_size = (padded_bytes_per_row * height) as u64;

        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("headless-readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        // Submit and wait
        self.queue.submit(std::iter::once(encoder.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                result.expect("failed to map readback buffer");
                tx.send(()).expect("failed to signal readback");
            });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| anyhow::anyhow!("readback recv: {}", e))?;

        // Extract tightly-packed RGBA pixels (strip row padding)
        let mapped = readback.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * bytes_per_pixel) as usize);
        for row in 0..height {
            let start = (row * padded_bytes_per_row) as usize;
            let end = start + (width * bytes_per_pixel) as usize;
            pixels.extend_from_slice(&mapped[start..end]);
        }
        drop(mapped);
        readback.unmap();

        Ok((width, height, pixels))
    }
}
