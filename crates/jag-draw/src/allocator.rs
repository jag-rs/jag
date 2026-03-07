use std::collections::HashMap;
use std::sync::Arc;

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
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("alloc:tex"),
                size: wgpu::Extent3d {
                    width: key.width,
                    height: key.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: key.format,
                usage: key.usage,
                view_formats: &[],
            })
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        OwnedTexture { texture, view, key }
    }

    pub fn release_texture(&mut self, tex: OwnedTexture) {
        self.texture_pool
            .entry(tex.key)
            .or_default()
            .push(tex.texture);
    }

    pub fn allocate_buffer(&mut self, key: BufKey) -> OwnedBuffer {
        let entry = self.buffer_pool.entry(key).or_default();
        let buffer = entry.pop().unwrap_or_else(|| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("alloc:buf"),
                size: key.size,
                usage: key.usage,
                mapped_at_creation: false,
            })
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
