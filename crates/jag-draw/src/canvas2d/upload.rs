//! Publish native raster coverage/paint output to the same persistent GPU target.
use super::*;
impl Renderer {
    pub fn upload(&self, bitmap: &mut Bitmap, pixels: &[u8]) {
        assert_eq!(
            pixels.len(),
            bitmap.width as usize * bitmap.height as usize * 4
        );
        texture::upload_texture_2d(
            &self.queue,
            &bitmap.pixels,
            [0, 0],
            [bitmap.width, bitmap.height],
            bitmap.width * 4,
            pixels,
        );
        let view = texture::create_default_texture_view(&bitmap.pixels);
        let mut encoder = self.device.create_command_encoder(&Default::default());
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
    }
}
