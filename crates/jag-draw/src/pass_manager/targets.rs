//! `PassManager` methods (targets). Verbatim extraction from the
//! former monolithic `pass_manager.rs`; no logic changed.

use super::{PassManager, PassTargets};
use crate::allocator::{RenderAllocator, TexKey};

impl PassManager {
    pub fn alloc_targets(
        &self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) -> PassTargets {
        let color = allocator.allocate_texture(TexKey {
            width,
            height,
            format: self.offscreen_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
        });
        PassTargets { color }
    }

    /// Allocate or reuse intermediate texture matching the surface size.
    /// This texture is used for Vello-style smooth resizing.
    ///
    /// Strategy: Always ensure texture matches exact size for MSAA resolve compatibility.
    /// We preserve content by using LoadOp::Load when rendering, not by keeping oversized textures.
    pub fn ensure_intermediate_texture(
        &mut self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) {
        let needs_realloc = match &self.intermediate_texture {
            Some(tex) => {
                // Reallocate if size doesn't match exactly
                // MSAA resolve requires exact size match between MSAA texture and resolve target
                tex.key.width != width || tex.key.height != height
            }
            None => true,
        };

        if needs_realloc {
            // Release old texture if it exists
            if let Some(old_tex) = self.intermediate_texture.take() {
                allocator.release_texture(old_tex);
            }

            // Allocate new intermediate texture with surface format at exact size
            let tex = allocator.allocate_texture(TexKey {
                width,
                height,
                format: self.surface_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
            });
            self.intermediate_texture = Some(tex);
        }
    }

    /// Clear the intermediate texture with the specified color.
    /// This should be called before rendering to the intermediate texture.
    pub fn clear_intermediate_texture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        clear_color: wgpu::Color,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before clearing");

        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear-intermediate"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &intermediate.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
    }

    /// Blit the intermediate texture to the surface. This is a very fast operation
    /// that enables smooth window resizing (Vello-style).
    pub fn blit_to_surface(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        surface_view: &wgpu::TextureView,
    ) {
        let intermediate = self
            .intermediate_texture
            .as_ref()
            .expect("intermediate texture must be allocated before blitting");

        let bg = self.blitter.bind_group(&self.device, &intermediate.view);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("blit-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.blitter.record(&mut pass, &bg);
    }

    /// Ensure depth texture is allocated and matches the given dimensions.
    /// Depth texture is used for z-ordering across all element types (solids, text, images, SVGs).
    pub fn ensure_depth_texture(
        &mut self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) {
        let needs_realloc = match &self.depth_texture {
            Some(tex) => tex.key.width != width || tex.key.height != height,
            None => true,
        };

        if needs_realloc {
            // Release old texture if it exists
            if let Some(old_tex) = self.depth_texture.take() {
                allocator.release_texture(old_tex);
            }

            // Allocate new depth texture at exact size
            let tex = allocator.allocate_texture(TexKey {
                width,
                height,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            });
            self.depth_texture = Some(tex);
        }
    }

    /// Get the depth texture view for use in render passes.
    /// Panics if depth texture hasn't been allocated via ensure_depth_texture.
    pub fn depth_view(&self) -> &wgpu::TextureView {
        &self
            .depth_texture
            .as_ref()
            .expect("depth texture must be allocated before use")
            .view
    }

    pub(crate) fn ensure_scrim_stencil_texture(
        &mut self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) {
        let needs_realloc = match &self.scrim_stencil_tex {
            Some(tex) => tex.key.width != width || tex.key.height != height,
            None => true,
        };

        if needs_realloc {
            if let Some(old) = self.scrim_stencil_tex.take() {
                allocator.release_texture(old);
            }
            let tex = allocator.allocate_texture(TexKey {
                width,
                height,
                format: wgpu::TextureFormat::Depth24PlusStencil8,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            });
            self.scrim_stencil_tex = Some(tex);
        }
    }

    pub(crate) fn ensure_smaa_textures(
        &mut self,
        allocator: &mut RenderAllocator,
        width: u32,
        height: u32,
    ) {
        let key = TexKey {
            width,
            height,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        };

        if self.smaa_edges.as_ref().map_or(true, |tex| tex.key != key) {
            if let Some(old) = self.smaa_edges.take() {
                allocator.release_texture(old);
            }
            self.smaa_edges = Some(allocator.allocate_texture(key));
        }

        if self
            .smaa_weights
            .as_ref()
            .map_or(true, |tex| tex.key != key)
        {
            if let Some(old) = self.smaa_weights.take() {
                allocator.release_texture(old);
            }
            self.smaa_weights = Some(allocator.allocate_texture(key));
        }
    }
}
