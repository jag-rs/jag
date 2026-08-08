//! Surface-owned texture operations backed by JAG's shared compatibility seam.

use jag_draw::{ExternalTextureId, PassManager, wgpu};

pub(crate) use jag_draw::gpu_texture::Texture2dSpec;

pub(crate) fn create_texture_2d(
    device: &wgpu::Device,
    label: &str,
    spec: Texture2dSpec,
) -> wgpu::Texture {
    jag_draw::gpu_texture::create_texture_2d(device, label, spec)
}

pub(crate) fn create_default_texture_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    jag_draw::gpu_texture::create_default_texture_view(texture)
}

pub(crate) fn upload_texture_2d(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    size: [u32; 2],
    bytes_per_row: u32,
    data: &[u8],
) {
    jag_draw::gpu_texture::upload_texture_2d(queue, texture, [0, 0], size, bytes_per_row, data);
}

pub(crate) fn copy_texture_to_buffer_2d(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    destination: &wgpu::Buffer,
    padded_bytes_per_row: u32,
    size: [u32; 2],
) {
    jag_draw::gpu_texture::copy_texture_to_buffer_2d(
        encoder,
        source,
        destination,
        padded_bytes_per_row,
        size,
    );
}

pub(crate) fn register_external_texture(
    pass: &mut PassManager,
    id: ExternalTextureId,
    view: wgpu::TextureView,
) {
    pass.register_external_texture(id, view);
}

#[cfg(test)]
mod tests {
    #[test]
    fn jag_surface_texture_operations_cannot_bypass_surface_seam() {
        let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut pending = vec![source_root];
        let mut checked_files = 0;
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).expect("read jag-surface source directory") {
                let path = entry.expect("read jag-surface source entry").path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                if path.extension().and_then(|value| value.to_str()) != Some("rs")
                    || path.file_name().and_then(|value| value.to_str()) == Some("gpu_texture.rs")
                {
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("read jag-surface source");
                for prohibited in [
                    ".create_texture(",
                    ".create_view(",
                    ".write_texture(",
                    "copy_texture_to_buffer(",
                    ".register_external_texture(",
                ] {
                    assert!(
                        !source.contains(prohibited),
                        "{} bypasses the surface texture seam with {prohibited}",
                        path.display()
                    );
                }
                checked_files += 1;
            }
        }
        assert!(checked_files > 20, "jag-surface source scan was incomplete");
    }
}
