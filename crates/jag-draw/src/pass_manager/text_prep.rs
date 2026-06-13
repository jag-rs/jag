//! Text-atlas upload and per-(z, clip) text-group GPU resource preparation for
//! `render_unified`.
//!
//! Verbatim extractions of the text grouping + atlas-upload + buffer/bind-group
//! build that previously lived inline in `render_unified`. Returns fully owned
//! resource vectors so they outlive the render pass. No logic changed.

use super::{PassManager, TextQuadVtx, glyph_mask_for_atlas};

type TextResource = (
    i32,
    wgpu::Buffer,
    wgpu::Buffer,
    u32,
    wgpu::BindGroup,
    wgpu::Buffer,
    Option<crate::Rect>,
);

type GlyphDraw = (
    [f32; 2],
    crate::text::RasterizedGlyph,
    crate::ColorLinPremul,
    i32,
    Option<crate::Rect>,
);

#[allow(clippy::type_complexity, clippy::let_and_return)]
impl PassManager {
    pub(super) fn prep_text_direct(
        &mut self,
        glyph_draws: &[GlyphDraw],
        transparent_text_z: &std::collections::HashSet<i32>,
        inv_logical: f32,
        queue: &wgpu::Queue,
    ) -> Vec<TextResource> {
        // Group text by (z-index, clip rect) for proper depth rendering
        // and per-clip-region scissor rects.
        //
        // We use a Vec of groups instead of a HashMap because Option<Rect>
        // (with f32 fields) cannot be hashed. Groups with the same (z, clip)
        // are merged by scanning linearly.
        struct TextGroup<'a> {
            z: i32,
            clip: Option<crate::Rect>,
            glyphs: Vec<(
                usize,
                [f32; 2],
                &'a crate::text::RasterizedGlyph,
                &'a crate::ColorLinPremul,
            )>,
        }
        let mut text_groups_by_zclip: Vec<TextGroup<'_>> = Vec::new();
        for (idx, (origin, glyph, color, z, clip)) in glyph_draws.iter().enumerate() {
            // Try to merge with an existing group at the same (z, clip).
            let found = text_groups_by_zclip
                .iter_mut()
                .find(|g| g.z == *z && g.clip == *clip);
            if let Some(group) = found {
                group.glyphs.push((idx, *origin, glyph, color));
            } else {
                text_groups_by_zclip.push(TextGroup {
                    z: *z,
                    clip: *clip,
                    glyphs: vec![(idx, *origin, glyph, color)],
                });
            }
        }
        // eprintln!("🎨 Grouped text into {} z-index groups", text_by_z.len());

