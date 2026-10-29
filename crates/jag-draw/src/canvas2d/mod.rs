//! Ordered persistent bitmap rendering for browser Canvas 2D. No scene z sorting.
//! Authored colors blend in premultiplied sRGB; composition samples linear output.
use crate::gpu_commands::CommandEncoderExt as _;
use crate::{gpu_bindings as bindings, gpu_pipeline as pipeline, gpu_texture as texture};
use std::sync::Arc;

/// An immutable rectangle operation after browser state/transform resolution.
#[derive(Clone, Debug)]
pub struct RectOp {
    pub rect: [f64; 4],
    pub transform: [f64; 6],
    /// Straight sRGB components in 0..=1.
    pub color: [f32; 4],
    pub stroke_width: f64,
    pub clear: bool,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct NativeOp {
    rect: [f32; 4],
    inverse_x: [f32; 4],
    inverse_y: [f32; 4],
    color: [f32; 4],
    options: [f32; 4],
}
impl RectOp {
    fn native(&self) -> Option<NativeOp> {
        let [a, b, c, d, e, f] = self.transform;
        let determinant = a * d - b * c;
        if !determinant.is_finite() || determinant == 0.0 {
            return None;
        }
        let ix = [
            d / determinant,
            -c / determinant,
            (c * f - d * e) / determinant,
            0.0,
        ];
        let iy = [
            -b / determinant,
            a / determinant,
            (b * e - a * f) / determinant,
            0.0,
        ];
        let [x, y, w, h] = self.rect;
        let rect = [x + w.min(0.0), y + h.min(0.0), w.abs(), h.abs()];
        let result = NativeOp {
            rect: rect.map(|v| v as f32),
            inverse_x: ix.map(|v| v as f32),
            inverse_y: iy.map(|v| v as f32),
            color: self.color,
            options: [self.stroke_width as f32, f32::from(self.clear), 0.0, 0.0],
        };
        bytemuck::cast_slice::<NativeOp, f32>(&[result])
            .iter()
            .all(|v| v.is_finite())
            .then_some(result)
    }
}

/// Immutable native ownership can outlive the page registration during submission.
pub struct Bitmap {
    pixels: wgpu::Texture,
    linear: wgpu::Texture,
    pub width: u32,
    pub height: u32,
    alpha: bool,
    initialized: bool,
}
impl Bitmap {
    pub fn composition_view(&self) -> wgpu::TextureView {
        texture::create_default_texture_view(&self.linear)
    }
    /// Host-only diagnostic/readback source; never a JavaScript resource.
    pub fn pixels(&self) -> &wgpu::Texture {
        &self.pixels
    }
    pub fn byte_size(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height) * 12
    }
}

