//! Image/SVG/external-texture GPU resource preparation for `render_unified`.
//!
//! Verbatim extractions of the per-quad resource-build loops that previously
//! lived inline in `render_unified`. They create all buffers and bind groups
//! (fully owned values) before the render pass so the resources outlive the
//! pass. No logic changed during extraction.

use super::{ImageQuadVtx, PassManager, composite_resources::ExtResource};
use crate::gpu_transfer::create_transient_upload;
use std::sync::Arc;

type ImageResource = (
    wgpu::Buffer,
    wgpu::Buffer,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::Buffer,
    wgpu::Buffer,
    Option<crate::Rect>,
);

type ImageViewEntry<'a> = (
    wgpu::TextureView,
    [f32; 2],
    [f32; 2],
    f32,
    f32,
    Option<crate::Rect>,
    Option<&'a crate::RoundedRectClipGpu>,
);

type SvgViewEntry<'a> = (
    wgpu::TextureView,
    [[f32; 2]; 4],
    f32,
    f32,
    Option<crate::Rect>,
    Option<&'a crate::RoundedRectClipGpu>,
);

#[allow(clippy::type_complexity)]
impl PassManager {
    pub(super) fn prep_image_direct(
        &self,
        image_views: &[ImageViewEntry<'_>],
    ) -> (Vec<i32>, Vec<ImageResource>) {
        let mut image_resources: Vec<ImageResource> = Vec::new();
        let mut image_z_vals: Vec<i32> = Vec::new();
        for (tex_view, origin, size, z_val, opacity, clip, rounded_clip) in image_views.iter() {
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

            let vbuf = create_transient_upload(
                &self.device,
                "image-vbuf-unified",
                bytemuck::cast_slice(&verts),
                wgpu::BufferUsages::VERTEX,
            );
            let ibuf = create_transient_upload(
                &self.device,
                "image-ibuf-unified",
                bytemuck::cast_slice(&idx),
                wgpu::BufferUsages::INDEX,
            );

            let vp_bg_img = self.image.vp_bind_group(&self.device, &self.vp_buffer);
            // Pass z_index as float directly - shader will convert to depth
            let (z_bg_img, z_buf_img) = self.create_group_z_bind_group(*z_val as f32);
            let tex_bg = self.image.tex_bind_group(&self.device, tex_view);
            let (params_bg, params_buf) = self.image.params_bind_group_clipped(
                &self.device,
                *opacity,
                false,
                true,
                *rounded_clip,
            );

            image_z_vals.push(*z_val as i32);
            image_resources.push((
                vbuf, ibuf, vp_bg_img, z_bg_img, tex_bg, params_bg, z_buf_img, params_buf, *clip,
            ));
        }
        (image_z_vals, image_resources)
    }

    pub(super) fn prep_svg_direct(
        &self,
        svg_views: &[SvgViewEntry<'_>],
    ) -> (Vec<i32>, Vec<ImageResource>) {
        let mut svg_z_vals: Vec<i32> = Vec::new();
        let mut svg_resources: Vec<ImageResource> = Vec::new();
        for (view_scaled, quad, z_val, opacity, clip, rounded_clip) in svg_views.iter() {
            let verts = [
                ImageQuadVtx {
                    pos: quad[0],
                    uv: [0.0, 0.0],
                },
                ImageQuadVtx {
                    pos: quad[1],
                    uv: [1.0, 0.0],
                },
                ImageQuadVtx {
                    pos: quad[2],
                    uv: [1.0, 1.0],
                },
                ImageQuadVtx {
                    pos: quad[3],
                    uv: [0.0, 1.0],
                },
            ];
            let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

            let vbuf = create_transient_upload(
                &self.device,
                "svg-vbuf-unified",
                bytemuck::cast_slice(&verts),
                wgpu::BufferUsages::VERTEX,
            );
            let ibuf = create_transient_upload(
                &self.device,
                "svg-ibuf-unified",
                bytemuck::cast_slice(&idx),
                wgpu::BufferUsages::INDEX,
            );

            let vp_bg_svg = self.image.vp_bind_group(&self.device, &self.vp_buffer);
            // Pass z_index as float directly - shader will convert to depth
            let (z_bg_svg, z_buf_svg) = self.create_group_z_bind_group(*z_val as f32);
            let tex_bg = self.image.tex_bind_group(&self.device, view_scaled);
            let (params_bg, params_buf) = self.image.params_bind_group_clipped(
                &self.device,
                *opacity,
                true,
                true,
                *rounded_clip,
            );

            svg_z_vals.push(*z_val as i32);
            svg_resources.push((
                vbuf, ibuf, vp_bg_svg, z_bg_svg, tex_bg, params_bg, z_buf_svg, params_buf, *clip,
            ));
        }
        (svg_z_vals, svg_resources)
    }

    pub(super) fn prep_ext_direct(
        &mut self,
        external_texture_draws: &[crate::upload::ExtractedExternalTextureDraw],
        queue: &wgpu::Queue,
    ) -> (Vec<i32>, Vec<Arc<ExtResource>>) {
        let mut z_values = Vec::with_capacity(external_texture_draws.len());
        let mut resources = Vec::with_capacity(external_texture_draws.len());
        if external_texture_draws.is_empty() {
            return (z_values, resources);
        }
        let vertices =
            self.composite_direct
                .vertex_batch(&self.device, queue, external_texture_draws);
        for (index, draw) in external_texture_draws.iter().enumerate() {
            let Some(view) = self.external_textures.get(&draw.texture_id) else {
                continue;
            };
            z_values.push(draw.z);
            resources.push(self.composite_direct.prepare(
                &self.device,
                queue,
                &self.image,
                &self.vp_buffer,
                self.solid_direct.z_index_bgl(),
                draw,
                view,
                self.linear_external_textures.contains(&draw.texture_id),
                &vertices,
                (index * 4) as i32,
            ));
        }
        (z_values, resources)
    }

    pub(super) fn prep_image_offscreen(
        &self,
        image_views_off: &[ImageViewEntry<'_>],
    ) -> (Vec<i32>, Vec<ImageResource>) {
        let mut image_z_vals_off: Vec<i32> = Vec::new();
        let mut image_resources_off: Vec<ImageResource> = Vec::new();
        for (tex_view, origin, size, z_val, opacity, clip, rounded_clip) in image_views_off.iter() {
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

            let vbuf = create_transient_upload(
                &self.device,
                "image-vbuf-unified-offscreen",
                bytemuck::cast_slice(&verts),
                wgpu::BufferUsages::VERTEX,
            );
            let ibuf = create_transient_upload(
                &self.device,
                "image-ibuf-unified-offscreen",
                bytemuck::cast_slice(&idx),
                wgpu::BufferUsages::INDEX,
            );

            let vp_bg_img = self
                .image_offscreen
                .vp_bind_group(&self.device, &self.vp_buffer);
            // Pass z_index as float directly - shader will convert to depth
            let (z_bg_img, z_buf_img) = self.create_group_z_bind_group(*z_val as f32);
            let tex_bg = self.image_offscreen.tex_bind_group(&self.device, tex_view);
            let (params_bg, params_buf) = self.image_offscreen.params_bind_group_clipped(
                &self.device,
                *opacity,
                false,
                true,
                *rounded_clip,
            );

            image_z_vals_off.push(*z_val as i32);
            image_resources_off.push((
                vbuf, ibuf, vp_bg_img, z_bg_img, tex_bg, params_bg, z_buf_img, params_buf, *clip,
            ));
        }
        (image_z_vals_off, image_resources_off)
    }

    pub(super) fn prep_svg_offscreen(
        &self,
        svg_views_off: &[SvgViewEntry<'_>],
    ) -> (Vec<i32>, Vec<ImageResource>) {
        let mut svg_z_vals_off: Vec<i32> = Vec::new();
        let mut svg_resources_off: Vec<ImageResource> = Vec::new();
        for (view_scaled, quad, z_val, opacity, clip, rounded_clip) in svg_views_off.iter() {
            let verts = [
                ImageQuadVtx {
                    pos: quad[0],
                    uv: [0.0, 0.0],
                },
                ImageQuadVtx {
                    pos: quad[1],
                    uv: [1.0, 0.0],
                },
                ImageQuadVtx {
                    pos: quad[2],
                    uv: [1.0, 1.0],
                },
                ImageQuadVtx {
                    pos: quad[3],
                    uv: [0.0, 1.0],
                },
            ];
            let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

            let vbuf = create_transient_upload(
                &self.device,
                "svg-vbuf-unified-offscreen",
                bytemuck::cast_slice(&verts),
                wgpu::BufferUsages::VERTEX,
            );
            let ibuf = create_transient_upload(
                &self.device,
                "svg-ibuf-unified-offscreen",
                bytemuck::cast_slice(&idx),
                wgpu::BufferUsages::INDEX,
            );

            let vp_bg_svg = self
                .image_offscreen
                .vp_bind_group(&self.device, &self.vp_buffer);
            // Pass z_index as float directly - shader will convert to depth
            let (z_bg_svg, z_buf_svg) = self.create_group_z_bind_group(*z_val as f32);
            let tex_bg = self
                .image_offscreen
                .tex_bind_group(&self.device, view_scaled);
            let (params_bg, params_buf) = self.image_offscreen.params_bind_group_clipped(
                &self.device,
                *opacity,
                true,
                true,
                *rounded_clip,
            );

            svg_z_vals_off.push(*z_val as i32);
            svg_resources_off.push((
                vbuf, ibuf, vp_bg_svg, z_bg_svg, tex_bg, params_bg, z_buf_svg, params_buf, *clip,
            ));
        }
        (svg_z_vals_off, svg_resources_off)
    }

    pub(super) fn prep_ext_offscreen(
        &mut self,
        external_texture_draws: &[crate::upload::ExtractedExternalTextureDraw],
        queue: &wgpu::Queue,
    ) -> (Vec<i32>, Vec<Arc<ExtResource>>) {
        let mut z_values = Vec::with_capacity(external_texture_draws.len());
        let mut resources = Vec::with_capacity(external_texture_draws.len());
        if external_texture_draws.is_empty() {
            return (z_values, resources);
        }
        let vertices =
            self.composite_offscreen
                .vertex_batch(&self.device, queue, external_texture_draws);
        for (index, draw) in external_texture_draws.iter().enumerate() {
            let Some(view) = self.external_textures.get(&draw.texture_id) else {
                continue;
            };
            z_values.push(draw.z);
            resources.push(self.composite_offscreen.prepare(
                &self.device,
                queue,
                &self.image_offscreen,
                &self.vp_buffer,
                self.solid_direct.z_index_bgl(),
                draw,
                view,
                self.linear_external_textures.contains(&draw.texture_id),
                &vertices,
                (index * 4) as i32,
            ));
        }
        (z_values, resources)
    }
}
