//! `PassManager` methods (paint_root_gradients). Verbatim extraction from the
//! former monolithic `pass_manager.rs`; no logic changed.

use super::PassManager;

impl PassManager {
    /// Paint linear gradient to intermediate texture.
    pub fn paint_root_linear_gradient_multi_to_intermediate(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        start_uv: [f32; 2],
        end_uv: [f32; 2],
        stops_in: &[(f32, crate::scene::ColorLinPremul)],
        queue: &wgpu::Queue,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before painting");
        self.paint_root_linear_gradient_multi(
            encoder,
            &intermediate.view,
            start_uv,
            end_uv,
            stops_in,
            queue,
        );
    }

    /// Multi-stop linear gradient background
    pub fn paint_root_linear_gradient_multi(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        start_uv: [f32; 2],
        end_uv: [f32; 2],
        stops_in: &[(f32, crate::scene::ColorLinPremul)],
        queue: &wgpu::Queue,
    ) {
        // Normalize and sort stops for deterministic evaluation
        let mut sorted: Vec<(f32, crate::scene::ColorLinPremul)> = stops_in
            .iter()
            .map(|(p, c)| (p.clamp(0.0, 1.0), *c))
            .collect();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let count = sorted.len().min(8).max(2) as u32;
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgParams {
            start_end: [f32; 4],
            center_radius_stop: [f32; 4],
            flags: [f32; 4],
        }
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }
        let mut stops: [Stop; 8] = [Stop {
            pos: 0.0,
            _pad0: [0.0; 3],
            color: [0.0; 4],
        }; 8];
        for (i, (p, c)) in sorted.iter().take(8).enumerate() {
            stops[i] = Stop {
                pos: *p,
                _pad0: [0.0; 3],
                color: [c.r, c.g, c.b, c.a],
            };
        }
        let debug_flag = std::env::var("DEBUG_RADIAL")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let params = BgParams {
            start_end: [start_uv[0], start_uv[1], end_uv[0], end_uv[1]],
            center_radius_stop: [0.5, 0.5, 1.0, count as f32],
            flags: [1.0, if debug_flag { 1.0 } else { 0.0 }, 0.0, 0.0],
        };
        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind-linear"),
            layout: self.bg.bgl(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.bg_param_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bg_stops_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bg-linear-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
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
        self.bg.record(&mut pass, &bg_bind);
    }

    /// Paint radial gradient to intermediate texture.
    pub fn paint_root_radial_gradient_multi_to_intermediate(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        center_uv: [f32; 2],
        radius: f32,
        stops_in: &[(f32, crate::scene::ColorLinPremul)],
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before painting");
        self.paint_root_radial_gradient_multi(
            encoder,
            &intermediate.view,
            center_uv,
            radius,
            stops_in,
            queue,
            width,
            height,
        );
    }

    /// Multi-stop radial gradient background
    pub fn paint_root_radial_gradient_multi(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        center_uv: [f32; 2],
        radius: f32,
        stops_in: &[(f32, crate::scene::ColorLinPremul)],
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
    ) {
        // Normalize and sort stops for deterministic evaluation
        let mut sorted: Vec<(f32, crate::scene::ColorLinPremul)> = stops_in
            .iter()
            .map(|(p, c)| (p.clamp(0.0, 1.0), *c))
            .collect();
        sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let count = sorted.len().min(8).max(2) as u32;
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgParams {
            start_end: [f32; 4],
            center_radius_stop: [f32; 4],
            flags: [f32; 4],
        }
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }
        let mut stops: [Stop; 8] = [Stop {
            pos: 0.0,
            _pad0: [0.0; 3],
            color: [0.0; 4],
        }; 8];
        for (i, (p, c)) in sorted.iter().take(8).enumerate() {
            stops[i] = Stop {
                pos: *p,
                _pad0: [0.0; 3],
                color: [c.r, c.g, c.b, c.a],
            };
        }
        let debug_flag = std::env::var("DEBUG_RADIAL")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let aspect_ratio = (width.max(1) as f32) / (height.max(1) as f32);
        if debug_flag {
            // debug logging removed
        }
        // macOS-specific DPI correction: Only adjust for centered fullscreen radials.
        // When center ~ [0.5,0.5], divide center and radius by scale factor to correct
        // for retina scaling differences in UV sampling. No-op elsewhere.
        let mut adj_center = center_uv;
        let mut adj_radius = radius;
        #[cfg(target_os = "macos")]
        {
            let sf = self.scale_factor.max(1.0);
            // Within ~1e-3 of exact center counts as centered
            if (adj_center[0] - 0.5).abs() < 1e-3 && (adj_center[1] - 0.5).abs() < 1e-3 {
                adj_center = [adj_center[0] / sf, adj_center[1] / sf];
                adj_radius = adj_radius / sf;
                if debug_flag {
                    // debug logging removed
                }
            }
        }
        let params = BgParams {
            start_end: [0.0, 0.0, 1.0, 1.0],
            center_radius_stop: [adj_center[0], adj_center[1], adj_radius, count as f32],
            flags: [2.0, if debug_flag { 1.0 } else { 0.0 }, aspect_ratio, 0.0],
        };
        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind-radial"),
            layout: self.bg.bgl(),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.bg_param_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.bg_stops_buffer.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bg-radial-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
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
        self.bg.record(&mut pass, &bg_bind);
    }
}
