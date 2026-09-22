//! Offscreen analytic box shadows on a gamma-blend target.

use super::{PassManager, PassTargets};
use wgpu::util::DeviceExt;

impl PassManager {
    /// Draw `shadow_instances` over the opaque solids with hardware blending.
    /// Only for gamma-blend targets, where that blending is already sRGB.
    pub(super) fn record_blended_shadows(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &PassTargets,
    ) {
        let shadow_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shadow-instances"),
                contents: bytemuck::cast_slice(&self.shadow_instances),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let vp_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow-vp-bg-offscreen"),
            layout: self.shadow_offscreen.viewport_bgl(),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("unified-offscreen-shadow-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &targets.color.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.depth_view(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.shadow_offscreen.record(
            &mut pass,
            &vp_bg,
            &shadow_buf,
            self.shadow_instances.len() as u32,
        );
    }
}
