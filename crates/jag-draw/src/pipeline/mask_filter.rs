use std::sync::Arc;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MaskParams {
    surface_origin: [f32; 2],
    surface_size: [f32; 2],
    inverse_x: [f32; 4],
    inverse_y: [f32; 4],
    paint_rect: [f32; 4],
    tile_rect: [f32; 4],
    tile_step: [f32; 2],
    padding: [f32; 2],
    flags: [u32; 4],
}

pub struct MaskFilterRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl MaskFilterRenderer {
    pub fn new(device: Arc<wgpu::Device>, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mask-filter-shader"),
            source: wgpu::ShaderSource::Wgsl(jag_shaders::MASK_FILTER_WGSL.into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mask-filter-layout"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(112),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mask-filter-pipeline-layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mask-filter-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    pub(crate) fn bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        mask: &wgpu::TextureView,
        params: MaskParams,
    ) -> wgpu::BindGroup {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("mask-filter-params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mask-filter-group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(mask),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: buffer.as_entire_binding(),
                },
            ],
        })
    }

    pub(crate) fn params(
        surface_origin: [f32; 2],
        surface_size: [f32; 2],
        mask: crate::MaskEffect,
    ) -> MaskParams {
        let mapping = mask.mapping.unwrap_or(crate::MaskTextureMapping {
            inverse_transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            paint_rect: mask.rect,
            tile_rect: mask.rect,
            tile_step: [mask.rect.w, mask.rect.h],
            repeat_axes: [false; 2],
            flip_y: false,
        });
        let [a, b, c, d, e, f] = mapping.inverse_transform;
        MaskParams {
            surface_origin,
            surface_size,
            inverse_x: [a, c, e, 0.0],
            inverse_y: [b, d, f, 0.0],
            paint_rect: [
                mapping.paint_rect.x,
                mapping.paint_rect.y,
                mapping.paint_rect.w,
                mapping.paint_rect.h,
            ],
            tile_rect: [
                mapping.tile_rect.x,
                mapping.tile_rect.y,
                mapping.tile_rect.w,
                mapping.tile_rect.h,
            ],
            tile_step: mapping.tile_step,
            padding: [0.0; 2],
            flags: [
                u32::from(mask.mode == crate::MaskMode::Luminance),
                u32::from(mapping.repeat_axes[0]),
                u32::from(mapping.repeat_axes[1]),
                u32::from(mapping.flip_y),
            ],
        }
    }

    pub fn record<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, group: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}
