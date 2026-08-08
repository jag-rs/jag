//! Version-insulated creation, view, and upload operations for 2D textures.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Texture2dSpec {
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub usage: wgpu::TextureUsages,
}

impl Texture2dSpec {
    fn extent(self) -> wgpu::Extent3d {
        wgpu::Extent3d {
            width: self.width,
            height: self.height,
            depth_or_array_layers: 1,
        }
    }
}

pub(crate) fn create_texture_2d(
    device: &wgpu::Device,
    label: &str,
    spec: Texture2dSpec,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: spec.extent(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: spec.format,
        usage: spec.usage,
        view_formats: &[],
    })
}

pub(crate) fn create_default_texture_view(texture: &wgpu::Texture) -> wgpu::TextureView {
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

pub(crate) fn upload_texture_2d(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    origin: [u32; 2],
    size: [u32; 2],
    bytes_per_row: u32,
    data: &[u8],
) {
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        data,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(bytes_per_row),
            rows_per_image: Some(size[1]),
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

pub(crate) fn copy_texture_2d(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    destination: &wgpu::Texture,
    size: [u32; 2],
) {
    encoder.copy_texture_to_texture(
        wgpu::ImageCopyTexture {
            texture: source,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyTexture {
            texture: destination,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_spec_maps_to_single_layer_2d_extent() {
        let spec = Texture2dSpec {
            width: 37,
            height: 19,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
        };

        assert_eq!(
            spec.extent(),
            wgpu::Extent3d {
                width: 37,
                height: 19,
                depth_or_array_layers: 1,
            }
        );
    }

    #[test]
    fn jag_draw_texture_operations_cannot_bypass_resource_seam() {
        let source_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut pending = vec![source_root];
        let mut checked_files = 0;
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).expect("read jag-draw source directory") {
                let path = entry.expect("read jag-draw source entry").path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                if path.extension().and_then(|value| value.to_str()) != Some("rs")
                    || path.file_name().and_then(|value| value.to_str()) == Some("gpu_texture.rs")
                {
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("read jag-draw source");
                for prohibited in [
                    ".create_texture(",
                    ".create_view(",
                    ".write_texture(",
                    "copy_texture_to_texture(",
                ] {
                    assert!(
                        !source.contains(prohibited),
                        "{} bypasses the texture-resource seam with {prohibited}",
                        path.display()
                    );
                }
                checked_files += 1;
            }
        }
        assert!(checked_files > 40, "jag-draw source scan was incomplete");
    }
}
