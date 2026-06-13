//! Image/SVG/external-texture GPU resource preparation for `render_unified`.
//!
//! Verbatim extractions of the per-quad resource-build loops that previously
//! lived inline in `render_unified`. They create all buffers and bind groups
//! (fully owned values) before the render pass so the resources outlive the
//! pass. No logic changed during extraction.

use super::{ImageQuadVtx, PassManager};

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

type ExtResource = (
    wgpu::Buffer,
    wgpu::Buffer,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::Buffer,
    wgpu::Buffer,
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
        queue: &wgpu::Queue,
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
                self.image
                    .params_bind_group_clipped(&self.device, *opacity, false, *rounded_clip);

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
        queue: &wgpu::Queue,
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
                self.image
                    .params_bind_group_clipped(&self.device, *opacity, false, *rounded_clip);

            svg_z_vals.push(*z_val as i32);
            svg_resources.push((
                vbuf, ibuf, vp_bg_svg, z_bg_svg, tex_bg, params_bg, z_buf_svg, params_buf, *clip,
            ));
        }
        (svg_z_vals, svg_resources)
    }

    pub(super) fn prep_ext_direct(
        &self,
        external_texture_draws: &[crate::upload::ExtractedExternalTextureDraw],
        queue: &wgpu::Queue,
    ) -> (Vec<i32>, Vec<ExtResource>) {
        let mut ext_z_vals: Vec<i32> = Vec::new();
        let mut ext_resources: Vec<ExtResource> = Vec::new();
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
        (ext_z_vals, ext_resources)
    }

    pub(super) fn prep_image_offscreen(
        &self,
        image_views_off: &[ImageViewEntry<'_>],
        queue: &wgpu::Queue,
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
            let (params_bg, params_buf) = self.image_offscreen.params_bind_group_clipped(
                &self.device,
                *opacity,
                false,
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
        queue: &wgpu::Queue,
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
            let (params_bg, params_buf) = self.image_offscreen.params_bind_group_clipped(
                &self.device,
                *opacity,
                false,
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
        &self,
        external_texture_draws: &[crate::upload::ExtractedExternalTextureDraw],
        queue: &wgpu::Queue,
    ) -> (Vec<i32>, Vec<ExtResource>) {
        let mut ext_z_vals_off: Vec<i32> = Vec::new();
        let mut ext_resources_off: Vec<ExtResource> = Vec::new();
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
        (ext_z_vals_off, ext_resources_off)
    }
}
