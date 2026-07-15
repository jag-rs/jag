use crate::canvas::{GeneratedMaskTexture, UrlMaskTexture};

use super::JagSurface;

impl JagSurface {
    pub(super) fn register_generated_mask_textures(&mut self, masks: &[GeneratedMaskTexture]) {
        for mask in masks {
            let texture = self
                .device
                .create_texture(&jag_draw::wgpu::TextureDescriptor {
                    label: Some("generated-css-mask"),
                    size: jag_draw::wgpu::Extent3d {
                        width: mask.width,
                        height: mask.height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: jag_draw::wgpu::TextureDimension::D2,
                    format: jag_draw::wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: jag_draw::wgpu::TextureUsages::TEXTURE_BINDING
                        | jag_draw::wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
            self.queue.write_texture(
                jag_draw::wgpu::ImageCopyTexture {
                    texture: &texture,
                    mip_level: 0,
                    origin: jag_draw::wgpu::Origin3d::ZERO,
                    aspect: jag_draw::wgpu::TextureAspect::All,
                },
                &mask.pixels,
                jag_draw::wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(mask.width * 4),
                    rows_per_image: Some(mask.height),
                },
                jag_draw::wgpu::Extent3d {
                    width: mask.width,
                    height: mask.height,
                    depth_or_array_layers: 1,
                },
            );
            self.pass.register_external_texture(
                mask.id,
                texture.create_view(&jag_draw::wgpu::TextureViewDescriptor::default()),
            );
        }
    }

    pub(super) fn register_url_mask_textures(&mut self, masks: &[UrlMaskTexture]) {
        for mask in masks {
            let path = crate::resolve_asset_path(&mask.path);
            let view = if let Some((view, _, _)) = self.pass.try_get_image_view(&path) {
                view
            } else {
                self.pending_image_loads |= self.pass.request_image_load(&path);
                self.transparent_mask_view()
            };
            self.pass.register_external_texture(mask.id, view);
        }
    }

    fn transparent_mask_view(&self) -> jag_draw::wgpu::TextureView {
        let texture = self
            .device
            .create_texture(&jag_draw::wgpu::TextureDescriptor {
                label: Some("pending-url-mask"),
                size: jag_draw::wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: jag_draw::wgpu::TextureDimension::D2,
                format: jag_draw::wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: jag_draw::wgpu::TextureUsages::TEXTURE_BINDING
                    | jag_draw::wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
        self.queue.write_texture(
            jag_draw::wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: jag_draw::wgpu::Origin3d::ZERO,
                aspect: jag_draw::wgpu::TextureAspect::All,
            },
            &[0; 4],
            jag_draw::wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            jag_draw::wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        texture.create_view(&jag_draw::wgpu::TextureViewDescriptor::default())
    }
}
