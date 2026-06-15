use crate::upload::GpuScene;
use std::sync::Arc;

pub struct BasicSolidRenderer {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    z_bgl: wgpu::BindGroupLayout,
}

impl BasicSolidRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
    ) -> Self {
        Self::new_with_depth_state(
            device,
            target_format,
            sample_count,
            true,
            wgpu::CompareFunction::LessEqual,
        )
    }

    pub fn new_with_depth_write(
        device: Arc<wgpu::Device>,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        depth_write_enabled: bool,
    ) -> Self {
        Self::new_with_depth_state(
            device,
            target_format,
            sample_count,
            depth_write_enabled,
            wgpu::CompareFunction::LessEqual,
        )
    }

    pub fn new_with_depth_state(
        device: Arc<wgpu::Device>,
        target_format: wgpu::TextureFormat,
        sample_count: u32,
        depth_write_enabled: bool,
        depth_compare: wgpu::CompareFunction,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("solid-shader"),
            source: wgpu::ShaderSource::Wgsl(jag_shaders::SOLID_WGSL.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("solid-vp-bgl"),
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

        let z_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("solid-z-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(4),
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("solid-pipeline-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("solid-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::upload::Vertex>() as u64,
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
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 24,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32,
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
                depth_write_enabled,
                depth_compare,
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
            bgl,
            z_bgl,
        }
    }

    pub fn record<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        vp_bg: &'a wgpu::BindGroup,
        scene: &'a GpuScene,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, vp_bg, &[]);
        pass.set_vertex_buffer(0, scene.vertex.buffer.slice(..));
        pass.set_index_buffer(scene.index.buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..scene.indices, 0, 0..1);
    }

    pub fn record_index_range<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        vp_bg: &'a wgpu::BindGroup,
        scene: &'a GpuScene,
        index_start: u32,
        index_count: u32,
    ) {
        if index_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, vp_bg, &[]);
        pass.set_vertex_buffer(0, scene.vertex.buffer.slice(..));
        pass.set_index_buffer(scene.index.buffer.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(index_start..(index_start + index_count), 0, 0..1);
    }

    pub fn viewport_bgl(&self) -> &wgpu::BindGroupLayout {
        &self.bgl
    }

    pub fn z_index_bgl(&self) -> &wgpu::BindGroupLayout {
        &self.z_bgl
    }
}

/// Solid renderer variant for overlays that do not participate in depth testing.
/// Uses the same SOLID_WGSL shader and vertex layout as `BasicSolidRenderer`, but
/// disables depth-stencil so overlay quads simply blend over existing content.
pub struct OverlaySolidRenderer {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
}

impl OverlaySolidRenderer {
    pub fn new(device: Arc<wgpu::Device>, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("overlay-solid-shader"),
            source: wgpu::ShaderSource::Wgsl(jag_shaders::SOLID_WGSL.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay-solid-vp-bgl"),
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
            label: Some("overlay-solid-pipeline-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay-solid-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::upload::Vertex>() as u64,
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
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 24,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32,
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
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
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
        vbuf: &'a wgpu::Buffer,
        ibuf: &'a wgpu::Buffer,
        icount: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, vp_bg, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..icount, 0, 0..1);
    }
}

/// Scrim renderer that blends over all existing content without depth testing.
/// Uses depth_write_enabled=false and depth_compare=Always so the scrim:
/// 1. Always passes depth test (renders regardless of existing depth)
/// 2. Doesn't write to depth buffer (allows content at higher z to render on top)
/// This is ideal for modal scrim overlays that should dim background content
/// while allowing the modal panel to render on top.
pub struct ScrimSolidRenderer {
    pipeline: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
}

impl ScrimSolidRenderer {
    pub fn new(device: Arc<wgpu::Device>, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scrim-solid-shader"),
            source: wgpu::ShaderSource::Wgsl(jag_shaders::SOLID_WGSL.into()),
        });

        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scrim-solid-vp-bgl"),
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
            label: Some("scrim-solid-pipeline-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("scrim-solid-pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<crate::upload::Vertex>() as u64,
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
                            format: wgpu::VertexFormat::Float32x4,
                        },
                        wgpu::VertexAttribute {
                            offset: 24,
                            shader_location: 2,
                            format: wgpu::VertexFormat::Float32,
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
            // No depth attachment - scrim renders directly to surface without depth testing
            depth_stencil: None,
            // No MSAA - scrim renders directly to final surface
            multisample: wgpu::MultisampleState::default(),
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
        vbuf: &'a wgpu::Buffer,
        ibuf: &'a wgpu::Buffer,
        icount: u32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, vp_bg, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..icount, 0, 0..1);
    }
}
