//! Offscreen analytic box shadows on a gamma-blend target.

use super::{PassManager, PassTargets};
use crate::gpu_commands::CommandEncoderExt as _;

impl PassManager {
    /// Draw `shadow_instances` over the opaque solids with hardware blending.
    /// Only for gamma-blend targets, where that blending is already sRGB.
    pub(super) fn record_blended_shadows(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        targets: &PassTargets,
    ) {
        let shadow_buf = crate::gpu_transfer::create_transient_upload(
            &self.device,
            "shadow-instances",
            bytemuck::cast_slice(&self.shadow_instances),
            wgpu::BufferUsages::VERTEX,
        );
        let vp_bg = crate::gpu_bindings::create_bind_group(
            &self.device,
            "shadow-vp-bg-offscreen",
            self.shadow_offscreen.viewport_bgl(),
            &[wgpu::BindGroupEntry {
                binding: 0,
                resource: self.vp_buffer.as_entire_binding(),
            }],
        );
        let mut pass =
            encoder.begin_semantic_render_pass(crate::gpu_commands::RenderPassDescriptor {
                label: Some("unified-offscreen-shadow-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.color.view,
                    depth_slice: None,
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