        // Prepare text rendering data before render pass
        let text_groups = if !glyph_draws.is_empty() {
            let mut atlas_cursor_x = 0u32;
            let mut atlas_cursor_y = 0u32;
            let mut next_row_height = 0u32;
            let mut atlas_max_x = 0u32;
            let mut atlas_max_y = 0u32;
            let mut all_text_groups: Vec<(i32, Option<crate::Rect>, Vec<TextQuadVtx>)> = Vec::new();

            // Process each (z-index, clip) group
            for tg in text_groups_by_zclip.iter() {
                let z_index = &tg.z;
                let glyphs = &tg.glyphs;
                let mut vertices: Vec<TextQuadVtx> = Vec::new();
                let force_grayscale = transparent_text_z.contains(z_index);
                // eprintln!("      🔠 Processing z={} with {} glyphs", z_index, glyphs.len());

                let mut local_idx = 0;
                for (_idx, origin, glyph, color) in glyphs.iter() {
                    let (w, h, data) = glyph_mask_for_atlas(&glyph.mask, force_grayscale);
                    if local_idx == 0 {
                        // eprintln!("        🔤 First glyph: origin=[{:.1}, {:.1}], size=[{}, {}], color=[{:.3}, {:.3}, {:.3}, {:.3}]",
                        //     origin[0], origin[1], w, h, color.r, color.g, color.b, color.a);
                    }
                    local_idx += 1;

                    if atlas_cursor_x + w >= 4096 {
                        atlas_cursor_x = 0;
                        atlas_cursor_y += next_row_height;
                        next_row_height = 0;
                    }
                    next_row_height = next_row_height.max(h);

                    // Track maximum atlas region used for clearing next frame
                    atlas_max_x = atlas_max_x.max(atlas_cursor_x + w);
                    atlas_max_y = atlas_max_y.max(atlas_cursor_y + h);

                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &self.text_mask_atlas,
                            mip_level: 0,
                            origin: wgpu::Origin3d {
                                x: atlas_cursor_x,
                                y: atlas_cursor_y,
                                z: 0,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        data.as_ref(),
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some(w * 4),
                            rows_per_image: Some(h),
                        },
                        wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                    );

                    let u0 = atlas_cursor_x as f32 / 4096.0;
                    let v0 = atlas_cursor_y as f32 / 4096.0;
                    let u1 = (atlas_cursor_x + w) as f32 / 4096.0;
                    let v1 = (atlas_cursor_y + h) as f32 / 4096.0;

                    // Glyph masks are rasterized at *physical* pixel size. To avoid
                    // scaling them again during composition, convert their size into
                    // logical scene units so that the subsequent logical->device
                    // scaling in the viewport transform maps them 1:1 to the atlas.
                    let quad_w = (w as f32) * inv_logical;
                    let quad_h = (h as f32) * inv_logical;

                    if local_idx == 1 {
                        // eprintln!("        📐 Atlas pos: cursor=({}, {}), uv=[{:.4}, {:.4}] to [{:.4}, {:.4}]",
                        //     atlas_cursor_x, atlas_cursor_y, u0, v0, u1, v1);
                    }

                    vertices.extend_from_slice(&[
                        TextQuadVtx {
                            pos: [origin[0], origin[1]],
                            uv: [u0, v0],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0] + quad_w, origin[1]],
                            uv: [u1, v0],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0] + quad_w, origin[1] + quad_h],
                            uv: [u1, v1],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0], origin[1] + quad_h],
                            uv: [u0, v1],
                            color: [color.r, color.g, color.b, color.a],
                        },
                    ]);

                    atlas_cursor_x += w;
                }

                // Store vertices for this (z-index, clip) group
                if !vertices.is_empty() {
                    all_text_groups.push((*z_index, tg.clip, vertices));
                }
            }

            // Create buffers and bind groups for each text group
            // eprintln!("🔧 all_text_groups.len() = {}", all_text_groups.len());
            let mut text_resources: Vec<(
                i32,
                wgpu::Buffer,
                wgpu::Buffer,
                u32,
                wgpu::BindGroup,
                wgpu::Buffer,
                Option<crate::Rect>,
            )> = Vec::new();
            for (z_index, clip, vertices) in all_text_groups {
                // eprintln!(
                //     "  🛠️  Creating resources for z={}, vertices={}",
                //     z_index,
                //     vertices.len()
                // );
                let quad_count = vertices.len() / 4;
                let mut indices: Vec<u16> = Vec::with_capacity(quad_count * 6);
                for i in 0..quad_count {
                    let base = (i * 4) as u16;
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }

                // Create vertex buffer for this group
                let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("text-vertex-buffer-group"),
                    size: (vertices.len() * std::mem::size_of::<TextQuadVtx>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                // Create index buffer for this group
                let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("text-index-buffer-group"),
                    size: (indices.len() * std::mem::size_of::<u16>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&vertices));
                queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&indices));

                // Create z bind group for this text group
                // Pass z_index as float directly - shader will convert to depth
                // eprintln!("    💎 z={} (passing as z-index to shader)", z_index);
                let (z_bg, z_buf) = self.create_group_z_bind_group(z_index as f32, queue);

                text_resources.push((z_index, vbuf, ibuf, indices.len() as u32, z_bg, z_buf, clip));
            }

            // Store atlas usage for next frame's clearing
            self.prev_atlas_max_x = atlas_max_x;
            self.prev_atlas_max_y = atlas_max_y;

            text_resources
        } else {
            Vec::new()
        };
        text_groups
    }

    pub(super) fn prep_text_offscreen(
        &mut self,
        glyph_draws: &[GlyphDraw],
        transparent_text_z: &std::collections::HashSet<i32>,
        inv_logical: f32,
        queue: &wgpu::Queue,
    ) -> Vec<TextResource> {
        // Group text by (z-index, clip) for proper depth rendering (offscreen path)
        struct TextGroupOff<'a> {
            z: i32,
            clip: Option<crate::Rect>,
            glyphs: Vec<(
                usize,
                [f32; 2],
                &'a crate::text::RasterizedGlyph,
                &'a crate::ColorLinPremul,
            )>,
        }
        let mut text_groups_by_zclip_off: Vec<TextGroupOff<'_>> = Vec::new();
        for (idx, (origin, glyph, color, z, clip)) in glyph_draws.iter().enumerate() {
            let found = text_groups_by_zclip_off
                .iter_mut()
                .find(|g| g.z == *z && g.clip == *clip);
            if let Some(group) = found {
                group.glyphs.push((idx, *origin, glyph, color));
            } else {
                text_groups_by_zclip_off.push(TextGroupOff {
                    z: *z,
                    clip: *clip,
                    glyphs: vec![(idx, *origin, glyph, color)],
                });
            }
        }

        // Prepare text rendering data (same as direct path)
        let text_groups_off = if !glyph_draws.is_empty() {
            let mut atlas_cursor_x = 0u32;
            let mut atlas_cursor_y = 0u32;
            let mut next_row_height = 0u32;
            let mut atlas_max_x = 0u32;
            let mut atlas_max_y = 0u32;
            let atlas_width = 4096usize;
            let atlas_row_stride = atlas_width * 4;
            let mut all_text_groups: Vec<(i32, Option<crate::Rect>, Vec<TextQuadVtx>)> = Vec::new();

            // Process each (z-index, clip) group
            for tg in text_groups_by_zclip_off.iter() {
                let z_index = &tg.z;
                let glyphs = &tg.glyphs;
                let mut vertices: Vec<TextQuadVtx> = Vec::new();
                let force_grayscale = transparent_text_z.contains(z_index);

                for (_idx, origin, glyph, color) in glyphs.iter() {
                    let (w, h, data) = glyph_mask_for_atlas(&glyph.mask, force_grayscale);

                    if atlas_cursor_x + w >= 4096 {
                        atlas_cursor_x = 0;
                        atlas_cursor_y += next_row_height;
                        next_row_height = 0;
                    }
                    next_row_height = next_row_height.max(h);

                    // Track maximum atlas region used for clearing next frame
                    atlas_max_x = atlas_max_x.max(atlas_cursor_x + w);
                    atlas_max_y = atlas_max_y.max(atlas_cursor_y + h);

                    let glyph_width_bytes = (w as usize) * 4;
                    let dst_x_bytes = (atlas_cursor_x as usize) * 4;
                    let dst_y = atlas_cursor_y as usize;
                    let rows = h as usize;
                    let required_len = (dst_y + rows)
                        .saturating_mul(atlas_row_stride)
                        .min(atlas_row_stride * atlas_width);
                    if self.text_atlas_upload.len() < required_len {
                        self.text_atlas_upload.resize(required_len, 0);
                    }
                    let glyph_data = data.as_ref();
                    for row in 0..rows {
                        let src_start = row * glyph_width_bytes;
                        let dst_start = (dst_y + row) * atlas_row_stride + dst_x_bytes;
                        let dst_end = dst_start + glyph_width_bytes;
                        if dst_end <= self.text_atlas_upload.len()
                            && src_start + glyph_width_bytes <= glyph_data.len()
                        {
                            self.text_atlas_upload[dst_start..dst_end].copy_from_slice(
                                &glyph_data[src_start..src_start + glyph_width_bytes],
                            );
                        }
                    }

                    let u0 = atlas_cursor_x as f32 / 4096.0;
                    let v0 = atlas_cursor_y as f32 / 4096.0;
                    let u1 = (atlas_cursor_x + w) as f32 / 4096.0;
                    let v1 = (atlas_cursor_y + h) as f32 / 4096.0;

                    // Convert glyph bitmap size from physical pixels into logical
                    // scene units so that the viewport scale maps them back to
                    // physical pixels without additional filtering.
                    let quad_w = (w as f32) * inv_logical;
                    let quad_h = (h as f32) * inv_logical;

                    vertices.extend_from_slice(&[
                        TextQuadVtx {
                            pos: [origin[0], origin[1]],
                            uv: [u0, v0],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0] + quad_w, origin[1]],
                            uv: [u1, v0],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0] + quad_w, origin[1] + quad_h],
                            uv: [u1, v1],
                            color: [color.r, color.g, color.b, color.a],
                        },
                        TextQuadVtx {
                            pos: [origin[0], origin[1] + quad_h],
                            uv: [u0, v1],
                            color: [color.r, color.g, color.b, color.a],
                        },
                    ]);

                    atlas_cursor_x += w;
                }

                // Store vertices for this (z-index, clip) group
                if !vertices.is_empty() {
                    all_text_groups.push((*z_index, tg.clip, vertices));
                }
            }

            if atlas_max_y > 0 {
                let upload_height = atlas_max_y.min(4096);
                let upload_len = (upload_height as usize) * atlas_row_stride;
                if self.text_atlas_upload.len() >= upload_len {
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &self.text_mask_atlas,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &self.text_atlas_upload[..upload_len],
                        wgpu::ImageDataLayout {
                            offset: 0,
                            bytes_per_row: Some((atlas_row_stride) as u32),
                            rows_per_image: Some(upload_height),
                        },
                        wgpu::Extent3d {
                            width: 4096,
                            height: upload_height,
                            depth_or_array_layers: 1,
                        },
                    );
                }
            }

            // Create buffers and bind groups for each text group
            let mut text_resources: Vec<(
                i32,
                wgpu::Buffer,
                wgpu::Buffer,
                u32,
                wgpu::BindGroup,
                wgpu::Buffer,
                Option<crate::Rect>,
            )> = Vec::new();
            for (z_index, clip, vertices) in all_text_groups {
                let quad_count = vertices.len() / 4;
                let mut indices: Vec<u16> = Vec::with_capacity(quad_count * 6);
                for i in 0..quad_count {
                    let base = (i * 4) as u16;
                    indices.extend_from_slice(&[
                        base,
                        base + 1,
                        base + 2,
                        base,
                        base + 2,
                        base + 3,
                    ]);
                }

                // Create vertex buffer for this group
                let vbuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("text-vertex-buffer-group-off"),
                    size: (vertices.len() * std::mem::size_of::<TextQuadVtx>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                // Create index buffer for this group
                let ibuf = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("text-index-buffer-group-off"),
                    size: (indices.len() * std::mem::size_of::<u16>()) as u64,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });

                queue.write_buffer(&vbuf, 0, bytemuck::cast_slice(&vertices));
                queue.write_buffer(&ibuf, 0, bytemuck::cast_slice(&indices));

                // Create z bind group for this text group
                // Pass z_index as float directly - shader will convert to depth
                let (z_bg, z_buf) = self.create_group_z_bind_group(z_index as f32, queue);

                text_resources.push((z_index, vbuf, ibuf, indices.len() as u32, z_bg, z_buf, clip));
            }

            // Store atlas usage for next frame's clearing
            self.prev_atlas_max_x = atlas_max_x;
            self.prev_atlas_max_y = atlas_max_y;

            text_resources
        } else {
            Vec::new()
        };
        text_groups_off
    }
}
