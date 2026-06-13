//! `render_unified` entry point: shared viewport-uniform setup, then dispatch
//! to the direct (surface) or offscreen (intermediate) path.
//!
//! The two path bodies were extracted verbatim into `render_direct` /
//! `render_offscreen`; the per-resource preparation loops live in `text_prep` /
//! `quad_prep`. This module keeps the original setup and the `if direct` branch
//! unchanged. No logic changed.

use super::PassManager;
use crate::allocator::RenderAllocator;
use crate::upload::GpuScene;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
impl PassManager {
    pub fn render_unified(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        allocator: &mut RenderAllocator,
        surface_view: &wgpu::TextureView,
        width: u32,
        height: u32,
        scene: &GpuScene,
        solid_batches: &[crate::upload::SolidBatch],
        transparent_scene: &GpuScene,
        transparent_batches: &[crate::upload::TransparentBatch],
        glyph_draws: &[(
            [f32; 2],
            crate::text::RasterizedGlyph,
            crate::ColorLinPremul,
            i32,
            Option<crate::Rect>,
        )], // (origin, glyph, color, z, clip)
        svg_draws: &[(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            Option<crate::SvgStyle>,
            i32,
            f32,
            crate::Transform2D,
            Option<crate::Rect>,
            Option<crate::RoundedRectClipGpu>,
        )],
        image_draws: &[(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            i32,
            f32,
            Option<crate::Rect>,
            Option<crate::RoundedRectClipGpu>,
        )],
        backdrop_blur_draws: &[crate::BackdropBlurDraw],
        external_texture_draws: &[crate::upload::ExtractedExternalTextureDraw],
        clear: wgpu::Color,
        direct: bool,
        queue: &wgpu::Queue,
        preserve_surface: bool,
    ) {
        // Update viewport uniform. `logical` is the combined DPI/UI scale factor
        // applied to all scene coordinates when mapping to device pixels.
        let logical =
            crate::dpi::logical_multiplier(self.logical_pixels, self.scale_factor, self.ui_scale);
        let inv_logical = if logical.is_finite() && logical > 0.0 {
            1.0 / logical
        } else {
            1.0
        };
        let scale = [
            (2.0f32 / (width.max(1) as f32)) * logical,
            (-2.0f32 / (height.max(1) as f32)) * logical,
        ];
        let translate = [-1.0f32, 1.0f32];
        let vp_data: [f32; 8] = [
            scale[0],
            scale[1],
            translate[0],
            translate[1],
            self.scroll_offset[0],
            self.scroll_offset[1],
            0.0, // padding
            0.0, // padding
        ];
        let data = bytemuck::bytes_of(&vp_data);
        queue.write_buffer(&self.vp_buffer, 0, data);
        let transparent_text_z: std::collections::HashSet<i32> =
            transparent_batches.iter().map(|b| b.z).collect();

        // Ensure depth buffer matches current render size (1x sample)
        self.ensure_depth_texture(allocator, width.max(1), height.max(1));

        if direct {
            self.render_unified_direct(
                encoder,
                surface_view,
                width,
                height,
                scene,
                solid_batches,
                transparent_scene,
                transparent_batches,
                glyph_draws,
                svg_draws,
                image_draws,
                external_texture_draws,
                clear,
                queue,
                preserve_surface,
                inv_logical,
                &transparent_text_z,
            );
        } else {
            self.render_unified_offscreen(
                encoder,
                allocator,
                surface_view,
                width,
                height,
                scene,
                solid_batches,
                transparent_scene,
                transparent_batches,
                glyph_draws,
                svg_draws,
                image_draws,
                backdrop_blur_draws,
                external_texture_draws,
                clear,
                queue,
                inv_logical,
                &transparent_text_z,
            );
        }
    }
}
