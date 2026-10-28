use crate::gpu_transfer::initialize_buffer_resource;
use std::sync::Arc;

pub struct ImageRenderer {
    pipeline: wgpu::RenderPipeline,
    vp_bgl: wgpu::BindGroupLayout,
    _z_bgl: wgpu::BindGroupLayout,
    tex_bgl: wgpu::BindGroupLayout,
    params_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl ImageRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        target_format: wgpu::TextureFormat,
        gamma_blend: bool,
    ) -> Self {
        let shader = crate::gpu_bindings::create_wgsl_shader(
            &device,
            "image-shader",
            &jag_shaders::with_blend_space(jag_shaders::IMAGE_WGSL, gamma_blend),
        );

        // Optional debug/escape hatch: allow disabling depth testing for images.
        // When JAG_IMAGE_NO_DEPTH=1, raster images (including WebView textures)
        // are rendered without depth testing so they always blend on top.
        let disable_depth = std::env::var("JAG_IMAGE_NO_DEPTH")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        // Viewport uniform group (matches solids layout)
        let vp_bgl = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "image-vp-bgl",
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
            "image-z-bgl",
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

        // Texture + sampler
        let tex_bgl = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "image-tex-bgl",
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

        // Per-draw image params (ImageParams uniform):
        // 12 floats = 48 bytes: [opacity, premultiplied, clip_enabled, pad,
        //                        clip_rect(4), clip_radii(4)]
        let params_bgl = crate::gpu_bindings::create_bind_group_layout(
            &device,
            "image-params-bgl",
            &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(48),
                },
                count: None,
            }],
        );

        let layout = crate::gpu_bindings::create_pipeline_layout(
            &device,
            "image-pipeline-layout",
            &[&vp_bgl, &z_bgl, &tex_bgl, &params_bgl],
        );

        let depth_stencil = if disable_depth {
            None
        } else {
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            })
        };

        let pipeline = crate::gpu_pipeline::create_render_pipeline(
            &device,
            crate::gpu_pipeline::RenderPipelineDescriptor {
                label: Some("image-pipeline"),
                layout: Some(&layout),
                vertex: crate::gpu_pipeline::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: (std::mem::size_of::<f32>() * 4) as u64,
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
                depth_stencil,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            },
        );

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("image-sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        Self {
            pipeline,
            vp_bgl,
            _z_bgl: z_bgl,
            tex_bgl,
            params_bgl,
            sampler,
        }
    }

    pub fn vp_bind_group(
        &self,
        device: &wgpu::Device,
        vp_buffer: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        crate::gpu_bindings::create_bind_group(
            &device,
            "image-vp-bg",
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
            "image-tex-bg",
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

    /// Create the image-params uniform bind group.
    ///
    /// `rounded_clip`: if `Some`, the shader discards fragments outside
    /// the rounded rect (SDF-based).  The rect and radii must be in
    /// **device pixels** (after DPI scaling).
    ///
    /// `source_linear`: sampling the texture yields linear values (an `*Srgb`
    /// format), so the shader encodes for the target's blend space.
    pub fn params_bind_group(
        &self,
        device: &wgpu::Device,
        opacity: f32,
        premultiplied_input: bool,
        source_linear: bool,
    ) -> (wgpu::BindGroup, wgpu::Buffer) {
        self.params_bind_group_clipped(device, opacity, premultiplied_input, source_linear, None)
    }

    pub fn params_bind_group_clipped(
        &self,
        device: &wgpu::Device,
        opacity: f32,
        premultiplied_input: bool,
        source_linear: bool,
        rounded_clip: Option<&crate::scene::RoundedRectClipGpu>,
    ) -> (wgpu::BindGroup, wgpu::Buffer) {
        let (clip_enabled, clip_rect, clip_radii) = match rounded_clip {
            Some(c) => (1.0_f32, c.rect, c.radii),
            None => (0.0, [0.0; 4], [0.0; 4]),
        };
        // Must match the WGSL ImageParams struct: 12 floats (3 vec4s).
        let params: [f32; 12] = [
            opacity.clamp(0.0, 1.0),
            if premultiplied_input { 1.0 } else { 0.0 },
            clip_enabled,
            if source_linear { 1.0 } else { 0.0 },
            clip_rect[0],
            clip_rect[1],
            clip_rect[2],
            clip_rect[3],
            clip_radii[0],
            clip_radii[1],
            clip_radii[2],
            clip_radii[3],
        ];
        let buffer = initialize_buffer_resource(
            device,
            "image-params-buffer",
            bytemuck::cast_slice(&params),
            wgpu::BufferUsages::UNIFORM,
        );
        let bind_group = crate::gpu_bindings::create_bind_group(
            &device,
            "image-params-bg",
            &self.params_bgl,
            &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        );
        (bind_group, buffer)
    }

    pub fn record<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        vp_bg: &'a wgpu::BindGroup,
        z_bg: &'a wgpu::BindGroup,
        tex_bg: &'a wgpu::BindGroup,
        params_bg: &'a wgpu::BindGroup,
        vbuf: &'a wgpu::Buffer,
        ibuf: &'a wgpu::Buffer,
        icount: u32,
    ) {
        self.record_at_vertex(pass, vp_bg, z_bg, tex_bg, params_bg, vbuf, ibuf, icount, 0);
    }

    pub fn record_at_vertex<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        vp_bg: &'a wgpu::BindGroup,
        z_bg: &'a wgpu::BindGroup,
        tex_bg: &'a wgpu::BindGroup,
        params_bg: &'a wgpu::BindGroup,
        vbuf: &'a wgpu::Buffer,
        ibuf: &'a wgpu::Buffer,
        icount: u32,
        base_vertex: i32,
    ) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, vp_bg, &[]);
        pass.set_bind_group(1, z_bg, &[]);
        pass.set_bind_group(2, tex_bg, &[]);
        pass.set_bind_group(3, params_bg, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(..));
        pass.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..icount, base_vertex, 0..1);
    }
}
