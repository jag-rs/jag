//! Version-insulated shader and binding-resource creation operations.

pub(crate) fn create_wgsl_shader(
    device: &wgpu::Device,
    label: &str,
    source: &'static str,
) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

pub(crate) fn create_pipeline_layout(
    device: &wgpu::Device,
    label: &str,
    bind_group_layouts: &[&wgpu::BindGroupLayout],
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts,
        push_constant_ranges: &[],
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn pipeline_shader_and_layout_owners_cannot_bypass_binding_seam() {
        let pipeline_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/pipeline");
        let mut checked_files = 0;
        for entry in std::fs::read_dir(pipeline_root).expect("read jag-draw pipeline directory") {
            let path = entry.expect("read jag-draw pipeline entry").path();
            if path.extension().and_then(|value| value.to_str()) != Some("rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read jag-draw pipeline source");
            for prohibited in [".create_shader_module(", ".create_pipeline_layout("] {
                assert!(
                    !source.contains(prohibited),
                    "{} bypasses the binding-resource seam with {prohibited}",
                    path.display()
                );
            }
            checked_files += 1;
        }
        assert!(checked_files > 10, "jag-draw pipeline scan was incomplete");
    }
}
