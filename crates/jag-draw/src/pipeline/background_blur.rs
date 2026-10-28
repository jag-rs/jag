use std::sync::Arc;

use crate::gpu_transfer::{allocate_buffer_resource, initialize_buffer_resource};

pub struct BackgroundRenderer {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
}

impl BackgroundRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        target_format: wgpu::TextureFormat,
        gamma_blend: bool,
    ) -> Self {
        let shader = crate::gpu_bindings::create_wgsl_shader(
            &device,
            "background-shader",
            &jag_shaders::with_blend_space(jag_shaders::BACKGROUND_WGSL, gamma_blend),
        );
        let bgl = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "bg-bgl",
            &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(64),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(256),
                    },
                    count: None,
                },
            ],
        );
        let layout =
            crate::gpu_bindings::create_pipeline_layout(&device, "bg-pipeline-layout", &[&bgl]);
        let pipeline = crate::gpu_pipeline::create_render_pipeline(
            &device,
            crate::gpu_pipeline::RenderPipelineDescriptor {
                label: Some("bg-pipeline"),
                layout: Some(&layout),
                vertex: crate::gpu_pipeline::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[],
                },
                fragment: Some(crate::gpu_pipeline::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            },
        );
        Self { pipeline, bgl }
    }

    pub fn record<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, bg: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bg, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn bgl(&self) -> &wgpu::BindGroupLayout {
        &self.bgl
    }
}

pub struct BlurRenderer {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pub param_buffer: wgpu::Buffer,
}

impl BlurRenderer {
    pub fn new(device: Arc<wgpu::Device>, target_format: wgpu::TextureFormat) -> Self {
        let shader = crate::gpu_bindings::create_wgsl_shader(
            &device,
            "shadow-blur-shader",
            jag_shaders::SHADOW_BLUR_WGSL,
        );
        let bgl = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "shadow-blur-bgl",
            &[
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
                        min_binding_size: std::num::NonZeroU64::new(32),
                    },
                    count: None,
                },
            ],
        );
        let layout = crate::gpu_bindings::create_pipeline_layout(
            &device,
            "shadow-blur-pipeline-layout",
            &[&bgl],
        );
        let pipeline = crate::gpu_pipeline::create_render_pipeline(
            &device,
            crate::gpu_pipeline::RenderPipelineDescriptor {
                label: Some("shadow-blur-pipeline"),
                layout: Some(&layout),
                vertex: crate::gpu_pipeline::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[],
                },
                fragment: Some(crate::gpu_pipeline::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(wgpu::BlendState::REPLACE),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            },
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow-blur-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let param_buffer = allocate_buffer_resource(
            &device,
            "shadow-blur-params",
            32, // vec2 + vec2 + sigma + pad -> round up to 32
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        Self {
            pipeline,
            bgl,
            sampler,
            param_buffer,
        }
    }

    pub fn bind_group(
        &self,
        device: &wgpu::Device,
        tex_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        crate::gpu_bindings::create_bind_group(
            &device,
            "shadow-blur-bg",
            &self.bgl,
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: self.param_buffer.as_entire_binding(),
                },
            ],
        )
    }

    pub fn bind_group_with_params(
        &self,
        device: &wgpu::Device,
        tex_view: &wgpu::TextureView,
        params: &[f32; 8],
    ) -> wgpu::BindGroup {
        let buffer = initialize_buffer_resource(
            device,
            "blur-pass-params",
            bytemuck::bytes_of(params),
            wgpu::BufferUsages::UNIFORM,
        );
        crate::gpu_bindings::create_bind_group(
            &device,
            "blur-pass-bg",
            &self.bgl,
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(tex_view),
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

    pub fn record<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, bg: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, bg, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[path = "background_blur_effects.rs"]
mod effects;
pub use effects::{BackdropBlurRenderer, ShadowCompositeRenderer};
