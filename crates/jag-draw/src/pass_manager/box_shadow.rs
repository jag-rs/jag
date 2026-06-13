//! `PassManager` methods (box_shadow). Verbatim extraction from the
//! former monolithic `pass_manager.rs`; no logic changed.

use super::PassManager;
use crate::scene::{BoxShadowSpec, RoundedRadii, RoundedRect};

impl PassManager {
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
}
