use std::sync::Arc;

use crate::gpu_transfer::initialize_buffer_resource;

pub struct ColorFilterRenderer {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl ColorFilterRenderer {
    pub fn new(device: Arc<wgpu::Device>, format: wgpu::TextureFormat) -> Self {
        let shader = crate::gpu_bindings::create_wgsl_shader(
            &device,
            "color-filter-shader",
            jag_shaders::COLOR_FILTER_WGSL,
        );
        let entries = [
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(80),
                },
                count: None,
            },
        ];
        let layout =
            crate::gpu_bindings::create_bind_group_layout(&device, "color-filter-layout", &entries);
        let pipeline_layout = crate::gpu_bindings::create_pipeline_layout(
            &device,
            "color-filter-pipeline-layout",
            &[&layout],
        );
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("color-filter-pipeline"),
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
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
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
        matrix: crate::ColorMatrix,
    ) -> wgpu::BindGroup {
        let data = [
            matrix.rows[0],
            matrix.rows[1],
            matrix.rows[2],
            matrix.rows[3],
            matrix.bias,
        ];
        let buffer = initialize_buffer_resource(
            device,
            "color-filter-params",
            bytemuck::cast_slice(&data),
            wgpu::BufferUsages::UNIFORM,
        );
        crate::gpu_bindings::create_bind_group(
            &device,
            "color-filter-group",
            &self.layout,
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
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
