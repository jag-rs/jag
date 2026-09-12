use jag_draw::{
    Brush, ColorLinPremul, GlyphMask, MaskFormat, RasterizedGlyph, Rect, SubpixelMask,
    TextProvider, TextRun, Transform2D, wgpu,
};
use jag_surface::JagSurface;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
struct ChangingFont {
    revision: AtomicU64,
}
impl TextProvider for ChangingFont {
    fn cache_tag(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }
    fn rasterize_run(&self, _: &TextRun) -> Vec<RasterizedGlyph> {
        let c = if self.cache_tag() == 0 { 100 } else { 210 };
        vec![RasterizedGlyph {
            offset: [0.0, 0.0],
            mask: GlyphMask::Subpixel(SubpixelMask {
                width: 8,
                height: 8,
                format: MaskFormat::Rgba8,
                data: [c, c, c, 0].repeat(64),
            }),
        }]
    }
}

fn surfaces() -> (JagSurface, JagSurface) {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default(), None)).expect("GPU device");
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let retained = JagSurface::new(
        device.clone(),
        queue.clone(),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    let mut fresh = JagSurface::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb);
    fresh.set_retained_layer_budget(0);
    (retained, fresh)
}

fn render(
    surface: &mut JagSurface,
    font: &Arc<ChangingFont>,
    dpr: f32,
    offset: f32,
    scroll: f32,
    alpha: f32,
) -> Vec<u8> {
    surface.set_dpi_scale(dpr);
    let mut canvas = surface.begin_frame((96.0 * dpr) as u32, (96.0 * dpr) as u32);
    canvas.set_text_provider(font.clone());
    canvas.clear(ColorLinPremul::from_srgba_u8([240, 240, 240, 255]));
    canvas.push_transform(Transform2D::translate(offset, offset));
    canvas.push_opacity(alpha);
    canvas.fill_rect(
        8.0,
        8.0,
        70.0,
        70.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([30, 40, 50, 255])),
        0,
    );
    canvas.push_clip_rect(Rect {
        x: 12.0,
        y: 12.0,
        w: 50.0,
        h: 50.0,
    });
    canvas.push_opacity(0.7);
    canvas.push_transform(Transform2D::translate(0.0, -scroll));
    canvas.fill_rect(
        16.0,
        16.0,
        40.0,
        50.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([180, 20, 70, 255])),
        1,
    );
    canvas.draw_text_run(
        [20.0, 24.0],
        "cached".into(),
        16.0,
        ColorLinPremul::from_srgba_u8([255; 4]),
        2,
    );
    canvas.pop_transform();
    canvas.pop_opacity();
    canvas.pop_clip();
    canvas.pop_opacity();
    canvas.pop_transform();
    let pixels = surface.end_frame_headless(canvas).unwrap().2;
    // The production shell drops transient registrations after presentation.
    surface.pass_manager().clear_external_textures();
    pixels
}

#[test]
fn nested_surfaces_reuse_pixels_and_invalidate_on_scroll_fonts_and_scale() {
    let (mut retained, mut fresh) = surfaces();
    let font = Arc::new(ChangingFont::default());
    for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let mut untranslated = Vec::new();
        let scenarios = [
            (0.0, 0.0, 0.9, false),
            (0.0, 0.0, 0.9, true),
            (4.0, 0.0, 0.9, true), // integer physical translation at every tested DPR
            (4.0, 0.0, 0.5, true), // opacity changes only the composite
            (4.0, 9.0, 0.5, false), // inner scroll invalidates child and parent
            (4.0, 9.0, 0.5, true),
        ];
        for (offset, scroll, alpha, reuse) in scenarios {
            let actual = render(&mut retained, &font, dpr, offset, scroll, alpha);
            let expected = render(&mut fresh, &font, dpr, offset, scroll, alpha);
            assert!(
                actual == expected,
                "pixels differ at DPR {dpr}, offset {offset}, scroll {scroll}, alpha {alpha}"
            );
            assert!(
                actual
                    .chunks_exact(4)
                    .any(|p| p[0] > p[1].saturating_add(20)),
                "child paint must be visible at DPR {dpr}, offset {offset}, scroll {scroll}, alpha {alpha}; max red-green {}",
                actual
                    .chunks_exact(4)
                    .map(|p| i16::from(p[0]) - i16::from(p[1]))
                    .max()
                    .unwrap()
            );
            if offset == 0.0 {
                untranslated = actual.clone();
            } else if scroll == 0.0 && alpha == 0.9 {
                let width = (96.0 * dpr) as usize;
                let delta = (4.0 * dpr) as usize;
                for y in 0..width - delta {
                    for x in 0..width - delta {
                        let old = (y * width + x) * 4;
                        let new = ((y + delta) * width + x + delta) * 4;
                        assert_eq!(
                            &untranslated[old..old + 4],
                            &actual[new..new + 4],
                            "clip did not translate with its layer at DPR {dpr}, ({x}, {y})"
                        );
                    }
                }
            }
            let stats = retained.retained_layer_stats();
            if reuse {
                assert_eq!(
                    stats.hits, 2,
                    "expected child and parent reuse at DPR {dpr}, offset {offset}, scroll {scroll}, alpha {alpha}: {stats:?}"
                );
                assert_eq!(stats.rasterized_pixels, 0);
            } else {
                assert!(stats.misses > 0, "expected invalidation: {stats:?}");
            }
        }
        font.revision.fetch_add(1, Ordering::Relaxed);
        assert_eq!(
            render(&mut retained, &font, dpr, 4.0, 9.0, 0.5),
            render(&mut fresh, &font, dpr, 4.0, 9.0, 0.5)
        );
        assert_eq!(
            retained.retained_layer_stats().misses,
            2,
            "font revision must invalidate both levels"
        );
    }
}

