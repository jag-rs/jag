use std::sync::Arc;

use anyhow::Result;

use jag_draw::{ColorLinPremul, Command, HitIndex, Painter, Viewport, wgpu};

use crate::canvas::Canvas;

use super::{
    CachedFrameData, JagSurface, apply_transform_to_point, calculate_image_fit,
    set_last_raw_image_rect,
};

impl JagSurface {
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
            backdrop_blur_draws: Vec::new(),
            raw_image_draws: Vec::new(),
            dpi_scale: self.dpi_scale,
            clip_stack: vec![None],
            rounded_clip_stack: vec![None],
            overlay_draws: Vec::new(),
            scrim_draws: Vec::new(),
            opacity_stack: vec![1.0],
        }
    }

    /// Finish the frame by rendering accumulated commands to the provided surface texture.
    pub fn end_frame(&mut self, frame: wgpu::SurfaceTexture, canvas: Canvas) -> Result<()> {
        // Keep passes in sync with DPI/logical settings
        self.pass.set_scale_factor(self.dpi_scale);
        self.pass.set_logical_pixels(self.logical_pixels);
        self.pass.set_ui_scale(self.ui_scale);
        self.pending_image_loads = false;
        self.pass.poll_image_loads(&self.queue);

        let text_provider = canvas.text_provider.clone();

        // Build final display list from painter
        let mut list = canvas.painter.finish();
        let width = canvas.viewport.width.max(1);
        let height = canvas.viewport.height.max(1);
        let has_backdrop_blur = !canvas.backdrop_blur_draws.is_empty();

        // Determine the render target: prefer intermediate when SMAA, Vello-style
        // resizing, or framebuffer-sampling effects are in use.
        let use_intermediate = self.enable_smaa || self.use_intermediate || has_backdrop_blur;

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
            .map(
                |(path, origin, max_size, style, z, opacity, transform, clip, rounded_clip)| {
                    let resolved_path = crate::resolve_asset_path(path);
                    (
                        resolved_path,
                        *origin,
                        *max_size,
                        *style,
                        *z,
                        *opacity,
                        *transform,
                        *clip,
                        rounded_clip
                            .as_ref()
                            .map(|rc| jag_draw::RoundedRectClipGpu {
                                rect: [rc.rect.x, rc.rect.y, rc.rect.w, rc.rect.h],
                                radii: rc.radii,
                            }),
                    )
                },
            )
            .collect();
        svg_draws.sort_by_key(|(_, _, _, _, z, _, _, _, _)| *z);

        // Sort image draws by z-index and prepare simplified data (for unified pass)
        let mut image_draws = canvas.image_draws.clone();
        image_draws.sort_by_key(|(_, _, _, _, z, _, _, _, _)| *z);

        // Convert image draws to simplified format. Cache misses start an
        // asynchronous CPU decode and skip this frame instead of blocking tab
        // switches or other interactive paints.
        let mut prepared_images: Vec<(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            i32,
            f32,
            Option<jag_draw::Rect>,
            Option<jag_draw::RoundedRectClipGpu>,
        )> = Vec::new();
        for (path, origin, size, fit, z, opacity, transform, clip, rounded_clip) in
            image_draws.iter()
        {
            // Resolve path to check app bundle resources
            let resolved_path = crate::resolve_asset_path(path);

            if let Some((tex_view, img_w, img_h)) = self.pass.try_get_image_view(&resolved_path) {
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
                    *opacity,
                    *clip,
                    rounded_clip
                        .as_ref()
                        .map(|rc| jag_draw::RoundedRectClipGpu {
                            rect: [rc.rect.x, rc.rect.y, rc.rect.w, rc.rect.h],
                            radii: rc.radii,
                        }),
                ));
            } else {
                self.pending_image_loads |= self.pass.request_image_load(&resolved_path);
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
                1.0,
                raw_draw.clip,
                None, // no rounded clip for raw images
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
                    logical_size: 0.0,
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
                    glyph_draws.push((origin, g.clone(), run.color, text_draw.z, text_draw.clip));
                }
            }
        }

        // Unified solids + text/images/SVGs pass
        let preserve_surface = self.preserve_surface;
        let direct = if has_backdrop_blur {
            false
        } else {
            self.direct || !use_intermediate
        };
        self.pass.render_unified(
            &mut encoder,
            &mut self.allocator,
            &scene_view,
            width,
            height,
            &unified_scene.gpu_scene,
            &unified_scene.solid_batches,
            &unified_scene.transparent_gpu_scene,
            &unified_scene.transparent_batches,
            &glyph_draws,
            &svg_draws,
            &prepared_images,
            &canvas.backdrop_blur_draws,
            &unified_scene.external_texture_draws,
            clear_wgpu,
            direct,
            &self.queue,
            preserve_surface,
        );

        let mut reusable_gpu_scenes = None;
        if self.frame_cache_enabled {
            // Cache the frame data for scroll-only fast path reuse.
            // The GPU buffers inside `unified_scene` persist as long as this
            // cache entry is alive, allowing `render_cached_frame` to re-render
            // without rebuilding the display list or re-uploading geometry.
            self.frame_cache = Some(CachedFrameData {
                gpu_scene: unified_scene.gpu_scene,
                solid_batches: unified_scene.solid_batches,
                transparent_gpu_scene: unified_scene.transparent_gpu_scene,
                transparent_batches: unified_scene.transparent_batches,
                glyph_draws,
                svg_draws,
                image_draws: prepared_images,
                backdrop_blur_draws: canvas.backdrop_blur_draws.clone(),
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
        } else {
            self.frame_cache = None;
            reusable_gpu_scenes =
                Some((unified_scene.gpu_scene, unified_scene.transparent_gpu_scene));
        }

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
        if let Some((gpu_scene, transparent_gpu_scene)) = reusable_gpu_scenes {
            let _ = self.device.poll(wgpu::Maintain::Wait);
            self.allocator.release_buffer(gpu_scene.vertex);
            self.allocator.release_buffer(gpu_scene.index);
            self.allocator.release_buffer(transparent_gpu_scene.vertex);
            self.allocator.release_buffer(transparent_gpu_scene.index);
        }
        Ok(())
    }
}
