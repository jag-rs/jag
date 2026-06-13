//! Direct (surface) path of `render_unified`.
//!
//! Verbatim extraction of the `if direct { .. }` branch body that previously
//! lived inline in `render_unified`. Resource preparation that must outlive the
//! render pass is delegated to the `prep_*` helpers (see `text_prep` /
//! `quad_prep`); everything else is unchanged. No logic changed.

use super::{PassManager, set_scissor_for_clip, transformed_quad_points};
use crate::upload::GpuScene;
use wgpu::util::DeviceExt;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
impl PassManager {
    pub(super) fn render_unified_direct(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
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
        external_texture_draws: &[crate::upload::ExtractedExternalTextureDraw],
        clear: wgpu::Color,
        queue: &wgpu::Queue,
        preserve_surface: bool,
        inv_logical: f32,
        transparent_text_z: &std::collections::HashSet<i32>,
    ) {
        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("vp-bg-direct-local"),
            layout: self.solid_direct.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Build the analytic box-shadow instance buffer + viewport bind group
        // before the render pass so both outlive the pass borrow. Skipped
        // entirely when there are no shadows.
        let shadow_buf = (!self.shadow_instances.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("shadow-instances"),
                    contents: bytemuck::cast_slice(&self.shadow_instances),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let shadow_vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow-vp-bg-direct"),
            layout: self.shadow_direct.viewport_bgl(),
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
            f32,
            Option<crate::Rect>,
            Option<&crate::RoundedRectClipGpu>,
        )> = Vec::new();
        for (path, origin, size, z, opacity, clip, rounded_clip) in image_draws.iter() {
            let tex_opt = if let Some(view) = self.try_get_image_view(std::path::Path::new(path)) {
                Some(view)
            } else {
                self.load_image_to_view(std::path::Path::new(path), queue)
            };
            if let Some((tex_view, _w, _h)) = tex_opt {
                image_views.push((
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

        // Pre-rasterize all SVGs before render pass (to avoid mutable borrow conflicts)
        let mut svg_views: Vec<(
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
                    let draw_w = base_w * scale;
                    let draw_h = base_h * scale;
                    let transformed_quad =
                        transformed_quad_points(*origin, [draw_w, draw_h], *transform);
                    svg_views.push((
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

        let mut text_groups =
            self.prep_text_direct(glyph_draws, transparent_text_z, inv_logical, queue);

        // Sort text groups by z-index (back to front)
        text_groups.sort_by_key(|(z, _, _, _, _, _, _)| *z);

        // Create text bind groups before render pass so they live long enough
        let vp_bg_text = self.text.vp_bind_group(&self.device, &self.vp_buffer);

        let (image_z_vals, image_resources) = self.prep_image_direct(&image_views, queue);
        let (svg_z_vals, svg_resources) = self.prep_svg_direct(&svg_views, queue);
        let (ext_z_vals, ext_resources) = self.prep_ext_direct(external_texture_draws, queue);

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

        // Render opaque solids in per-clip batches. Each batch has its own
        // scissor rect so that overflow:hidden/scroll clips content correctly.
        if solid_batches.is_empty() {
            // No clip batches recorded — fall back to drawing all at once.
            self.solid_direct.record(&mut pass, &vp_bg, scene);
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
                self.solid_direct.record_index_range(
                    &mut pass,
                    &vp_bg,
                    scene,
                    batch.index_start,
                    batch.index_count,
                );
                if batch.clip.is_some() {
                    pass.set_scissor_rect(0, 0, width, height);
                }
            }
        }

        // Analytic box shadows: drawn after the opaque solids (so they read the
        // opaque depth) and before the transparent interleave. The instance
        // buffer was created above and outlives the pass.
        if let Some(buf) = shadow_buf.as_ref() {
            self.shadow_direct.record(
                &mut pass,
                &shadow_vp_bg,
                buf,
                self.shadow_instances.len() as u32,
            );
        }

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
        for (i, (z, _, _, _, _, _, _)) in text_groups.iter().enumerate() {
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
                    if let Some(c) = batch.clip {
                        if !set_scissor_for_clip(&mut pass, c, width, height) {
                            continue;
                        }
                    }
                    self.transparent_solid_direct.record_index_range(
                        &mut pass,
                        &vp_bg,
                        transparent_scene,
                        batch.index_start,
                        batch.index_count,
                    );
                    if batch.clip.is_some() {
                        pass.set_scissor_rect(0, 0, width, height);
                    }
                }
                DrawItem::TextGroup(i) => {
                    let (_z, vbuf, ibuf, index_count, z_bg, _z_buf, clip) = &text_groups[i];
                    if *index_count > 0 {
                        if let Some(c) = clip {
                            if !set_scissor_for_clip(&mut pass, *c, width, height) {
                                continue;
                            }
                        }
                        pass.set_pipeline(&self.text.pipeline);
                        pass.set_bind_group(0, &vp_bg_text, &[]);
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
                DrawItem::Image(i) => {
                    let (vbuf, ibuf, vp_bg_img, z_bg_img, tex_bg, params_bg, _, _, clip) =
                        &image_resources[i];
                    if let Some(c) = clip {
                        if !set_scissor_for_clip(&mut pass, *c, width, height) {
                            continue;
                        }
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
                        if !set_scissor_for_clip(&mut pass, *c, width, height) {
                            continue;
                        }
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
    }
}
