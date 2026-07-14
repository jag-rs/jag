use anyhow::Result;

use jag_draw::{ColorLinPremul, Command, wgpu};

use crate::canvas::Canvas;

use super::{JagSurface, apply_transform_to_point, calculate_image_fit};

impl JagSurface {
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

        // Box shadows composite in sRGB (gamma) space, which only the
        // offscreen path implements; force intermediate when shadows exist.
        let use_intermediate = self.enable_smaa
            || self.use_intermediate
            || !cache.backdrop_blur_draws.is_empty()
            || !cache.shadow_instances.is_empty();
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

        // Set the cached shadow instances before taking the immutable cache
        // borrow (the setter needs `&mut self.pass`). Cloned out of the cache.
        let cached_shadows = self.frame_cache.as_ref().unwrap().shadow_instances.clone();
        self.pass.set_shadow_instances(&cached_shadows);

        // Re-borrow the cache after the mutable borrows above are done.
        let cache = self.frame_cache.as_ref().unwrap();
        self.pass.render_unified(
            &mut encoder,
            &mut self.allocator,
            &scene_view,
            width,
            height,
            &cache.gpu_scene,
            &cache.solid_batches,
            &cache.transparent_gpu_scene,
            &cache.transparent_batches,
            &cache.glyph_draws,
            &cache.svg_draws,
            &cache.image_draws,
            &cache.backdrop_blur_draws,
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
        self.register_generated_mask_textures(&canvas.generated_mask_textures);

        // Build final display list from painter
        let mut list = canvas.painter.finish();
        let width = canvas.viewport.width.max(1);
        let height = canvas.viewport.height.max(1);

        if list.commands.iter().any(|cmd| {
            matches!(
                cmd,
                Command::PushOpacity(_)
                    | Command::PopOpacity
                    | Command::PushFilter(_)
                    | Command::PopFilter
            )
        }) {
            let flattened =
                self.flatten_effect_groups(&list.commands, list.viewport, text_provider.as_ref())?;
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
                    logical_size: 0.0,
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
                    glyph_draws.push((origin, g.clone(), run.color, text_draw.z, text_draw.clip));
                }
            }
        }

        // Sort and resolve SVG draws
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

        // Sort and prepare image draws
        let mut image_draws = canvas.image_draws.clone();
        image_draws.sort_by_key(|(_, _, _, _, z, _, _, _, _)| *z);

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
            let resolved_path = crate::resolve_asset_path(path);
            if let Some((tex_view, img_w, img_h)) = self.pass.try_get_image_view(&resolved_path) {
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
        self.pass
            .set_shadow_instances(&unified_scene.shadow_instances);
        self.pass.render_unified(
            &mut encoder,
            &mut self.allocator,
            &texture_view,
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
            canvas.backdrop_blur_draws.is_empty(), // direct unless framebuffer sampling is needed
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
