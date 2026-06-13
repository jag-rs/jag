//! `PassManager` methods (paint_root). Verbatim extraction from the
//! former monolithic `pass_manager.rs`; no logic changed.

use super::{Background, PassManager, PassTargets};
use crate::upload::GpuScene;

impl PassManager {
    pub fn render_solids_to_offscreen(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        vp_bg: &wgpu::BindGroup,
        targets: &PassTargets,
        scene: &GpuScene,
        clear_color: wgpu::Color,
        queue: &wgpu::Queue,
    ) {
        // Depth attachment for offscreen rendering (1x)
        let depth_tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("solid-depth-offscreen"),
            size: wgpu::Extent3d {
                width: targets.color.key.width,
                height: targets.color.key.height,
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

        let _z_bg = self.create_z_bind_group(0.0, queue);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("solid-offscreen-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.color.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.solid_offscreen.record(&mut pass, vp_bg, scene);
    }

    pub fn composite_to_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        offscreen: &PassTargets,
        clear: Option<wgpu::Color>,
    ) {
        let bg = self
            .compositor
            .bind_group(&self.device, &offscreen.color.view);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: match clear {
                        Some(c) => wgpu::LoadOp::Clear(c),
                        None => wgpu::LoadOp::Load,
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.compositor.record(&mut pass, &bg);
    }

    /// Paint background to intermediate texture instead of directly to surface.
    /// This enables smooth resizing when combined with blit_to_surface.
    pub fn paint_root_to_intermediate(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bg: &Background,
        queue: &wgpu::Queue,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before painting");
        self.paint_root(encoder, &intermediate.view, bg, queue);
    }

    pub fn paint_root(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        bg: &Background,
        queue: &wgpu::Queue,
    ) {
        // If solid, do a minimal clear pass
        if let Background::Solid(c) = bg {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bg-solid-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: c.r as f64,
                            g: c.g as f64,
                            b: c.b as f64,
                            a: c.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            return;
        }

        // For gradient, draw fullscreen triangle
        let (start_uv, end_uv, stop0, stop1) = match bg {
            Background::LinearGradient {
                start_uv,
                end_uv,
                stop0,
                stop1,
            } => (*start_uv, *end_uv, *stop0, *stop1),
            _ => unreachable!(),
        };
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgParams {
            start: [f32; 2],
            end: [f32; 2],
            center: [f32; 2],
            radius: f32,
            stop_count: u32,
            mode: u32,
            _pad: u32,
        }
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }

        let params = BgParams {
            start: start_uv,
            end: end_uv,
            center: [0.5, 0.5],
            radius: 1.0,
            stop_count: 2,
            mode: 1,
            _pad: 0,
        };
        let c0 = stop0.1;
        let c1 = stop1.1;
        let stops = [
            Stop {
                pos: stop0.0,
                _pad0: [0.0; 3],
                color: [c0.r, c0.g, c0.b, c0.a],
            },
            Stop {
                pos: stop1.0,
                _pad0: [0.0; 3],
                color: [c1.r, c1.g, c1.b, c1.a],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
            Stop {
                pos: 0.0,
                _pad0: [0.0; 3],
                color: [0.0; 4],
            },
        ];

        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind"),
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
            label: Some("bg-grad-pass"),
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

    /// Convenience: paint a solid background color directly to the surface.
    pub fn paint_root_color(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        color: crate::scene::ColorLinPremul,
        queue: &wgpu::Queue,
    ) {
        // Draw solid via the background fullscreen shader to avoid sRGB clear vs blit inconsistencies.
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
        let params = BgParams {
            start_end: [0.0, 0.0, 1.0, 1.0],
            center_radius_stop: [0.5, 0.5, 1.0, 1.0],
            flags: [0.0, 0.0, 0.0, 0.0], // mode = 0 => solid
        };
        let stops: [Stop; 1] = [Stop {
            pos: 0.0,
            _pad0: [0.0; 3],
            color: [color.r, color.g, color.b, color.a],
        }];
        // Write uniforms (only first stop used for solid mode)
        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind-solid"),
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
            label: Some("bg-solid-pass"),
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

    /// Convenience: paint a simple 2-stop linear gradient to the surface.
    pub fn paint_root_gradient(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
        start_uv: [f32; 2],
        end_uv: [f32; 2],
        stop0: (f32, crate::scene::ColorLinPremul),
        stop1: (f32, crate::scene::ColorLinPremul),
        queue: &wgpu::Queue,
    ) {
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct BgData {
            start_end: [f32; 4],
            center_radius_stop: [f32; 4],
            flags: [f32; 4],
        }
        let c0 = stop0.1;
        let c1 = stop1.1;
        // Reuse the multi-stop layout by writing two stops into the stops buffer
        let debug_flag = std::env::var("DEBUG_RADIAL")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let params = BgData {
            start_end: [start_uv[0], start_uv[1], end_uv[0], end_uv[1]],
            center_radius_stop: [0.5, 0.5, 1.0, 2.0],
            flags: [1.0, if debug_flag { 1.0 } else { 0.0 }, 0.0, 0.0],
        };
        // Populate first two stops in the stop buffer for the simple gradient helper
        #[repr(C)]
        #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
        struct Stop {
            pos: f32,
            _pad0: [f32; 3],
            color: [f32; 4],
        }
        let stops: [Stop; 2] = [
            Stop {
                pos: stop0.0,
                _pad0: [0.0; 3],
                color: [c0.r, c0.g, c0.b, c0.a],
            },
            Stop {
                pos: stop1.0,
                _pad0: [0.0; 3],
                color: [c1.r, c1.g, c1.b, c1.a],
            },
        ];
        queue.write_buffer(&self.bg_param_buffer, 0, bytemuck::bytes_of(&params));
        queue.write_buffer(&self.bg_stops_buffer, 0, bytemuck::cast_slice(&stops));
        let bg_bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bg-bind"),
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
            label: Some("bg-grad-pass"),
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
