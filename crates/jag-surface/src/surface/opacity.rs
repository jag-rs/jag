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

    fn render_effect_group_layer(
        &mut self,
        geometry: LayerGeometry,
        mut commands: Vec<Command>,
        effect: jag_draw::SurfaceEffect,
        text_provider: Option<&Arc<dyn jag_draw::TextProvider + Send + Sync>>,
    ) -> Result<ExternalTextureId> {
        let needs_text_clip = matches!(
            &effect,
            jag_draw::SurfaceEffect::MaskGroup(group)
                if group.layers.iter().any(|layer| layer.text_clip)
        );
        translate_clips(&mut commands, geometry.origin);
        let backdrop_draws = commands
            .iter()
            .filter_map(|command| match command {
                Command::BackdropFilter(draw) => {
                    let mut draw = draw.clone();
                    if let Some(clip) = &mut draw.clip {
                        clip.x -= geometry.origin[0];
                        clip.y -= geometry.origin[1];
                    }
                    Some(draw)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
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
            &backdrop_draws,
            &group_scene.external_texture_draws,
            wgpu::Color::TRANSPARENT,
            backdrop_draws.is_empty(),
            &self.queue,
            false,
        );
        let text_clip_view = needs_text_clip.then(|| {
            self.render_mask_text_coverage(&mut encoder, width, height, &group_scene, &group_glyphs)
        });
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
            jag_draw::SurfaceEffect::MaskGroup(group) => self.pass.mask_group_surface(
                &mut encoder,
                &layer_view,
                width,
                height,
                geometry.origin,
                geometry.logical_size,
                &group,
                text_clip_view.as_ref(),
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

                    let layer_opacity = match &surface.effect {
                        jag_draw::SurfaceEffect::Opacity(opacity) => *opacity,
                        jag_draw::SurfaceEffect::Blur(_) => 1.0,
                        jag_draw::SurfaceEffect::ColorMatrix(_) => 1.0,
                        jag_draw::SurfaceEffect::DropShadow(_) => 1.0,
                        jag_draw::SurfaceEffect::Mask(_) => 1.0,
                        jag_draw::SurfaceEffect::MaskGroup(_) => 1.0,
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
                                surface.effect.clone(),
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
