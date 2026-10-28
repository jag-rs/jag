use std::collections::HashMap;
use std::sync::Arc;

use crate::gpu_texture::{Texture2dSpec, create_default_texture_view, create_texture_2d};
use crate::gpu_transfer::allocate_buffer_resource;

#[derive(Debug)]
pub struct OwnedTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub key: TexKey,
}

#[derive(Debug)]
pub struct OwnedBuffer {
    pub buffer: wgpu::Buffer,
    pub key: BufKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TexKey {
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    pub usage: wgpu::TextureUsages,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct BufKey {
    pub size: u64,
    pub usage: wgpu::BufferUsages,
}

/// Simple render allocator with basic pooling for textures and buffers.
pub struct RenderAllocator {
    device: Arc<wgpu::Device>,
    texture_pool: HashMap<TexKey, Vec<wgpu::Texture>>,
    buffer_pool: HashMap<BufKey, Vec<wgpu::Buffer>>,
}

fn pooled_buffer_size(size: u64) -> u64 {
    let size = size.max(4);
    if size <= 1_048_576 {
        size.next_power_of_two()
    } else {
        let alignment = 1_048_576;
        size.div_ceil(alignment) * alignment
    }
}

impl RenderAllocator {
    pub fn new(device: Arc<wgpu::Device>) -> Self {
        Self {
            device,
            texture_pool: HashMap::new(),
            buffer_pool: HashMap::new(),
        }
    }

    pub fn begin_frame(&mut self) {
        // placeholder for any per-frame bookkeeping
    }

    pub fn end_frame(&mut self) {
        // placeholder for returning transients automatically in the future
    }

    pub fn allocate_texture(&mut self, key: TexKey) -> OwnedTexture {
        let entry = self.texture_pool.entry(key).or_default();
        let texture = entry.pop().unwrap_or_else(|| {
            create_texture_2d(
                &self.device,
                "alloc:tex",
                Texture2dSpec {
                    width: key.width,
                    height: key.height,
                    format: key.format,
                    usage: key.usage,
                },
            )
        });
        let view = create_default_texture_view(&texture);
        OwnedTexture { texture, view, key }
    }

    pub fn release_texture(&mut self, tex: OwnedTexture) {
        self.texture_pool
            .entry(tex.key)
            .or_default()
            .push(tex.texture);
    }

    pub fn allocate_buffer(&mut self, key: BufKey) -> OwnedBuffer {
        let key = BufKey {
            size: pooled_buffer_size(key.size),
            usage: key.usage,
        };
        let entry = self.buffer_pool.entry(key).or_default();
        let buffer = entry.pop().unwrap_or_else(|| {
            allocate_buffer_resource(&self.device, "alloc:buf", key.size, key.usage)
        });
        OwnedBuffer { buffer, key }
    }

    pub fn release_buffer(&mut self, buf: OwnedBuffer) {
        self.buffer_pool
            .entry(buf.key)
            .or_default()
            .push(buf.buffer);
    }
}
