//! Offscreen (intermediate target) path of `render_unified`.
//!
//! Verbatim extraction of the offscreen branch body that previously lived
//! inline in `render_unified`. Resource preparation that must outlive the
//! render pass is delegated to the `prep_*` helpers (see `text_prep` /
//! `quad_prep`); everything else is unchanged. No logic changed.

use super::{PassManager, set_scissor_for_clip, transformed_quad_points};
use crate::allocator::RenderAllocator;
use crate::upload::GpuScene;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
impl PassManager {
    pub(super) fn render_unified_offscreen(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        allocator: &mut RenderAllocator,
        surface_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        scene: &GpuScene,
        solid_batches: &[crate::upload::SolidBatch],
        transparent_scene: &GpuScene,
        transparent_batches: &[crate::upload::TransparentBatch],
        glyph_draws: &[(
            [f32; 2],
            crate::text::RasterizedGlyph,
            crate::ColorLinPremul,
            i32,
            Option<crate::Rect>,
        )],
        svg_draws: &[(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            Option<crate::SvgStyle>,
            i32,
            f32,
            crate::Transform2D,
            Option<crate::Rect>,
            Option<crate::RoundedRectClipGpu>,
        )],
        image_draws: &[(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            i32,
            f32,
            Option<crate::Rect>,
            Option<crate::RoundedRectClipGpu>,
        )],
        backdrop_blur_draws: &[crate::BackdropBlurDraw],
        external_texture_draws: &[crate::upload::ExtractedExternalTextureDraw],
        clear: wgpu::Color,
        queue: &wgpu::Queue,
        inv_logical: f32,
        transparent_text_z: &std::collections::HashSet<i32>,
    ) {
        // Create viewport bind group
        let vp_bg_off = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vp-bg-offscreen"),
            layout: self.solid_offscreen.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Offscreen path - unified rendering to offscreen target
        let targets = self.alloc_targets(allocator, width.max(1), height.max(1));

        // Pre-fetch (and lazily load) all image views before render pass (to avoid mutable borrow conflicts)
        let mut image_views_off: Vec<(
            wgpu::TextureView,
            [f32; 2],
            [f32; 2],
            f32,
            f32,
            Option<crate::Rect>,
            Option<&crate::RoundedRectClipGpu>,
        )> = Vec::new();
        // eprintln!("🔍 Pre-fetching {} images for unified offscreen render", image_draws.len());
        for (path, origin, size, z, opacity, clip, rounded_clip) in image_draws.iter() {
            // eprintln!("  📦 Image at z={}: {:?}", z, path.file_name().unwrap_or_default());
            let tex_opt = if let Some(view) = self.try_get_image_view(std::path::Path::new(path)) {
                Some(view)
            } else {
                self.load_image_to_view(std::path::Path::new(path), queue)
            };
            if let Some((tex_view, _w, _h)) = tex_opt {
                image_views_off.push((
                    tex_view,
                    *origin,
                    *size,
                    *z as f32,
                    *opacity,
                    *clip,
                    rounded_clip.as_ref(),
                ));
            }
        }

        // Pre-rasterize all SVGs before creating render pass (to avoid mutable borrow conflicts)
        let mut svg_views_off: Vec<(
            wgpu::TextureView,
            [[f32; 2]; 4],
            f32,
            f32,
            Option<crate::Rect>,
            Option<&crate::RoundedRectClipGpu>,
        )> = Vec::new();
        for (path, origin, max_size, style, _z, opacity, transform, clip, rounded_clip) in
            svg_draws.iter()
        {
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
                    let transformed_quad =
                        transformed_quad_points(*origin, [draw_w, draw_h], *transform);
                    svg_views_off.push((
                        view_scaled,
                        transformed_quad,
                        *_z as f32,
                        *opacity,
                        *clip,
                        rounded_clip.as_ref(),
                    ));
                }
            }
        }

        let mut text_groups_off =
            self.prep_text_offscreen(glyph_draws, transparent_text_z, inv_logical, queue);

        // Sort text groups by z-index (back to front)
        text_groups_off.sort_by_key(|(z, _, _, _, _, _, _)| *z);

        // Create text bind groups (use offscreen text renderer for offscreen rendering)
        let vp_bg_text_off = self
            .text_offscreen
            .vp_bind_group(&self.device, &self.vp_buffer);

        let (image_z_vals_off, image_resources_off) =
            self.prep_image_offscreen(&image_views_off, queue);
        let (svg_z_vals_off, svg_resources_off) = self.prep_svg_offscreen(&svg_views_off, queue);
        let (ext_z_vals_off, ext_resources_off) =
            self.prep_ext_offscreen(external_texture_draws, queue);

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

        // Render opaque solids in per-clip batches (offscreen path).
        if solid_batches.is_empty() {
            self.solid_offscreen.record(&mut pass, &vp_bg_off, scene);
        } else {
            for batch in solid_batches {
                if batch.index_count == 0 {
                    continue;
                }
                if let Some(c) = batch.clip {
                    if !set_scissor_for_clip(&mut pass, c, width, height) {
                        continue;
                    }
                }
                self.solid_offscreen.record_index_range(
                    &mut pass,
                    &vp_bg_off,
                    scene,
                    batch.index_start,
                    batch.index_count,
                );
                if batch.clip.is_some() {
                    pass.set_scissor_rect(0, 0, width, height);
                }
            }
        }

        // Unified z-sorted rendering (offscreen path): interleave ALL draw types
        // by z-index for correct depth ordering across element types.
        drop(pass);
        {
            #[derive(Clone, Copy)]
            enum DrawItemOff {
                BackdropBlur(usize),
                TransparentBatch(usize),
                TextGroup(usize),
                Image(usize),
                Svg(usize),
                ExternalTexture(usize),
            }
            let mut all_items: Vec<(i32, DrawItemOff)> = Vec::new();
            for (i, backdrop_blur) in backdrop_blur_draws.iter().enumerate() {
                all_items.push((backdrop_blur.z, DrawItemOff::BackdropBlur(i)));
            }
            for (i, batch) in transparent_batches.iter().enumerate() {
                all_items.push((batch.z, DrawItemOff::TransparentBatch(i)));
            }
            for (i, (z, _, _, _, _, _, _)) in text_groups_off.iter().enumerate() {
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
                if let DrawItemOff::BackdropBlur(i) = item {
                    self.draw_backdrop_blur_rect(
                        encoder,
                        allocator,
                        &targets.color,
                        width,
                        height,
                        backdrop_blur_draws[i],
                        queue,
                    );
                    continue;
                }

                let depth_attachment = Some(wgpu::RenderPassDepthStencilAttachment {
                    view: self.depth_view(),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                });
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("unified-offscreen-transparent-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &targets.color.view,
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

                match item {
                    DrawItemOff::BackdropBlur(_) => unreachable!(),
                    DrawItemOff::TransparentBatch(i) => {
                        let batch = &transparent_batches[i];
                        if let Some(c) = batch.clip {
                            if !set_scissor_for_clip(&mut pass, c, width, height) {
                                continue;
                            }
                        }
                        self.transparent_solid_offscreen.record_index_range(
                            &mut pass,
                            &vp_bg_off,
                            transparent_scene,
                            batch.index_start,
                            batch.index_count,
                        );
                        if batch.clip.is_some() {
                            pass.set_scissor_rect(0, 0, width, height);
                        }
                    }
                    DrawItemOff::TextGroup(i) => {
                        let (_z, vbuf, ibuf, index_count, z_bg, _z_buf, clip) = &text_groups_off[i];
                        if *index_count > 0 {
                            if let Some(c) = clip {
                                if !set_scissor_for_clip(&mut pass, *c, width, height) {
                                    continue;
                                }
                            }
                            pass.set_pipeline(&self.text_offscreen.pipeline);
                            pass.set_bind_group(0, &vp_bg_text_off, &[]);
                            pass.set_bind_group(1, z_bg, &[]);
                            pass.set_bind_group(2, &self.text_bind_group, &[]);
                            pass.set_vertex_buffer(0, vbuf.slice(..));
                            pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
                            pass.draw_indexed(0..*index_count, 0, 0..1);
                            if clip.is_some() {
                                pass.set_scissor_rect(0, 0, width, height);
                            }
                        }
                    }
                    DrawItemOff::Image(i) => {
                        let (vbuf, ibuf, vp_bg_img, z_bg_img, tex_bg, params_bg, _, _, clip) =
                            &image_resources_off[i];
                        if let Some(c) = clip {
                            if !set_scissor_for_clip(&mut pass, *c, width, height) {
                                continue;
                            }
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
                            if !set_scissor_for_clip(&mut pass, *c, width, height) {
                                continue;
                            }
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

        // Composite offscreen target to surface
        self.composite_to_surface(encoder, surface_view, &targets, Some(clear));
        allocator.release_texture(targets.color);
    }
}
