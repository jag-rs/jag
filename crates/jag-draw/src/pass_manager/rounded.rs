//! `PassManager` methods (rounded). Verbatim extraction from the
//! former monolithic `pass_manager.rs`; no logic changed.

use super::PassManager;
use crate::allocator::RenderAllocator;
use crate::scene::RoundedRect;

impl PassManager {
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
}
