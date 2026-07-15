use std::sync::Arc;

use jag_draw::wgpu;

use super::JagSurface;

impl JagSurface {
    pub(super) fn build_glyph_draws_from_text_draws(
        &self,
        text_draws: &[jag_draw::ExtractedTextDraw],
        provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Vec<(
        [f32; 2],
        jag_draw::RasterizedGlyph,
        jag_draw::ColorLinPremul,
        i32,
        Option<jag_draw::Rect>,
    )> {
        let Some(provider) = provider else {
            return Vec::new();
        };
        let sf = if self.dpi_scale.is_finite() && self.dpi_scale > 0.0 {
            self.dpi_scale
        } else {
            1.0
        };
        let snap = |v: f32| -> f32 { (v * sf).round() / sf };
        let mut glyph_draws = Vec::new();
        for text_draw in text_draws {
            let run = &text_draw.run;
            let [a, b, c, d, e, f] = text_draw.transform.m;
            let origin_x = a * run.pos[0] + c * run.pos[1] + e;
            let origin_y = b * run.pos[0] + d * run.pos[1] + f;
            let sx = (a * a + b * b).sqrt();
            let sy = (c * c + d * d).sqrt();
            let mut scale = if sx.is_finite() && sy.is_finite() {
                if sx > 0.0 && sy > 0.0 {
                    (sx + sy) * 0.5
                } else {
                    sx.max(sy).max(1.0)
                }
            } else {
                1.0
            };
            if !scale.is_finite() || scale <= 0.0 {
                scale = 1.0;
            }
            let logical_size = (run.size * scale).max(1.0);
            let run_for_provider = jag_draw::TextRun {
                text: run.text.clone(),
                pos: [0.0, 0.0],
                size: (logical_size * sf).max(1.0),
                logical_size: 0.0,
                color: run.color,
                weight: run.weight,
                style: run.style,
                family: run.family.clone(),
            };
            for glyph in jag_draw::rasterize_run_cached(provider.as_ref(), &run_for_provider).iter()
            {
                let mut origin = [
                    origin_x + glyph.offset[0] / sf,
                    origin_y + glyph.offset[1] / sf,
                ];
                if logical_size <= 15.0 {
                    origin = [snap(origin[0]), snap(origin[1])];
                }
                glyph_draws.push((
                    origin,
                    Self::grayscale_glyph_for_compositing(glyph),
                    run.color,
                    text_draw.z,
                    text_draw.clip,
                ));
            }
        }
        glyph_draws
    }

    fn grayscale_glyph_for_compositing(
        glyph: &jag_draw::RasterizedGlyph,
    ) -> jag_draw::RasterizedGlyph {
        use jag_draw::{GlyphMask, MaskFormat, SubpixelMask};
        let mask = match &glyph.mask {
            GlyphMask::Color(color) => GlyphMask::Color(color.clone()),
            GlyphMask::Subpixel(mask) => match mask.format {
                MaskFormat::Rgba8 => {
                    let mut out = Vec::with_capacity(mask.data.len());
                    for pixel in mask.data.chunks_exact(4) {
                        let gray =
                            ((u16::from(pixel[0]) + u16::from(pixel[1]) + u16::from(pixel[2])) / 3)
                                as u8;
                        out.extend_from_slice(&[gray, gray, gray, 0]);
                    }
                    GlyphMask::Subpixel(SubpixelMask {
                        width: mask.width,
                        height: mask.height,
                        format: MaskFormat::Rgba8,
                        data: out,
                    })
                }
                MaskFormat::Rgba16 => {
                    let mut out = Vec::with_capacity(mask.data.len());
                    for pixel in mask.data.chunks_exact(8) {
                        let red = u16::from_le_bytes([pixel[0], pixel[1]]);
                        let green = u16::from_le_bytes([pixel[2], pixel[3]]);
                        let blue = u16::from_le_bytes([pixel[4], pixel[5]]);
                        let bytes = ((u32::from(red) + u32::from(green) + u32::from(blue)) / 3)
                            .to_le_bytes();
                        out.extend_from_slice(&[
                            bytes[0], bytes[1], bytes[0], bytes[1], bytes[0], bytes[1], 0, 0,
                        ]);
                    }
                    GlyphMask::Subpixel(SubpixelMask {
                        width: mask.width,
                        height: mask.height,
                        format: MaskFormat::Rgba16,
                        data: out,
                    })
                }
            },
        };
        jag_draw::RasterizedGlyph {
            offset: glyph.offset,
            mask,
        }
    }

    pub(super) fn render_mask_text_coverage(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        width: u32,
        height: u32,
        scene: &jag_draw::UnifiedSceneData,
        glyphs: &[(
            [f32; 2],
            jag_draw::RasterizedGlyph,
            jag_draw::ColorLinPremul,
            i32,
            Option<jag_draw::Rect>,
        )],
    ) -> wgpu::TextureView {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mask-text-coverage"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let white = jag_draw::ColorLinPremul::from_srgba_u8([255; 4]);
        let glyphs = glyphs
            .iter()
            .map(|(origin, glyph, _, z, clip)| (*origin, glyph.clone(), white, *z, *clip))
            .collect::<Vec<_>>();
        let no_solids = [jag_draw::SolidBatch {
            index_start: 0,
            index_count: 0,
            clip: None,
        }];
        self.pass.set_shadow_instances(&[]);
        self.pass.render_unified(
            encoder,
            &mut self.allocator,
            &view,
            width,
            height,
            &scene.gpu_scene,
            &no_solids,
            &scene.transparent_gpu_scene,
            &[],
            &glyphs,
            &[],
            &[],
            &[],
            &[],
            wgpu::Color::TRANSPARENT,
            true,
            &self.queue,
            false,
        );
        view
    }
}