#[test]
fn changing_external_pixels_never_reuse_a_stale_parent_surface() {
    let (mut retained, _) = surfaces();
    let device = retained.device();
    let queue = retained.queue();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("changing-canvas"),
        size: wgpu::Extent3d {
            width: 8,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let id = jag_draw::ExternalTextureId(17);
    for color in [[220, 20, 20, 255], [20, 220, 20, 255]] {
        queue.write_texture(
            texture.as_image_copy(),
            &color.repeat(64),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(32),
                rows_per_image: Some(8),
            },
            wgpu::Extent3d {
                width: 8,
                height: 8,
                depth_or_array_layers: 1,
            },
        );
        retained
            .pass_manager()
            .register_external_texture(id, texture.create_view(&Default::default()));
        let mut canvas = retained.begin_frame(32, 32);
        canvas.push_opacity(1.0);
        canvas.push_opacity(1.0);
        canvas.external_texture(
            Rect {
                x: 4.0,
                y: 4.0,
                w: 8.0,
                h: 8.0,
            },
            id,
            0,
        );
        canvas.pop_opacity();
        canvas.pop_opacity();
        let pixels = retained.end_frame_headless(canvas).unwrap().2;
        let center = (8 * 32 + 8) * 4;
        assert_eq!(&pixels[center..center + 4], &color);
        assert_eq!(retained.retained_layer_stats().hits, 0);
        assert_eq!(retained.retained_layer_stats().bypassed, 2);
        retained.pass_manager().clear_external_textures();
    }
}

#[test]
fn retention_respects_memory_budget_and_can_be_disabled() {
    let (mut retained, _) = surfaces();
    retained.set_retained_layer_budget(16 * 1024);
    for frame in 0..6 {
        let mut canvas = retained.begin_frame(96, 96);
        for i in 0..12 {
            canvas.push_opacity(0.8);
            canvas.fill_rect(
                (i % 3) as f32 * 24.0,
                (i / 3) as f32 * 20.0,
                24.0,
                20.0,
                Brush::Solid(ColorLinPremul::from_srgba_u8([frame * 20, 40, 90, 255])),
                i,
            );
            canvas.pop_opacity();
        }
        retained.end_frame_headless(canvas).unwrap();
        retained.pass_manager().clear_external_textures();
        let stats = retained.retained_layer_stats();
        assert!(stats.resident_bytes <= 16 * 1024, "{stats:?}");
        assert!(stats.budget_usage_bytes <= 16 * 1024, "{stats:?}");
        // The visible working set is pinned during a frame. Overflow paints
        // transiently instead of evicting and re-rasterizing its own tiles.
        assert_eq!(stats.evictions, 0, "{stats:?}");
    }
    retained.set_retained_layer_budget(0);
    assert_eq!(retained.retained_layer_stats().resident_bytes, 0);
    let font = Arc::new(ChangingFont::default());
    render(&mut retained, &font, 1.0, 0.0, 0.0, 0.8);
    assert_eq!(retained.retained_layer_stats().resident_bytes, 0);
    assert_eq!(retained.retained_layer_stats().hits, 0);
}

