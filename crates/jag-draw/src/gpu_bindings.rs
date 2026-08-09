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

pub(crate) fn create_bind_group_layout(
    device: &wgpu::Device,
    label: &str,
    entries: &[wgpu::BindGroupLayoutEntry],
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries,
    })
}

pub(crate) fn create_bind_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    entries: &[wgpu::BindGroupEntry<'_>],
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn jag_draw_cannot_bypass_binding_resource_seam() {
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
        let seam_path = source_root.join("gpu_bindings.rs");
        let mut sources = Vec::new();
        rust_sources(&source_root, &mut sources);
        let mut checked_files = 0;
        for path in sources {
            if path == seam_path {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read jag-draw source");
            for prohibited in [
                ".create_shader_module(",
                ".create_pipeline_layout(",
                ".create_bind_group_layout(",
                ".create_bind_group(",
            ] {
                assert!(
                    !source.contains(prohibited),
                    "{} bypasses the binding-resource seam with {prohibited}",
                    path.display()
                );
            }
            checked_files += 1;
        }
        assert!(checked_files > 40, "recursive jag-draw scan was incomplete");
    }
}
