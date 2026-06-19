//! Analytic box-shadow GPU pass.
//!
//! Mirrors `BasicSolidRenderer` (see `pipeline/solid.rs`) but consumes
//! `SHADOW_INSTANCE_WGSL`: each instance expands to one quad (6 vertices, two
//! triangles via `@builtin(vertex_index)`) over the padded shadow bbox, and the
//! fragment shader computes analytic coverage and returns premultiplied
//! `color * coverage`.
//!
//! Shadows are transparent, so the pipeline tests against the opaque depth
//! buffer (`LessEqual`) but does NOT write depth: it slots between the opaque
//! solids and the z-sorted transparent interleave without occluding either.

use std::sync::Arc;

/// GPU renderer for analytic drop shadows. One draw call covers all instances
/// in a single vertex buffer; the vertex stage expands each instance to a quad.
pub struct ShadowInstanceRenderer {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
}

impl ShadowInstanceRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow-instance-shader"),
            source: wgpu::ShaderSource::Wgsl(jag_shaders::SHADOW_INSTANCE_WGSL.into()),
        });

        // Same viewport-uniform layout as the solid renderer (group 0, binding 0).
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow-vp-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(32),
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow-pipeline-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                // Per-instance attributes: stride 64, stepped per instance. The
                // quad's 6 vertices come from `@builtin(vertex_index)`, so there
                // is no per-vertex buffer.
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 64,
                    step_mode: wgpu::VertexStepMode::Instance,
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
                        wgpu::VertexAttribute {
                            offset: 32,
                            shader_location: 3,
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 48,
                            shader_location: 4,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: 56,
                            shader_location: 5,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
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
                // Shadows are transparent: test against opaque depth, don't write.
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: sample_count,
                ..Default::default()
            },
            multiview: None,
        });

        Self { pipeline, bgl }
    }

    pub fn viewport_bgl(&self) -> &wgpu::BindGroupLayout {
        &self.bgl
    }

    pub fn record<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        vp_bg: &'a wgpu::BindGroup,
        instance_buf: &'a wgpu::Buffer,
        instance_count: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, vp_bg, &[]);
        pass.set_vertex_buffer(0, instance_buf.slice(..));
        pass.draw(0..6, 0..instance_count);
    }
}
