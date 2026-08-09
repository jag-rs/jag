//! Version-insulated render-pass recording operations.

pub(crate) struct RenderPassDescriptor<'tex, 'desc> {
    pub(crate) label: Option<&'desc str>,
    pub(crate) color_attachments: &'desc [Option<wgpu::RenderPassColorAttachment<'tex>>],
    pub(crate) depth_stencil_attachment: Option<wgpu::RenderPassDepthStencilAttachment<'tex>>,
    pub(crate) timestamp_writes: Option<wgpu::RenderPassTimestampWrites<'desc>>,
    pub(crate) occlusion_query_set: Option<&'tex wgpu::QuerySet>,
}

pub(crate) trait CommandEncoderExt {
    fn begin_semantic_render_pass<'pass, 'desc>(
        &'pass mut self,
        descriptor: RenderPassDescriptor<'pass, 'desc>,
    ) -> wgpu::RenderPass<'pass>;
}

impl CommandEncoderExt for wgpu::CommandEncoder {
    fn begin_semantic_render_pass<'pass, 'desc>(
        &'pass mut self,
        descriptor: RenderPassDescriptor<'pass, 'desc>,
    ) -> wgpu::RenderPass<'pass> {
        let descriptor = wgpu::RenderPassDescriptor {
            label: descriptor.label,
            color_attachments: descriptor.color_attachments,
            depth_stencil_attachment: descriptor.depth_stencil_attachment,
            timestamp_writes: descriptor.timestamp_writes,
            occlusion_query_set: descriptor.occlusion_query_set,
        };
        self.begin_render_pass(&descriptor)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn migrated_render_pass_owners_cannot_bypass_command_seam() {
        let pass_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pass_manager");
        for file in [
            "paint_root.rs",
            "paint_root_gradients.rs",
            "render_direct.rs",
            "render_offscreen.rs",
            "targets.rs",
        ] {
            let path = pass_root.join(file);
            let source = std::fs::read_to_string(&path).expect("read jag-draw source");
            assert!(
                !source.contains(".begin_render_pass("),
                "{} bypasses the command-recording seam",
                path.display()
            );
        }
    }
}