/// Lives in the compositor, on its existing native device and queue.
pub struct Renderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    fill: wgpu::RenderPipeline,
    clear: wgpu::RenderPipeline,
    clear_opaque: wgpu::RenderPipeline,
    normalize: wgpu::RenderPipeline,
}
impl Renderer {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let shader =
            bindings::create_wgsl_shader(&device, "canvas2d rectangles", include_str!("rect.wgsl"));
        let normalized = bindings::create_wgsl_shader(
            &device,
            "canvas2d normalization",
            include_str!("normalize.wgsl"),
        );
        let blend = |src_factor, dst_factor| wgpu::BlendComponent {
            src_factor,
            dst_factor,
            operation: wgpu::BlendOperation::Add,
        };
        let over = blend(wgpu::BlendFactor::One, wgpu::BlendFactor::OneMinusSrcAlpha);
        let erase = blend(wgpu::BlendFactor::Zero, wgpu::BlendFactor::OneMinusSrcAlpha);
        let make = |label, shader: &wgpu::ShaderModule, format, blending| {
            pipeline::create_render_pipeline(
                &device,
                pipeline::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: None,
                    vertex: pipeline::VertexState {
                        module: shader,
                        entry_point: "vertex",
                        buffers: &[],
                    },
                    fragment: Some(pipeline::FragmentState {
                        module: shader,
                        entry_point: "fragment",
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: blending,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    multiview: None,
                },
            )
        };
        let fill = make(
            "canvas2d source over",
            &shader,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(wgpu::BlendState {
                color: over,
                alpha: over,
            }),
        );
        let clear = make(
            "canvas2d clear",
            &shader,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(wgpu::BlendState {
                color: erase,
                alpha: erase,
            }),
        );
        let clear_opaque = make(
            "canvas2d opaque clear",
            &shader,
            wgpu::TextureFormat::Rgba8Unorm,
            Some(wgpu::BlendState {
                color: erase,
                alpha: over,
            }),
        );
        let normalize = make(
            "canvas2d linear output",
            &normalized,
            wgpu::TextureFormat::Rgba16Float,
            None,
        );
        Self {
            device,
            queue,
            fill,
            clear,
            clear_opaque,
            normalize,
        }
    }
    /// Caller admits the 12 bytes/pixel charge before allocating. Zero bitmaps have no target.
    pub fn create_bitmap(&self, width: u32, height: u32, alpha: bool) -> Result<Bitmap, String> {
        if width == 0
            || height == 0
            || width > self.device.limits().max_texture_dimension_2d
            || height > self.device.limits().max_texture_dimension_2d
        {
            return Err("Canvas 2D bitmap dimensions exceed native limits".into());
        }
        let make = |format| {
            texture::create_texture_2d(
                &self.device,
                "canvas2d persistent bitmap",
                texture::Texture2dSpec {
                    width,
                    height,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                },
            )
        };
        Ok(Bitmap {
            pixels: make(wgpu::TextureFormat::Rgba8Unorm),
            linear: make(wgpu::TextureFormat::Rgba16Float),
            width,
            height,
            alpha,
            initialized: false,
        })
    }
    /// All pending author calls execute in one ordered pass. No replay of old calls.
    pub fn apply(&self, bitmap: &mut Bitmap, ops: &[RectOp], reset: bool) -> bool {
        let converted: Vec<_> = ops
            .iter()
            .filter_map(|op| op.native().map(|native| (native, op.clear)))
            .collect();
        if bitmap.initialized && !reset && converted.is_empty() {
            return false;
        }
        let mut native: Vec<_> = converted.iter().map(|(v, _)| *v).collect();
        if native.is_empty() {
            native.push(bytemuck::Zeroable::zeroed());
        }
        let storage = crate::gpu_transfer::initialize_buffer_resource(
            &self.device,
            "canvas2d commands",
            bytemuck::cast_slice(&native),
            wgpu::BufferUsages::STORAGE,
        );
        let groups: Vec<_> = [&self.fill, &self.clear, &self.clear_opaque]
            .iter()
            .map(|p| {
                bindings::create_bind_group(
                    &self.device,
                    "canvas2d commands",
                    &p.get_bind_group_layout(0),
                    &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: storage.as_entire_binding(),
                    }],
                )
            })
            .collect();
        let view = texture::create_default_texture_view(&bitmap.pixels);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass =
                encoder.begin_semantic_render_pass(crate::gpu_commands::RenderPassDescriptor {
                    label: Some("canvas2d ordered bitmap writes"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: if reset || !bitmap.initialized {
                                wgpu::LoadOp::Clear(if bitmap.alpha {
                                    wgpu::Color::TRANSPARENT
                                } else {
                                    wgpu::Color::BLACK
                                })
                            } else {
                                wgpu::LoadOp::Load
                            },
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            for (i, (_, clear)) in converted.iter().enumerate() {
                let pipeline = if *clear {
                    if bitmap.alpha {
                        &self.clear
                    } else {
                        &self.clear_opaque
                    }
                } else {
                    &self.fill
                };
                let group_index = if *clear {
                    if bitmap.alpha { 1 } else { 2 }
                } else {
                    0
                };
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &groups[group_index], &[]);
                pass.draw(0..3, i as u32..i as u32 + 1);
            }
        }
        let source = bindings::create_bind_group(
            &self.device,
            "canvas2d bitmap sample",
            &self.normalize.get_bind_group_layout(0),
            &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        );
        let linear = texture::create_default_texture_view(&bitmap.linear);
        {
            let mut pass =
                encoder.begin_semantic_render_pass(crate::gpu_commands::RenderPassDescriptor {
                    label: Some("canvas2d linear composition"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &linear,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
            pass.set_pipeline(&self.normalize);
            pass.set_bind_group(0, &source, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit([encoder.finish()]);
        bitmap.initialized = true;
        true
    }
}
