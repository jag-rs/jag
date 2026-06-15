//! sRGB-composite variant of the analytic box-shadow pass.
//!
//! Where [`ShadowInstanceRenderer`](super::ShadowInstanceRenderer) relies on
//! hardware PREMULTIPLIED_ALPHA blending — which composites in the offscreen's
//! stored LINEAR space and makes colored glows ~3.5x too bright vs Chrome —
//! this renderer composites in sRGB (gamma) space to match browsers.
//!
//! GPU fixed-function blending cannot work in gamma space, so the fragment
//! shader (`SHADOW_INSTANCE_COMPOSITE_WGSL`) reads a SNAPSHOT of the
//! destination (group 1, binding 0), performs the sRGB straight-alpha "over"
//! in-shader, and writes the full premultiplied-LINEAR result with REPLACE
//! blend. The caller must therefore copy the offscreen into a snapshot texture
//! and bind it before recording this pass.
//!
//! The vertex stage, viewport uniform, instance layout (stride 48), and depth
//! state (Depth32Float, no depth write, LessEqual) are identical to the linear
//! renderer; only the fragment and the added dst binding differ.

use std::sync::Arc;

/// GPU renderer for analytic drop shadows that composites in sRGB space by
/// reading a destination snapshot. One draw call covers all instances.
pub struct ShadowCompositeInstanceRenderer {
    pipeline: wgpu::RenderPipeline,
    vp_bgl: wgpu::BindGroupLayout,
    dst_bgl: wgpu::BindGroupLayout,
}

impl ShadowCompositeInstanceRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow-instance-composite-shader"),
            source: wgpu::ShaderSource::Wgsl(jag_shaders::SHADOW_INSTANCE_COMPOSITE_WGSL.into()),
        });

        // Group 0: same viewport-uniform layout as the linear shadow renderer.
        let vp_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow-composite-vp-bgl"),
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

        // Group 1: the destination snapshot. Sampled via `textureLoad`, so no
        // sampler is needed and the texture is non-filterable.
        let dst_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow-composite-dst-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow-composite-pipeline-layout"),
            bind_group_layouts: &[&vp_bgl, &dst_bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow-composite-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                // Per-instance attributes: stride 48, stepped per instance. The
                // quad's 6 vertices come from `@builtin(vertex_index)`.
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 48,
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
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    // The shader composites against the snapshot itself, so the
                    // output is the final pixel: write it verbatim (REPLACE).
                    blend: Some(wgpu::BlendState::REPLACE),
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

        Self {
            pipeline,
            vp_bgl,
            dst_bgl,
        }
    }

    pub fn viewport_bgl(&self) -> &wgpu::BindGroupLayout {
        &self.vp_bgl
    }

    pub fn dst_bgl(&self) -> &wgpu::BindGroupLayout {
        &self.dst_bgl
    }

    pub fn record<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        vp_bg: &'a wgpu::BindGroup,
        dst_bg: &'a wgpu::BindGroup,
        instance_buf: &'a wgpu::Buffer,
        instance_count: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, vp_bg, &[]);
        pass.set_bind_group(1, dst_bg, &[]);
        pass.set_vertex_buffer(0, instance_buf.slice(..));
        pass.draw(0..6, 0..instance_count);
    }
}
