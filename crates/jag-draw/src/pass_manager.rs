use std::sync::Arc;

// use anyhow::Result;
// use crate::display_list::{Command, DisplayList, Viewport};
use crate::pipeline::{
    BackdropBlurRenderer, BackgroundRenderer, BasicSolidRenderer, Blitter, BlurRenderer,
    Compositor, OverlaySolidRenderer, ScrimSolidRenderer, ScrimStencilMaskRenderer,
    ScrimStencilRenderer, ShadowCompositeRenderer, SmaaRenderer, TextRenderer,
};

/// Apply a 2D affine transform to a point
pub(crate) fn apply_transform_to_point(point: [f32; 2], transform: crate::Transform2D) -> [f32; 2] {
    let [a, b, c, d, e, f] = transform.m;
    let x = point[0];
    let y = point[1];
    [a * x + c * y + e, b * x + d * y + f]
}

pub(crate) fn transformed_quad_points(
    origin: [f32; 2],
    size: [f32; 2],
    transform: crate::Transform2D,
) -> [[f32; 2]; 4] {
    let right = origin[0] + size[0];
    let bottom = origin[1] + size[1];
    [
        apply_transform_to_point(origin, transform),
        apply_transform_to_point([right, origin[1]], transform),
        apply_transform_to_point([right, bottom], transform),
        apply_transform_to_point([origin[0], bottom], transform),
    ]
}

fn clipped_scissor_rect(
    clip: crate::scene::Rect,
    target_width: u32,
    target_height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if target_width == 0
        || target_height == 0
        || !clip.x.is_finite()
        || !clip.y.is_finite()
        || !clip.w.is_finite()
        || !clip.h.is_finite()
        || clip.w <= 0.0
        || clip.h <= 0.0
    {
        return None;
    }

    let x0 = clip.x.max(0.0).floor().min(target_width as f32);
    let y0 = clip.y.max(0.0).floor().min(target_height as f32);
    let x1 = (clip.x + clip.w).ceil().clamp(0.0, target_width as f32);
    let y1 = (clip.y + clip.h).ceil().clamp(0.0, target_height as f32);

    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    let x = x0 as u32;
    let y = y0 as u32;
    Some((
        x,
        y,
        (x1 as u32).saturating_sub(x),
        (y1 as u32).saturating_sub(y),
    ))
}

#[cfg(test)]
mod transform_tests {
    use super::*;

    #[test]
    fn transformed_quad_preserves_rotated_svg_bounds() {
        let transform = crate::Transform2D::rotate_around(std::f32::consts::PI, 408.0, 773.0);

        let quad = transformed_quad_points([400.0, 765.0], [16.0, 16.0], transform);

        assert_point_close(quad[0], [416.0, 781.0]);
        assert_point_close(quad[1], [400.0, 781.0]);
        assert_point_close(quad[2], [400.0, 765.0]);
        assert_point_close(quad[3], [416.0, 765.0]);
    }

    fn assert_point_close(actual: [f32; 2], expected: [f32; 2]) {
        assert!(
            (actual[0] - expected[0]).abs() < 0.001 && (actual[1] - expected[1]).abs() < 0.001,
            "expected point {expected:?}, got {actual:?}"
        );
    }
}

#[cfg(test)]
mod scissor_tests {
    use super::*;

    #[test]
    fn scissor_rejects_empty_clip_below_target() {
        let clip = crate::scene::Rect {
            x: 105.0,
            y: 3700.0,
            w: 868.0,
            h: 0.0,
        };

        assert_eq!(clipped_scissor_rect(clip, 1080, 2282), None);
    }

    #[test]
    fn scissor_clamps_intersecting_clip_to_target() {
        let clip = crate::scene::Rect {
            x: -10.4,
            y: 20.2,
            w: 30.8,
            h: 40.4,
        };

        assert_eq!(clipped_scissor_rect(clip, 100, 100), Some((0, 20, 21, 41)));
    }
}

pub(crate) fn set_scissor_for_clip(
    pass: &mut wgpu::RenderPass<'_>,
    clip: crate::scene::Rect,
    width: u32,
    height: u32,
) -> bool {
    let Some((x, y, w, h)) = clipped_scissor_rect(clip, width, height) else {
        return false;
    };
    pass.set_scissor_rect(x, y, w, h);
    true
}

fn u16_unorm_to_u8(v: u16) -> u8 {
    ((u32::from(v) * 255 + 32767) / 65535) as u8
}