#[cfg(target_os = "macos")]
#[test]
fn retina_working_set_above_32_mib_warms_without_repeated_rasterization() {
    let (mut retained, mut fresh) = surfaces();
    let render_large = |surface: &mut JagSurface| {
        let mut canvas = surface.begin_frame(1024, 1024);
        canvas.clear(ColorLinPremul::from_srgba_u8([255; 4]));
        for i in 0..12 {
            canvas.push_opacity(0.8);
            canvas.fill_rect(
                0.0,
                0.0,
                1024.0,
                1024.0,
                Brush::Solid(ColorLinPremul::from_srgba_u8([i * 20, 40, 90, 255])),
                i as i32,
            );
            canvas.pop_opacity();
        }
        let pixels = surface.end_frame_headless(canvas).unwrap().2;
        surface.pass_manager().clear_external_textures();
        pixels
    };
    let expected = render_large(&mut fresh);
    for frame in 0..4 {
        assert_eq!(render_large(&mut retained), expected, "frame {frame}");
        let stats = retained.retained_layer_stats();
        assert!(stats.budget_usage_bytes <= 128 * 1024 * 1024, "{stats:?}");
        if frame >= 2 {
            assert_eq!(stats.hits, 12, "{stats:?}");
            assert_eq!(stats.rasterized_pixels, 0, "{stats:?}");
            assert!(stats.resident_bytes > 32 * 1024 * 1024, "{stats:?}");
        }
    }
    // The same working set must respect a caller's fixed memory limit.
    retained.set_retained_layer_budget(32 * 1024 * 1024);
    for _ in 0..3 {
        assert_eq!(render_large(&mut retained), expected);
        let stats = retained.retained_layer_stats();
        assert!(stats.budget_usage_bytes <= 32 * 1024 * 1024, "{stats:?}");
        assert!(stats.misses > 0, "{stats:?}");
    }
}

#[test]
fn composite_slots_update_placement_depth_and_texture_without_aliasing_draws() {
    let (mut surface, _) = surfaces();
    let device = surface.device();
    let queue = surface.queue();
    let colors = [[220u8, 20, 20, 255], [20, 220, 20, 255]];
    let views: Vec<_> = colors
        .iter()
        .map(|color| {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("composite-slot-test"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            queue.write_texture(
                texture.as_image_copy(),
                color,
                wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(4),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            Arc::new(texture.create_view(&Default::default()))
        })
        .collect();
    let id = jag_draw::ExternalTextureId(7);
    for direct in [true, false] {
        surface.set_direct(direct);
        surface.set_use_intermediate(!direct);
        for frame in 0..6 {
            let color_index = frame / 3;
            surface
                .pass_manager()
                .register_shared_external_texture(id, views[color_index].clone());
            let x = (frame % 3 * 2) as f32;
            let mut canvas = surface.begin_frame(32, 24);
            canvas.clear(jag_draw::ColorLinPremul::from_srgba_u8([255; 4]));
            // Skipped registrations leave a hole in the uploaded vertex batch;
            // subsequent draws must use their original base-vertex offsets.
            canvas.external_texture(
                Rect {
                    x: 26.0,
                    y: 2.0,
                    w: 4.0,
                    h: 4.0,
                },
                jag_draw::ExternalTextureId(999),
                0,
            );
            // Same texture twice in one submission: updating the second draw
            // must not overwrite the first draw's vertices or depth uniform.
            for y in [2.0, 14.0] {
                canvas.external_texture(
                    Rect {
                        x,
                        y,
                        w: 8.0,
                        h: 8.0,
                    },
                    id,
                    frame as i32,
                );
            }
            let pixels = surface.end_frame_headless(canvas).unwrap().2;
            for y in [4usize, 16] {
                let offset = (y * 32 + x as usize + 2) * 4;
                assert_eq!(
                    &pixels[offset..offset + 4],
                    &colors[color_index],
                    "frame {frame}, direct {direct}"
                );
            }
            let gap = (11 * 32 + x as usize + 2) * 4;
            assert_eq!(&pixels[gap..gap + 4], &[255; 4]);
            surface.pass_manager().clear_external_textures();
        }
    }
    drop(views);
    surface.pass_manager().clear_external_textures();
}
