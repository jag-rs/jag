use std::sync::Arc;

use crate::gpu_transfer::initialize_buffer_resource;

pub struct DropShadowFilterRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl DropShadowFilterRenderer {
    pub fn new(device: Arc<wgpu::Device>, format: wgpu::TextureFormat) -> Self {
        let shader = crate::gpu_bindings::create_wgsl_shader(
            &device,
            "drop-shadow-filter-shader",
            jag_shaders::DROP_SHADOW_FILTER_WGSL,
        );
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
        let layout = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "drop-shadow-filter-layout",
            &[
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
                        min_binding_size: std::num::NonZeroU64::new(32),
                    },
                    count: None,
                },
            ],
        );
        let pipeline_layout = crate::gpu_bindings::create_pipeline_layout(
            &device,
            "drop-shadow-filter-pipeline-layout",
            &[&layout],
        );
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("drop-shadow-filter-pipeline"),
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

    pub fn bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
        blurred: &wgpu::TextureView,
        size: [u32; 2],
        shadow: crate::DropShadow,
    ) -> wgpu::BindGroup {
        let color = shadow.color;
        let params = [
            shadow.offset[0] / size[0] as f32,
            shadow.offset[1] / size[1] as f32,
            0.0,
            0.0,
            color.r as f32 / 255.0,
            color.g as f32 / 255.0,
            color.b as f32 / 255.0,
            color.a as f32 / 255.0,
        ];
        let buffer = initialize_buffer_resource(
            device,
            "drop-shadow-filter-params",
            bytemuck::cast_slice(&params),
            wgpu::BufferUsages::UNIFORM,
        );
        crate::gpu_bindings::create_bind_group(
            &device,
            "drop-shadow-filter-group",
            &self.layout,
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(blurred),
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
        )
    }

    pub fn record<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, group: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.draw(0..3, 0..1);
    }
}