pub(crate) fn glyph_mask_for_atlas(
    mask: &crate::text::GlyphMask,
    force_grayscale: bool,
) -> (u32, u32, std::borrow::Cow<'_, [u8]>) {
    match mask {
        crate::text::GlyphMask::Color(m) => {
            (m.width, m.height, std::borrow::Cow::Borrowed(&m.data))
        }
        crate::text::GlyphMask::Subpixel(m) => match m.format {
            crate::text::MaskFormat::Rgba8 => {
                if !force_grayscale {
                    return (m.width, m.height, std::borrow::Cow::Borrowed(&m.data));
                }

                let mut out = Vec::with_capacity((m.width as usize) * (m.height as usize) * 4);
                for px in m.data.chunks_exact(4) {
                    let gray = ((u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3) as u8;
                    out.extend_from_slice(&[gray, gray, gray, 0]);
                }
                (m.width, m.height, std::borrow::Cow::Owned(out))
            }
            crate::text::MaskFormat::Rgba16 => {
                // Text atlas is RGBA8, so normalize 16-bit glyph masks to RGBA8 on upload.
                let mut out = Vec::with_capacity((m.width as usize) * (m.height as usize) * 4);
                for px in m.data.chunks_exact(8) {
                    let r = u16::from_le_bytes([px[0], px[1]]);
                    let g = u16::from_le_bytes([px[2], px[3]]);
                    let b = u16::from_le_bytes([px[4], px[5]]);
                    let (r8, g8, b8) = if force_grayscale {
                        let gray16 = ((u32::from(r) + u32::from(g) + u32::from(b)) / 3) as u16;
                        let gray8 = u16_unorm_to_u8(gray16);
                        (gray8, gray8, gray8)
                    } else {
                        (u16_unorm_to_u8(r), u16_unorm_to_u8(g), u16_unorm_to_u8(b))
                    };
                    out.extend_from_slice(&[r8, g8, b8, 0]);
                }
                (m.width, m.height, std::borrow::Cow::Owned(out))
            }
        },
    }
}

pub struct PassTargets {
    pub color: crate::OwnedTexture,
}

pub enum Background {
    Solid(crate::scene::ColorLinPremul),
    LinearGradient {
        start_uv: [f32; 2],
        end_uv: [f32; 2],
        stop0: (f32, crate::scene::ColorLinPremul),
        stop1: (f32, crate::scene::ColorLinPremul),
    },
}

pub struct PassManager {
    device: Arc<wgpu::Device>,
    pub solid_offscreen: BasicSolidRenderer,
    pub solid_direct: BasicSolidRenderer,
    pub transparent_solid_offscreen: BasicSolidRenderer,
    pub transparent_solid_direct: BasicSolidRenderer,
    pub solid_direct_no_msaa: BasicSolidRenderer,
    overlay_solid: OverlaySolidRenderer,
    scrim_solid: ScrimSolidRenderer,
    pub compositor: Compositor,
    pub blitter: Blitter,
    pub smaa: SmaaRenderer,
    scrim_mask: ScrimStencilMaskRenderer,
    scrim_stencil: ScrimStencilRenderer,
    // Shadow/blur pipelines and helpers
    pub mask_renderer: BasicSolidRenderer,
    pub blur_r8: BlurRenderer,
    pub backdrop_blur: BackdropBlurRenderer,
    pub shadow_comp: ShadowCompositeRenderer,
    pub text: TextRenderer,
    pub text_offscreen: TextRenderer,
    pub image: crate::pipeline::ImageRenderer,
    pub image_offscreen: crate::pipeline::ImageRenderer,
    pub svg_cache: crate::svg::SvgRasterCache,
    pub image_cache: crate::image_cache::ImageCache,
    offscreen_format: wgpu::TextureFormat,
    surface_format: wgpu::TextureFormat,
    vp_buffer: wgpu::Buffer,
    /// Scroll offset applied via the viewport uniform (GPU-side scroll).
    /// Set by the caller before `render_unified` to shift content without
    /// rebuilding geometry. Values are in logical pixels (negative = scrolled down/right).
    scroll_offset: [f32; 2],
    // Z-index uniform buffer for dynamic depth control (Phase 2)
    z_index_buffer: wgpu::Buffer,
    bg: BackgroundRenderer,
    bg_param_buffer: wgpu::Buffer,
    bg_stops_buffer: wgpu::Buffer,
    // Platform DPI scale factor (used for mac-specific radial centering fix)
    scale_factor: f32,
    // Additional UI scale multiplier for logical pixel mode
    ui_scale: f32,
    // When true, treat positions as logical pixels and scale by `scale_factor` centrally
    logical_pixels: bool,
    // Intermediate texture for Vello-style smooth resizing
    pub intermediate_texture: Option<crate::OwnedTexture>,
    smaa_edges: Option<crate::OwnedTexture>,
    smaa_weights: Option<crate::OwnedTexture>,
    // Depth texture for z-ordering across all element types
    depth_texture: Option<crate::OwnedTexture>,
    // Stencil texture for scrim cutouts
    scrim_stencil_tex: Option<crate::OwnedTexture>,
    // Reusable GPU resources for text rendering to avoid per-glyph allocations.
    text_mask_atlas: wgpu::Texture,
    // Note: This view is not directly read but must be kept alive for the bind group reference
    #[allow(dead_code)]
    text_mask_atlas_view: wgpu::TextureView,
    text_bind_group: wgpu::BindGroup,
    text_atlas_upload: Vec<u8>,
    // Track atlas region used in previous frame for efficient clearing
    prev_atlas_max_x: u32,
    prev_atlas_max_y: u32,
    smaa_param_buffer: wgpu::Buffer,
    // Registry for externally-rendered textures (e.g., 3D viewports)
    external_textures:
        std::collections::HashMap<crate::display_list::ExternalTextureId, wgpu::TextureView>,
}

// Vertex structures for unified rendering
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextQuadVtx {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ImageQuadVtx {
    pos: [f32; 2],
    uv: [f32; 2],
}

mod box_shadow;
mod draw_shapes;
mod paint_root;
mod paint_root_gradients;
mod quad_prep;
mod render;
mod render_direct;
mod render_offscreen;
mod rounded;
mod scrim_cutout;
mod setup;
mod targets;
mod text_prep;
