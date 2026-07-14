use std::sync::Arc;

use anyhow::Result;

use jag_draw::{Command, DisplayList, ExternalTextureId, Rect, Transform2D, Viewport, wgpu};

use super::JagSurface;

#[derive(Clone, Copy, Debug, PartialEq)]
struct LayerGeometry {
    origin: [f32; 2],
    logical_size: [f32; 2],
    pixel_size: [u32; 2],
}

fn layer_geometry(bounds: Rect, viewport: Viewport, scale: f32) -> Option<LayerGeometry> {
    if !scale.is_finite() || scale <= 0.0 || !bounds.x.is_finite() || !bounds.y.is_finite() {
        return None;
    }
    let x0 = (bounds.x * scale).floor().clamp(0.0, viewport.width as f32) as u32;
    let y0 = (bounds.y * scale)
        .floor()
        .clamp(0.0, viewport.height as f32) as u32;
    let x1 = ((bounds.x + bounds.w) * scale)
        .ceil()
        .clamp(0.0, viewport.width as f32) as u32;
    let y1 = ((bounds.y + bounds.h) * scale)
        .ceil()
        .clamp(0.0, viewport.height as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(LayerGeometry {
        origin: [x0 as f32 / scale, y0 as f32 / scale],
        logical_size: [(x1 - x0) as f32 / scale, (y1 - y0) as f32 / scale],
        pixel_size: [x1 - x0, y1 - y0],
    })
}

fn translate_clips(commands: &mut [Command], origin: [f32; 2]) {
    for command in commands {
        if let Command::PushClip(clip) = command {
            clip.0.x -= origin[0];
            clip.0.y -= origin[1];
        }
    }
}

impl JagSurface {
    fn allocate_synthetic_external_texture_id(&mut self) -> ExternalTextureId {
        let id = ExternalTextureId(self.next_synthetic_external_texture_id);
        self.next_synthetic_external_texture_id =
            self.next_synthetic_external_texture_id.wrapping_add(1);
        id
    }

    fn effect_group_z(commands: &[Command]) -> Option<i32> {
        commands.iter().filter_map(Command::z_index).min()
    }

    fn build_glyph_draws_from_text_draws(
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
            let mut s = if sx.is_finite() && sy.is_finite() {
                if sx > 0.0 && sy > 0.0 {
                    (sx + sy) * 0.5
                } else {
                    sx.max(sy).max(1.0)
                }
            } else {
                1.0
            };
            if !s.is_finite() || s <= 0.0 {
                s = 1.0;
            }

            let logical_size = (run.size * s).max(1.0);
            let physical_size = (logical_size * sf).max(1.0);
            let run_for_provider = jag_draw::TextRun {
                text: run.text.clone(),
                pos: [0.0, 0.0],
                size: physical_size,
                logical_size: 0.0,
                color: run.color,
                weight: run.weight,
                style: run.style,
                family: run.family.clone(),
            };

            let glyphs = jag_draw::rasterize_run_cached(provider.as_ref(), &run_for_provider);
            for g in glyphs.iter() {
                let mut origin = [origin_x + g.offset[0] / sf, origin_y + g.offset[1] / sf];
                if logical_size <= 15.0 {
                    origin[0] = snap(origin[0]);
                    origin[1] = snap(origin[1]);
                }
                // Opacity groups are rendered into intermediate layers; LCD/subpixel text
                // can ghost when composited again. Force grayscale AA in this path.
                glyph_draws.push((
                    origin,
                    Self::grayscale_glyph_for_compositing(g),
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
            GlyphMask::Color(c) => GlyphMask::Color(c.clone()),
            GlyphMask::Subpixel(m) => match m.format {
                MaskFormat::Rgba8 => {
                    let mut out = Vec::with_capacity(m.data.len());
                    for px in m.data.chunks_exact(4) {
                        let gray =
                            ((u16::from(px[0]) + u16::from(px[1]) + u16::from(px[2])) / 3) as u8;
                        out.extend_from_slice(&[gray, gray, gray, 0]);
                    }
                    GlyphMask::Subpixel(SubpixelMask {
                        width: m.width,
                        height: m.height,
                        format: MaskFormat::Rgba8,
                        data: out,
                    })
                }
                MaskFormat::Rgba16 => {
                    let mut out = Vec::with_capacity(m.data.len());
                    for px in m.data.chunks_exact(8) {
                        let r = u16::from_le_bytes([px[0], px[1]]);
                        let g = u16::from_le_bytes([px[2], px[3]]);
                        let b = u16::from_le_bytes([px[4], px[5]]);
                        let gray = ((u32::from(r) + u32::from(g) + u32::from(b)) / 3) as u16;
                        let gb = gray.to_le_bytes();
                        out.extend_from_slice(&[gb[0], gb[1], gb[0], gb[1], gb[0], gb[1], 0, 0]);
                    }
                    GlyphMask::Subpixel(SubpixelMask {
                        width: m.width,
                        height: m.height,
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

    fn render_effect_group_layer(
        &mut self,
        geometry: LayerGeometry,
        mut commands: Vec<Command>,
        effect: jag_draw::SurfaceEffect,
        text_provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Result<ExternalTextureId> {
        translate_clips(&mut commands, geometry.origin);
        let group_list = DisplayList {
            viewport: Viewport {
                width: geometry.pixel_size[0],
                height: geometry.pixel_size[1],
            },
            commands,
        };

        let group_scene =
            jag_draw::upload_display_list_unified(&mut self.allocator, &self.queue, &group_list)?;
        let group_glyphs =
            self.build_glyph_draws_from_text_draws(&group_scene.text_draws, text_provider);

        let mut group_svgs: Vec<_> = group_scene
            .svg_draws
            .iter()
            .map(|draw| {
                (
                    crate::resolve_asset_path(&draw.path),
                    draw.origin,
                    draw.size,
                    None,
                    draw.z,
                    draw.opacity,
                    Transform2D::identity(),
                    None, // no clip for opacity group internals
                    None, // no rounded clip for opacity group internals
                )
            })
            .collect();
        group_svgs.sort_by_key(|(_, _, _, _, z, _, _, _, _)| *z);

        let mut group_images: Vec<(
            std::path::PathBuf,
            [f32; 2],
            [f32; 2],
            i32,
            f32,
            Option<jag_draw::Rect>,
            Option<jag_draw::RoundedRectClipGpu>,
        )> = Vec::new();
        for draw in &group_scene.image_draws {
            let resolved_path = crate::resolve_asset_path(&draw.path);
            if self.pass.try_get_image_view(&resolved_path).is_some() {
                group_images.push((
                    resolved_path,
                    draw.origin,
                    draw.size,
                    draw.z,
                    draw.opacity,
                    None,
                    None,
                ));
            } else {
                self.pending_image_loads |= self.pass.request_image_load(&resolved_path);
            }
        }
        group_images.sort_by_key(|(_, _, _, z, _, _, _)| *z);

        let width = geometry.pixel_size[0];
        let height = geometry.pixel_size[1];
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("effect-group-layer"),
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
        let layer_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("effect-group-encoder"),
            });
        // Shift world coordinates into this bounded layer's local pixel-aligned origin.
        let saved_scroll = self.pass.scroll_offset();
        self.pass
            .set_scroll_offset([-geometry.origin[0], -geometry.origin[1]]);
        self.pass
            .set_shadow_instances(&group_scene.shadow_instances);
        self.pass.render_unified(
            &mut encoder,
            &mut self.allocator,
            &layer_view,
            width,
            height,
            &group_scene.gpu_scene,
            &group_scene.solid_batches,
            &group_scene.transparent_gpu_scene,
            &group_scene.transparent_batches,
            &group_glyphs,
            &group_svgs,
            &group_images,
            &[],
            &group_scene.external_texture_draws,
            wgpu::Color::TRANSPARENT,
            true,
            &self.queue,
            false,
        );
        self.pass.set_scroll_offset(saved_scroll);
        self.pass.set_shadow_instances(&[]);
        let layer_view = match effect {
            jag_draw::SurfaceEffect::Opacity(_) => layer_view,
            jag_draw::SurfaceEffect::Blur(radius) => {
                self.pass
                    .blur_surface(&mut encoder, &layer_view, width, height, radius)
            }
            jag_draw::SurfaceEffect::ColorMatrix(matrix) => {
                self.pass
                    .color_filter_surface(&mut encoder, &layer_view, width, height, matrix)
            }
            jag_draw::SurfaceEffect::DropShadow(shadow) => {
                self.pass
                    .drop_shadow_surface(&mut encoder, &layer_view, width, height, shadow)
            }
            jag_draw::SurfaceEffect::Mask(mask) => self.pass.mask_surface(
                &mut encoder,
                &layer_view,
                width,
                height,
                geometry.origin,
                geometry.logical_size,
                mask,
            )?,
        };
        self.queue.submit(std::iter::once(encoder.finish()));

        let tex_id = self.allocate_synthetic_external_texture_id();
        self.pass.register_external_texture(tex_id, layer_view);
        Ok(tex_id)
    }

    pub(super) fn flatten_effect_groups(
        &mut self,
        commands: &[Command],
        viewport: Viewport,
        text_provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Result<Vec<Command>> {
        let plan = jag_draw::build_compositor_plan(&DisplayList {
            viewport,
            commands: commands.to_vec(),
        })?;
        let mut out: Vec<Command> = Vec::new();
        let mut i = 0usize;
        while i < commands.len() {
            match commands[i] {
                Command::PushOpacity(_) | Command::PushFilter(_) => {
                    let surface = plan
                        .surfaces
                        .iter()
                        .find(|surface| surface.parent.is_none() && surface.commands.start == i + 1)
                        .expect("validated compositor plan must own each root opacity scope");
                    let mut raw_group = commands[surface.commands.clone()].to_vec();
                    if let Some(clip) = surface.inherited_clip {
                        raw_group.insert(0, Command::PushClip(jag_draw::ClipRect(clip)));
                        raw_group.push(Command::PopClip);
                    }
                    let flattened_group =
                        self.flatten_effect_groups(&raw_group, viewport, text_provider)?;

                    // Preserve hit-only regions outside the composited layer.
                    for cmd in flattened_group.iter() {
                        match cmd {
                            Command::HitRegionRect { .. }
                            | Command::HitRegionRoundedRect { .. }
                            | Command::HitRegionEllipse { .. } => out.push(cmd.clone()),
                            _ => {}
                        }
                    }

                    let layer_opacity = match surface.effect {
                        jag_draw::SurfaceEffect::Opacity(opacity) => opacity,
                        jag_draw::SurfaceEffect::Blur(_) => 1.0,
                        jag_draw::SurfaceEffect::ColorMatrix(_) => 1.0,
                        jag_draw::SurfaceEffect::DropShadow(_) => 1.0,
                        jag_draw::SurfaceEffect::Mask(_) => 1.0,
                    };
                    if layer_opacity > 0.0
                        && let Some(z) = Self::effect_group_z(&flattened_group)
                        && let Some(bounds) = surface.bounds
                    {
                        // DrawExternalTexture coordinates are interpreted in logical units
                        // by PassManager when logical pixel mode is enabled.
                        let logical_scale = jag_draw::logical_multiplier(
                            self.logical_pixels,
                            self.dpi_scale,
                            self.ui_scale,
                        );
                        if let Some(geometry) = layer_geometry(bounds, viewport, logical_scale) {
                            let tex_id = self.render_effect_group_layer(
                                geometry,
                                flattened_group,
                                surface.effect,
                                text_provider,
                            )?;
                            out.push(Command::DrawExternalTexture {
                                rect: Rect {
                                    x: geometry.origin[0],
                                    y: geometry.origin[1],
                                    w: geometry.logical_size[0],
                                    h: geometry.logical_size[1],
                                },
                                texture_id: tex_id,
                                z,
                                transform: Transform2D::identity(),
                                opacity: layer_opacity,
                                premultiplied: true,
                            });
                        }
                    }
                    i = surface.commands.end + 1;
                }
                Command::PopOpacity | Command::PopFilter => {
                    // Ignore unmatched pops.
                    i += 1;
                }
                _ => {
                    out.push(commands[i].clone());
                    i += 1;
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_bounds_align_outward_to_device_pixels() {
        let geometry = layer_geometry(
            Rect {
                x: 10.25,
                y: 20.75,
                w: 5.5,
                h: 4.5,
            },
            Viewport {
                width: 200,
                height: 200,
            },
            2.0,
        )
        .unwrap();
        assert_eq!(geometry.origin, [10.0, 20.5]);
        assert_eq!(geometry.logical_size, [6.0, 5.0]);
        assert_eq!(geometry.pixel_size, [12, 10]);
    }

    #[test]
    fn layer_bounds_are_clamped_to_the_viewport() {
        let geometry = layer_geometry(
            Rect {
                x: -5.0,
                y: 8.0,
                w: 20.0,
                h: 10.0,
            },
            Viewport {
                width: 10,
                height: 12,
            },
            1.0,
        )
        .unwrap();
        assert_eq!(geometry.origin, [0.0, 8.0]);
        assert_eq!(geometry.logical_size, [10.0, 4.0]);
        assert_eq!(geometry.pixel_size, [10, 4]);
    }

    #[test]
    fn inherited_clips_are_translated_into_layer_space() {
        let mut commands = vec![Command::PushClip(jag_draw::ClipRect(Rect {
            x: 12.0,
            y: 18.0,
            w: 5.0,
            h: 6.0,
        }))];
        translate_clips(&mut commands, [10.0, 15.0]);
        let Command::PushClip(clip) = &commands[0] else {
            unreachable!()
        };
        assert_eq!(
            clip.0,
            Rect {
                x: 2.0,
                y: 3.0,
                w: 5.0,
                h: 6.0
            }
        );
    }
}
