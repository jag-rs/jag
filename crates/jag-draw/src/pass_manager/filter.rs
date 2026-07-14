use super::PassManager;
use anyhow::{Context, Result};

impl PassManager {
    pub fn mask_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        width: u32,
        height: u32,
        surface_origin: [f32; 2],
        surface_size: [f32; 2],
        mask: crate::MaskEffect,
    ) -> Result<wgpu::TextureView> {
        let mask_view = self
            .external_textures
            .get(&mask.texture_id)
            .context("mask texture is not registered")?;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mask-filter-output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let params =
            crate::pipeline::MaskFilterRenderer::params(surface_origin, surface_size, mask, false);
        let group = self
            .mask_filter
            .bind_group(&self.device, source, mask_view, params);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mask-filter-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.mask_filter.record(&mut pass, &group);
        drop(pass);
        Ok(view)
    }

    pub fn mask_group_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        width: u32,
        height: u32,
        surface_origin: [f32; 2],
        surface_size: [f32; 2],
        group: &crate::MaskGroupEffect,
        text_clip: Option<&wgpu::TextureView>,
    ) -> Result<wgpu::TextureView> {
        anyhow::ensure!(!group.layers.is_empty(), "mask group has no layers");
        let mut coverages = Vec::with_capacity(group.layers.len());
        for layer in &group.layers {
            let mask_view = self
                .external_textures
                .get(&layer.mask.texture_id)
                .context("mask texture is not registered")?;
            let view = self.new_filter_target(width, height, "mask-layer-coverage");
            let params = crate::pipeline::MaskFilterRenderer::params(
                surface_origin,
                surface_size,
                layer.mask,
                true,
            );
            let bind = self
                .mask_filter
                .bind_group(&self.device, mask_view, mask_view, params);
            self.record_mask_pass(encoder, &view, &bind, false);
            let view = if layer.text_clip {
                let text_clip = text_clip.context("text-clipped mask has no glyph coverage")?;
                let clipped = self.new_filter_target(width, height, "text-clipped-mask-layer");
                let bind = self.mask_filter.composite_group(
                    &self.device,
                    &view,
                    text_clip,
                    crate::MaskComposite::Intersect,
                );
                self.record_mask_pass(encoder, &clipped, &bind, true);
                clipped
            } else {
                view
            };
            coverages.push(view);
        }

        let mut accumulated = coverages.pop().expect("non-empty mask coverages");
        for (layer, coverage) in group.layers[..group.layers.len() - 1]
            .iter()
            .rev()
            .zip(coverages.into_iter().rev())
        {
            let view = self.new_filter_target(width, height, "mask-composite-output");
            let bind = self.mask_filter.composite_group(
                &self.device,
                &coverage,
                &accumulated,
                layer.composite,
            );
            self.record_mask_pass(encoder, &view, &bind, true);
            accumulated = view;
        }

        let view = self.new_filter_target(width, height, "mask-group-output");
        let rect = crate::Rect {
            x: surface_origin[0],
            y: surface_origin[1],
            w: surface_size[0],
            h: surface_size[1],
        };
        let params = crate::pipeline::MaskFilterRenderer::params(
            surface_origin,
            surface_size,
            crate::MaskEffect {
                texture_id: crate::ExternalTextureId(0),
                mode: crate::MaskMode::Alpha,
                rect,
                mapping: None,
            },
            false,
        );
        let bind = self
            .mask_filter
            .bind_group(&self.device, source, &accumulated, params);
        self.record_mask_pass(encoder, &view, &bind, false);
        Ok(view)
    }

    fn new_filter_target(&self, width: u32, height: u32, label: &'static str) -> wgpu::TextureView {
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default())
    }

    fn record_mask_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        group: &wgpu::BindGroup,
        composite: bool,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mask-compositor-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        if composite {
            self.mask_filter.record_composite(&mut pass, group);
        } else {
            self.mask_filter.record(&mut pass, group);
        }
    }

    pub fn drop_shadow_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        width: u32,
        height: u32,
        shadow: crate::DropShadow,
    ) -> wgpu::TextureView {
        let blurred = (shadow.blur_radius > 0.0)
            .then(|| self.blur_surface(encoder, source, width, height, shadow.blur_radius));
        let shadow_mask = blurred.as_ref().unwrap_or(source);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("drop-shadow-filter-output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = self.drop_shadow_filter.bind_group(
            &self.device,
            source,
            shadow_mask,
            [width, height],
            shadow,
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("drop-shadow-filter-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.drop_shadow_filter.record(&mut pass, &group);
        drop(pass);
        view
    }

    pub fn color_filter_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        width: u32,
        height: u32,
        matrix: crate::ColorMatrix,
    ) -> wgpu::TextureView {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("color-filter-output"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let group = self.color_filter.bind_group(&self.device, source, matrix);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("color-filter-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.color_filter.record(&mut pass, &group);
        drop(pass);
        view
    }

    /// Apply a separable Gaussian blur and return the final filtered texture view.
    pub fn blur_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        width: u32,
        height: u32,
        sigma: f32,
    ) -> wgpu::TextureView {
        let make_target = |label| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let ping = make_target("filter-blur-ping");
        let output = make_target("filter-blur-output");
        let ping_view = ping.create_view(&wgpu::TextureViewDescriptor::default());
        let output_view = output.create_view(&wgpu::TextureViewDescriptor::default());
        let texel = [1.0 / width as f32, 1.0 / height as f32];

        let horizontal = self.blur_rgba.bind_group_with_params(
            &self.device,
            source,
            &[1.0, 0.0, texel[0], texel[1], sigma, 0.0, 0.0, 0.0],
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("filter-blur-horizontal"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &ping_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.blur_rgba.record(&mut pass, &horizontal);
        }

        let vertical = self.blur_rgba.bind_group_with_params(
            &self.device,
            &ping_view,
            &[0.0, 1.0, texel[0], texel[1], sigma, 0.0, 0.0, 0.0],
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("filter-blur-vertical"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.blur_rgba.record(&mut pass, &vertical);
        }
        output_view
    }
}
