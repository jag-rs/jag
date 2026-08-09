//! Version-insulated render-pipeline creation.

pub(crate) struct VertexState<'a> {
    pub(crate) module: &'a wgpu::ShaderModule,
    pub(crate) entry_point: &'a str,
    pub(crate) buffers: &'a [wgpu::VertexBufferLayout<'a>],
}

pub(crate) struct FragmentState<'a> {
    pub(crate) module: &'a wgpu::ShaderModule,
    pub(crate) entry_point: &'a str,
    pub(crate) targets: &'a [Option<wgpu::ColorTargetState>],
}

pub(crate) struct RenderPipelineDescriptor<'a> {
    pub(crate) label: Option<&'a str>,
    pub(crate) layout: Option<&'a wgpu::PipelineLayout>,
    pub(crate) vertex: VertexState<'a>,
    pub(crate) fragment: Option<FragmentState<'a>>,
    pub(crate) primitive: wgpu::PrimitiveState,
    pub(crate) depth_stencil: Option<wgpu::DepthStencilState>,
    pub(crate) multisample: wgpu::MultisampleState,
    pub(crate) multiview: Option<std::num::NonZeroU32>,
}

pub(crate) fn create_render_pipeline(
    device: &wgpu::Device,
    descriptor: RenderPipelineDescriptor<'_>,
) -> wgpu::RenderPipeline {
    let buffers: Vec<_> = descriptor
        .vertex
        .buffers
        .iter()
        .cloned()
        .map(Some)
        .collect();
    let vertex = wgpu::VertexState {
        module: descriptor.vertex.module,
        entry_point: Some(descriptor.vertex.entry_point),
        buffers: &buffers,
        compilation_options: Default::default(),
    };
    let fragment = descriptor.fragment.map(|state| wgpu::FragmentState {
        module: state.module,
        entry_point: Some(state.entry_point),
        targets: state.targets,
        compilation_options: Default::default(),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: descriptor.label,
        layout: descriptor.layout,
        vertex,
        fragment,
        primitive: descriptor.primitive,
        depth_stencil: descriptor.depth_stencil,
        multisample: descriptor.multisample,
        multiview_mask: descriptor.multiview,
        cache: None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn jag_draw_cannot_bypass_pipeline_creation_seam() {
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
        let seam_path = source_root.join("gpu_pipeline.rs");
        let mut sources = Vec::new();
        rust_sources(&source_root, &mut sources);
        let mut checked_files = 0;
        for path in sources {
            if path == seam_path {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read jag-draw source");
            for prohibited in [".create_render_pipeline(", ".create_compute_pipeline("] {
                assert!(
                    !source.contains(prohibited),
                    "{} bypasses the pipeline seam with {prohibited}",
                    path.display()
                );
            }
            checked_files += 1;
        }
        assert!(checked_files > 40, "recursive jag-draw scan was incomplete");
    }
}
