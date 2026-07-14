//! `PassManager` methods (scrim_cutout). Verbatim extraction from the
//! former monolithic `pass_manager.rs`; no logic changed.

use super::{PassManager, apply_transform_to_point, set_scissor_for_clip};
use crate::allocator::{RenderAllocator, TexKey};
use crate::scene::RoundedRect;
use wgpu::util::DeviceExt;

impl PassManager {
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

    /// Draw a backdrop blur rectangle by sampling the current target contents and
    /// writing the filtered result back through the existing depth buffer.
    pub fn draw_backdrop_blur_rect(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        allocator: &mut RenderAllocator,
        target: &crate::OwnedTexture,
        width: u32,
        height: u32,
        draw: &crate::BackdropBlurDraw,
        queue: &wgpu::Queue,
    ) {
        if draw.rect.w <= 0.0 || draw.rect.h <= 0.0 || draw.effects.is_empty() {
            return;
        }

        let snapshot = allocator.allocate_texture(TexKey {
            width: width.max(1),
            height: height.max(1),
            format: target.key.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        });
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: &target.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
                texture: &snapshot.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
        );

        let mut filtered_views = Vec::with_capacity(draw.effects.len());
        for effect in &draw.effects {
            let input = filtered_views.last().unwrap_or(&snapshot.view);
            let output = match effect.clone() {
                crate::FilterEffect::Blur(radius) if radius > 0.0 => {
                    self.blur_surface(encoder, input, width, height, radius)
                }
                crate::FilterEffect::Blur(_) => continue,
                crate::FilterEffect::ColorMatrix(matrix) => {
                    self.color_filter_surface(encoder, input, width, height, matrix)
                }
                crate::FilterEffect::DropShadow(shadow) => {
                    self.drop_shadow_surface(encoder, input, width, height, shadow)
                }
                crate::FilterEffect::Mask(mask) => {
                    let Ok(output) = self.mask_surface(
                        encoder,
                        input,
                        width,
                        height,
                        [0.0, 0.0],
                        [width as f32, height as f32],
                        mask,
                    ) else {
                        allocator.release_texture(snapshot);
                        return;
                    };
                    output
                }
                crate::FilterEffect::MaskGroup(group) => {
                    let Ok(output) = self.mask_group_surface(
                        encoder,
                        input,
                        width,
                        height,
                        [0.0, 0.0],
                        [width as f32, height as f32],
                        &group,
                    ) else {
                        allocator.release_texture(snapshot);
                        return;
                    };
                    output
                }
            };
            filtered_views.push(output);
        }
        let filtered = filtered_views.last().unwrap_or(&snapshot.view);

        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            texel: [f32; 2],
            viewport_size: [f32; 2],
            radius: f32,
            logical: f32,
            pad: [f32; 2],
        }
        let params = Params {
            texel: [1.0 / width.max(1) as f32, 1.0 / height.max(1) as f32],
            viewport_size: [width.max(1) as f32, height.max(1) as f32],
            radius: 0.0,
            logical,
            pad: [0.0, 0.0],
        };
        queue.write_buffer(
            &self.backdrop_blur.param_buffer,
            0,
            bytemuck::bytes_of(&params),
        );

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BackdropVtx {
            pos: [f32; 2],
            z: f32,
        }
        let x = draw.rect.x;
        let y = draw.rect.y;
        let w = draw.rect.w;
        let h = draw.rect.h;
        let z = draw.z as f32;
        let p0 = apply_transform_to_point([x, y], draw.transform);
        let p1 = apply_transform_to_point([x + w, y], draw.transform);
        let p2 = apply_transform_to_point([x + w, y + h], draw.transform);
        let p3 = apply_transform_to_point([x, y + h], draw.transform);
        let verts = [
            BackdropVtx { pos: p0, z },
            BackdropVtx { pos: p1, z },
            BackdropVtx { pos: p2, z },
            BackdropVtx { pos: p3, z },
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let vbuf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("backdrop-blur-vbuf"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let ibuf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("backdrop-blur-ibuf"),
                contents: bytemuck::cast_slice(&idx),
                usage: wgpu::BufferUsages::INDEX,
            });

        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("backdrop-blur-vp-bg"),
            layout: self.backdrop_blur.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });
        let blur_bg = self.backdrop_blur.bind_group(&self.device, filtered);

        let depth_attachment = Some(wgpu::RenderPassDepthStencilAttachment {
            view: self.depth_view(),
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("backdrop-blur-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.view,
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
        if let Some(c) = draw.clip {
            if !set_scissor_for_clip(&mut pass, c, width, height) {
                drop(pass);
                allocator.release_texture(snapshot);
                return;
            }
        }
        self.backdrop_blur
            .record(&mut pass, &vp_bg, &blur_bg, &vbuf, &ibuf, idx.len() as u32);
        drop(pass);
        allocator.release_texture(snapshot);
    }
}
