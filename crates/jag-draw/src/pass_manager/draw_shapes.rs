//! `PassManager` methods (draw_shapes). Verbatim extraction from the
//! former monolithic `pass_manager.rs`; no logic changed.

use super::PassManager;

impl PassManager {
    /// Render an image texture to the target at origin with size (in pixels, y-down).
    /// Expects `tex_view` to be created from an `Rgba8UnormSrgb` texture for proper sRGB sampling.
    pub fn draw_image_quad(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        origin: [f32; 2],
        size: [f32; 2],
        tex_view: &wgpu::TextureView,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) {
        // Update viewport uniform based on render target dimensions (+ logical pixel scale)
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

        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct QuadVtx {
            pos: [f32; 2],
            uv: [f32; 2],
        }
        let x = origin[0];
        let y = origin[1];
        let w = size[0].max(0.0);
        let h = size[1].max(0.0);
        let verts = [
            QuadVtx {
                pos: [x, y],
                uv: [0.0, 0.0],
            },
            QuadVtx {
                pos: [x + w, y],
                uv: [1.0, 0.0],
            },
            QuadVtx {
                pos: [x + w, y + h],
                uv: [1.0, 1.0],
            },
            QuadVtx {
                pos: [x, y + h],
                uv: [0.0, 1.0],
            },
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let vsize = (verts.len() * std::mem::size_of::<QuadVtx>()) as u64;
        let isize = (idx.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));
        }

        let vp_bg = self.image.vp_bind_group(&self.device, &self.vp_buffer);
        let z_bg = self.create_z_bind_group(0.0, queue);
        let tex_bg = self.image.tex_bind_group(&self.device, tex_view);
        let (params_bg, _params_buf) = self.image.params_bind_group(&self.device, 1.0, false);

        // Create depth texture for image rendering (1x)
        let depth_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image-depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let depth_attachment = Some(wgpu::RenderPassDepthStencilAttachment {
            view: &depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load, // Preserve existing depth values
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        });

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("image-pass"),
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
        self.image.record(
            &mut pass,
            &vp_bg,
            &z_bg,
            &tex_bg,
            &params_bg,
            &vbuf,
            &ibuf,
            idx.len() as u32,
        );
    }

    /// Draw a simple overlay rectangle that darkens existing content without affecting depth.
    /// This is intended for UI overlays like modal scrims that should blend over the scene
    /// but not participate in depth testing.
    pub fn draw_overlay_rect(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        rect: crate::scene::Rect,
        color: crate::scene::ColorLinPremul,
        queue: &wgpu::Queue,
    ) {
        // Update viewport uniform based on render target dimensions (+ logical pixel scale)
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
        struct OverlayVtx {
            pos: [f32; 2],
            color: [f32; 4],
            z_index: f32,
        }

        let overlay_color = [color.r, color.g, color.b, color.a];
        let z_index = 0.0f32;
        let x = rect.x;
        let y = rect.y;
        let w = rect.w.max(0.0);
        let h = rect.h.max(0.0);

        // Skip degenerate rectangles
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        let verts = [
            OverlayVtx {
                pos: [x, y],
                color: overlay_color,
                z_index,
            },
            OverlayVtx {
                pos: [x + w, y],
                color: overlay_color,
                z_index,
            },
            OverlayVtx {
                pos: [x + w, y + h],
                color: overlay_color,
                z_index,
            },
            OverlayVtx {
                pos: [x, y + h],
                color: overlay_color,
                z_index,
            },
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

        let vsize = (verts.len() * std::mem::size_of::<OverlayVtx>()) as u64;
        let isize = (idx.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay-rect-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay-rect-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));
        }

        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay-vp-bg"),
            layout: self.overlay_solid.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Overlay pass: no depth attachment so the quad simply blends over existing content.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay-rect-pass"),
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
        self.overlay_solid
            .record(&mut pass, &vp_bg, &vbuf, &ibuf, idx.len() as u32);
    }

    /// Draw a full-viewport scrim rectangle that blends over existing content.
    /// Unlike draw_overlay_rect, this uses a depth buffer attachment but with:
    /// - depth_write_enabled = false (doesn't affect depth buffer)
    /// - depth_compare = Always (always passes depth test)
    /// This allows the scrim to render over all existing content while letting
    /// subsequent draws at higher z-index render on top of the scrim.
    ///
    /// NOTE: Scrim renders directly to target without MSAA or depth attachment.
    /// The scrim pipeline uses depth_compare=Always and depth_write_enabled=false.
    pub fn draw_scrim_rect(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        rect: crate::scene::Rect,
        color: crate::scene::ColorLinPremul,
        queue: &wgpu::Queue,
    ) {
        // Update viewport uniform based on render target dimensions (+ logical pixel scale)
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
        struct ScrimVtx {
            pos: [f32; 2],
            color: [f32; 4],
            z_index: f32,
        }

        let scrim_color = [color.r, color.g, color.b, color.a];
        // Use a middle z-index - the scrim pipeline ignores depth testing anyway
        let z_index = 0.5f32;
        let x = rect.x;
        let y = rect.y;
        let w = rect.w.max(0.0);
        let h = rect.h.max(0.0);

        // Skip degenerate rectangles
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        let verts = [
            ScrimVtx {
                pos: [x, y],
                color: scrim_color,
                z_index,
            },
            ScrimVtx {
                pos: [x + w, y],
                color: scrim_color,
                z_index,
            },
            ScrimVtx {
                pos: [x + w, y + h],
                color: scrim_color,
                z_index,
            },
            ScrimVtx {
                pos: [x, y + h],
                color: scrim_color,
                z_index,
            },
        ];
        let idx: [u16; 6] = [0, 1, 2, 0, 2, 3];

        let vsize = (verts.len() * std::mem::size_of::<ScrimVtx>()) as u64;
        let isize = (idx.len() * std::mem::size_of::<u16>()) as u64;
        let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scrim-rect-vbuf"),
            size: vsize.max(4),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("scrim-rect-ibuf"),
            size: isize.max(4),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        if vsize > 0 {
            queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&verts));
        }
        if isize > 0 {
            queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&idx));
        }

        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("scrim-vp-bg"),
            layout: self.scrim_solid.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });

        // Scrim pass: no depth attachment. The scrim pipeline is configured with
        // depth_compare=Always and depth_write_enabled=false, so depth isn't needed.
        // It simply blends over existing content without affecting any depth state.
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scrim-rect-pass"),
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
        self.scrim_solid
            .record(&mut pass, &vp_bg, &vbuf, &ibuf, idx.len() as u32);
    }
}
