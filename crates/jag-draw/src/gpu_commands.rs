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
            multiview_mask: None,
        };
        self.begin_render_pass(&descriptor)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn jag_draw_cannot_bypass_render_pass_seam() {
        fn rust_sources(root: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(root).expect("read jag-draw source directory") {
                let path = entry.expect("read jag-draw source entry").path();
                if path.is_dir() {
                    rust_sources(&path, files);
                } else if path.extension().and_then(|value| value.to_str()) == Some("rs") {
                    files.push(path);
                }
            }
        }

        let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let seam_path = source_root.join("gpu_commands.rs");
        let mut sources = Vec::new();
        rust_sources(&source_root, &mut sources);
        let mut checked_files = 0;
        for path in sources {
            if path == seam_path {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read jag-draw source");
            assert!(
                !source.contains(".begin_render_pass("),
                "{} bypasses the command-recording seam",
                path.display()
            );
            checked_files += 1;
        }
        assert!(checked_files > 40, "recursive jag-draw scan was incomplete");
    }
}
