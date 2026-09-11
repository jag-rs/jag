//! Reuse the small GPU resources used to place retained tiles. Each draw gets
//! its own slot until submission, including repeated uses of the same texture.
use std::sync::{Arc, Weak};

use super::ImageQuadVtx;
use crate::{pipeline::ImageRenderer, upload::ExtractedExternalTextureDraw};
use wgpu::util::DeviceExt;

pub(super) type ExtResource = (
    Arc<wgpu::Buffer>,
    wgpu::Buffer,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::BindGroup,
    wgpu::Buffer,
    wgpu::Buffer,
    i32,
);

const MAX_SLOTS: usize = 256;
const MAX_VERTEX_BATCHES: usize = 8;
const MAX_VERTEX_BYTES: u64 = 256 * 1024;

struct Slot {
    resource: Arc<ExtResource>,
    view: Weak<wgpu::TextureView>,
    z: i32,
    opacity: f32,
    premultiplied: bool,
    rounded_clip: Option<crate::RoundedRectClipGpu>,
}

#[derive(Default)]
pub(super) struct CompositeResources {
    slots: Vec<Slot>,
    next: usize,
    vertices: Vec<Arc<wgpu::Buffer>>,
    next_vertices: usize,
}

impl CompositeResources {
    pub fn begin_frame(&mut self) {
        self.slots.truncate(self.next);
        self.next = 0;
        self.vertices.truncate(self.next_vertices);
        self.next_vertices = 0;
    }

    pub fn release_expired_textures(&mut self) {
        // A bind group retains its GPU view internally. Do not let this pool
        // extend the lifetime of a texture evicted by the tile residency cache.
        self.slots.retain(|slot| slot.view.strong_count() > 0);
    }

    pub fn vertex_batch(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        draws: &[ExtractedExternalTextureDraw],
    ) -> Arc<wgpu::Buffer> {
        let mut vertices = Vec::with_capacity(draws.len() * 4);
        for draw in draws {
            vertices.extend(quad_vertices(draw));
        }
        let data = bytemuck::cast_slice(&vertices);
        let size = (data.len() as u64).max(4).next_power_of_two();
        let index = self.next_vertices;
        self.next_vertices += 1;
        let buffer = self
            .vertices
            .get(index)
            .filter(|b| b.size() >= size)
            .cloned()
            .unwrap_or_else(|| {
                Arc::new(device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("composite-vertex-batch"),
                    size,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }))
            });
        if index < MAX_VERTEX_BATCHES && buffer.size() <= MAX_VERTEX_BYTES {
            if let Some(slot) = self.vertices.get_mut(index) {
                *slot = buffer.clone();
            } else if index == self.vertices.len() {
                self.vertices.push(buffer.clone());
            }
        }
        if !data.is_empty() {
            // One staging allocation and copy for all tiles in this pass.
            queue.write_buffer(&buffer, 0, data);
        }
        buffer
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        renderer: &ImageRenderer,
        vp_buffer: &wgpu::Buffer,
        z_layout: &wgpu::BindGroupLayout,
        draw: &ExtractedExternalTextureDraw,
        view: &Arc<wgpu::TextureView>,
        vertices: &Arc<wgpu::Buffer>,
        base_vertex: i32,
    ) -> Arc<ExtResource> {
        let index = self.next;
        self.next += 1;
        if vertices.size() <= MAX_VERTEX_BYTES
            && let Some(slot) = self.slots.get_mut(index)
        {
            // Slots are not reused within one submission. Queue writes for the
            // next frame execute after the previous submitted draws, so no
            // CPU/GPU fence is required to update these buffers.
            let resource = Arc::get_mut(&mut slot.resource)
                .expect("composite resources still borrowed at frame boundary");
            resource.0 = vertices.clone();
            resource.8 = base_vertex;
            if slot.z != draw.z {
                queue.write_buffer(&resource.6, 0, bytemuck::bytes_of(&(draw.z as f32)));
                slot.z = draw.z;
            }
            if !Weak::ptr_eq(&slot.view, &Arc::downgrade(view)) {
                resource.4 = renderer.tex_bind_group(device, view);
                slot.view = Arc::downgrade(view);
            }
            if slot.opacity != draw.opacity
                || slot.premultiplied != draw.premultiplied
                || slot.rounded_clip != draw.rounded_clip
            {
                (resource.5, resource.7) = renderer.params_bind_group_clipped(
                    device,
                    draw.opacity,
                    draw.premultiplied,
                    draw.rounded_clip.as_ref(),
                );
                slot.opacity = draw.opacity;
                slot.premultiplied = draw.premultiplied;
                slot.rounded_clip = draw.rounded_clip;
            }
            return slot.resource.clone();
        }
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("composite-indices"),
            contents: bytemuck::cast_slice(&[0u16, 1, 2, 0, 2, 3]),
            usage: wgpu::BufferUsages::INDEX,
        });
        let zbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("composite-z"),
            contents: bytemuck::bytes_of(&(draw.z as f32)),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let zbg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite-z"),
            layout: z_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: zbuf.as_entire_binding(),
            }],
        });
        let (params_bg, params_buf) = renderer.params_bind_group_clipped(
            device,
            draw.opacity,
            draw.premultiplied,
            draw.rounded_clip.as_ref(),
        );
        let resource = Arc::new((
            vertices.clone(),
            ibuf,
            renderer.vp_bind_group(device, vp_buffer),
            zbg,
            renderer.tex_bind_group(device, view),
            params_bg,
            zbuf,
            params_buf,
            base_vertex,
        ));
        if index < MAX_SLOTS && index == self.slots.len() && vertices.size() <= MAX_VERTEX_BYTES {
            self.slots.push(Slot {
                resource: resource.clone(),
                view: Arc::downgrade(view),
                z: draw.z,
                opacity: draw.opacity,
                premultiplied: draw.premultiplied,
                rounded_clip: draw.rounded_clip,
            });
        }
        resource
    }
}

fn quad_vertices(draw: &ExtractedExternalTextureDraw) -> [ImageQuadVtx; 4] {
    [
        ImageQuadVtx {
            pos: draw.origin,
            uv: [draw.uv[0], draw.uv[1]],
        },
        ImageQuadVtx {
            pos: [draw.origin[0] + draw.size[0], draw.origin[1]],
            uv: [draw.uv[2], draw.uv[1]],
        },
        ImageQuadVtx {
            pos: [draw.origin[0] + draw.size[0], draw.origin[1] + draw.size[1]],
            uv: [draw.uv[2], draw.uv[3]],
        },
        ImageQuadVtx {
            pos: [draw.origin[0], draw.origin[1] + draw.size[1]],
            uv: [draw.uv[0], draw.uv[3]],
        },
    ]
}
