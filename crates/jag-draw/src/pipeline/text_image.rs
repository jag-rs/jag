use std::sync::Arc;

use crate::gpu_transfer::allocate_buffer_resource;

pub struct TextRenderer {
    pub pipeline: wgpu::RenderPipeline,
    vp_bgl: wgpu::BindGroupLayout,
    _z_bgl: wgpu::BindGroupLayout,
    pub tex_bgl: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub color_buffer: wgpu::Buffer,
}

impl TextRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        target_format: wgpu::TextureFormat,
        gamma_blend: bool,
    ) -> Self {
        let shader = crate::gpu_bindings::create_wgsl_shader(
            &device,
            "text-shader",
            &jag_shaders::with_blend_space(jag_shaders::TEXT_WGSL, gamma_blend),
        );

        // Viewport uniform group (matches solids layout)
        let vp_bgl = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "text-vp-bgl",
            &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(32),
                },
                count: None,
            }],
        );

        // Z-index uniform
        let z_bgl = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "text-z-bgl",
            &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(4),
                },
                count: None,
            }],
        );

        // Texture + sampler (color is now per-vertex)
        let tex_bgl = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "text-tex-bgl",
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
            ],
        );

        let layout = crate::gpu_bindings::create_pipeline_layout(
            &device,
            "text-pipeline-layout",
            &[&vp_bgl, &z_bgl, &tex_bgl],
        );

        let pipeline = crate::gpu_pipeline::create_render_pipeline(
            &device,
            crate::gpu_pipeline::RenderPipelineDescriptor {
                label: Some("text-pipeline"),
                layout: Some(&layout),
                vertex: crate::gpu_pipeline::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: (std::mem::size_of::<f32>() * 8) as u64, // pos(2) + uv(2) + color(4)
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[
                            wgpu::VertexAttribute {
                                offset: 0,
                                shader_location: 0,
                                format: wgpu::VertexFormat::Float32x2,
                            },
                            wgpu::VertexAttribute {
                                offset: 8,
                                shader_location: 1,
                                format: wgpu::VertexFormat::Float32x2,
                            },
                            wgpu::VertexAttribute {
                                offset: 16,
                                shader_location: 2,
                                format: wgpu::VertexFormat::Float32x4,
                            },
                        ],
                    }],
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

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        let color_buffer = allocate_buffer_resource(
            &device,
            "text-color",
            16,
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );

        Self {
            pipeline,
            vp_bgl,
            _z_bgl: z_bgl,
            tex_bgl,
            sampler,
            color_buffer,
        }
    }

    pub fn vp_bind_group(
        &self,
        device: &wgpu::Device,
        vp_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        crate::gpu_bindings::create_bind_group(
            &device,
            "text-vp-bg",
            &self.vp_bgl,
            &[wgpu::BindGroupEntry {
                binding: 0,
                resource: vp_buffer.as_entire_binding(),
            }],
        )
    }

    pub fn tex_bind_group(
        &self,
        device: &wgpu::Device,
        tex_view: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        crate::gpu_bindings::create_bind_group(
            &device,
            "text-tex-bg",
            &self.tex_bgl,
            &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(tex_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        )
    }

    pub fn record<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        vp_bg: &'a wgpu::BindGroup,
        z_bg: &'a wgpu::BindGroup,
        tex_bg: &'a wgpu::BindGroup,
        vbuf: &'a wgpu::Buffer,
        ibuf: &'a wgpu::Buffer,
        icount: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, vp_bg, &[]);
        pass.set_bind_group(1, z_bg, &[]);
        pass.set_bind_group(2, tex_bg, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..icount, 0, 0..1);
    }
}

#[path = "image_renderer.rs"]
mod effects;
pub use effects::ImageRenderer;
