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
    fn migrated_texture_owners_cannot_bypass_resource_seam() {
        for (name, source) in [
            ("allocator.rs", include_str!("allocator.rs")),
            ("image_cache.rs", include_str!("image_cache.rs")),
            ("svg.rs", include_str!("svg.rs")),
            (
                "pass_manager/text_prep.rs",
                include_str!("pass_manager/text_prep.rs"),
            ),
        ] {
            for prohibited in [".create_texture(", ".create_view(", ".write_texture("] {
                assert!(
                    !source.contains(prohibited),
                    "{name} bypasses the texture-resource seam with {prohibited}"
                );
            }
        }

        let setup = include_str!("pass_manager/setup.rs");
        for prohibited in [".create_texture(", ".write_texture("] {
            assert!(
                !setup.contains(prohibited),
                "pass_manager/setup.rs bypasses text-atlas ownership with {prohibited}"
            );
        }
    }
}
