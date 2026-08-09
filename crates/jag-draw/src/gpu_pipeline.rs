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
    let vertex = wgpu::VertexState {
        module: descriptor.vertex.module,
        entry_point: descriptor.vertex.entry_point,
        buffers: descriptor.vertex.buffers,
    };
    let fragment = descriptor.fragment.map(|state| wgpu::FragmentState {
        module: state.module,
        entry_point: state.entry_point,
        targets: state.targets,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: descriptor.label,
        layout: descriptor.layout,
        vertex,
        fragment,
        primitive: descriptor.primitive,
        depth_stencil: descriptor.depth_stencil,
        multisample: descriptor.multisample,
        multiview: descriptor.multiview,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn migrated_pipeline_owners_cannot_bypass_pipeline_seam() {
        let pipeline_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pipeline");
        for file in [
            "background_blur.rs",
            "color_filter.rs",
            "composite.rs",
            "drop_shadow_filter.rs",
            "mask_filter.rs",
            "scrim_stencil.rs",
            "shadow.rs",
            "shadow_composite_instance.rs",
            "smaa.rs",
            "text_image.rs",
        ] {
            let path = pipeline_root.join(file);
            let source = std::fs::read_to_string(&path).expect("read jag-draw source");
            for prohibited in [".create_render_pipeline(", ".create_compute_pipeline("] {
                assert!(
                    !source.contains(prohibited),
                    "{} bypasses the pipeline seam with {prohibited}",
                    path.display()
                );
            }
        }
    }
}
