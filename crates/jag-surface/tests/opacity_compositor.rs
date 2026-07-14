use std::sync::Arc;

use jag_draw::{
    Brush, ColorLinPremul, ColorMatrix, DropShadow, ExternalTextureId, FilterEffect, MaskEffect,
    MaskMode, SrgbColor, wgpu,
};
use jag_surface::JagSurface;

fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * width + x) * 4) as usize;
    pixels[offset..offset + 4].try_into().unwrap()
}

fn assert_alpha_near(actual: [u8; 4], expected: u8) {
    assert!(
        actual[3].abs_diff(expected) <= 3,
        "expected alpha near {expected}, got pixel {actual:?}"
    );
}

#[test]
fn overlapping_and_nested_descendants_composite_each_group_once() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
    else {
        eprintln!("skipping compositor render test: no GPU adapter available");
        return;
    };
    let Ok((device, queue)) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
    else {
        eprintln!("skipping compositor render test: GPU device unavailable");
        return;
    };

    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(24, 12);
    canvas.clear(ColorLinPremul::default());

    let red = Brush::Solid(ColorLinPremul {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    });
    let blue = Brush::Solid(ColorLinPremul {
        r: 0.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    });
    canvas.push_opacity(0.5);
    canvas.fill_rect(2.0, 2.0, 8.0, 8.0, red.clone(), 1);
    canvas.fill_rect(6.0, 2.0, 8.0, 8.0, red, 2);
    canvas.push_opacity(0.5);
    canvas.fill_rect(16.0, 2.0, 4.0, 8.0, blue, 3);
    canvas.pop_opacity();
    canvas.pop_opacity();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let single_red = pixel(&pixels, width, 3, 4);
    let overlapping_red = pixel(&pixels, width, 8, 4);
    let nested_blue = pixel(&pixels, width, 18, 4);

    assert_alpha_near(single_red, 128);
    assert_alpha_near(overlapping_red, 128);
    assert_alpha_near(nested_blue, 64);
}

#[test]
fn blur_filter_spreads_surface_alpha_beyond_descendant_ink() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
    else {
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap();
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(32, 16);
    canvas.clear(ColorLinPremul::default());
    canvas.push_filter(FilterEffect::Blur(1.5));
    canvas.fill_rect(
        10.0,
        4.0,
        12.0,
        8.0,
        Brush::Solid(ColorLinPremul {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        }),
        1,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let far = pixel(&pixels, width, 1, 8)[3];
    let halo = pixel(&pixels, width, 8, 8)[3];
    let center = pixel(&pixels, width, 16, 8)[3];
    assert!(far <= 2, "far pixel should remain transparent, got {far}");
    assert!(
        halo > 2,
        "blur should spread alpha beyond source ink, got {halo}"
    );
    assert!(center > halo, "center {center} should exceed halo {halo}");
}

#[test]
fn color_matrix_filter_uses_srgb_and_preserves_alpha() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
    else {
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap();
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(12, 12);
    canvas.clear(ColorLinPremul::default());
    canvas.push_filter(FilterEffect::ColorMatrix(ColorMatrix {
        rows: [
            [0.5, 0.0, 0.0, 0.0],
            [0.0, 0.5, 0.0, 0.0],
            [0.0, 0.0, 0.5, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
        bias: [0.0; 4],
    }));
    canvas.fill_rect(
        2.0,
        2.0,
        8.0,
        8.0,
        Brush::Solid(ColorLinPremul {
            r: 0.25,
            g: 0.25,
            b: 0.25,
            a: 1.0,
        }),
        1,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let transformed = pixel(&pixels, width, 6, 6);
    assert!(
        transformed[..3]
            .iter()
            .all(|channel| (65..=72).contains(channel)),
        "sRGB brightness should produce channels near 69: {transformed:?}"
    );
    assert!(
        transformed[3] > 250,
        "alpha should be preserved: {transformed:?}"
    );
}

#[test]
fn drop_shadow_keeps_source_above_shifted_tinted_alpha() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap();
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(32, 16);
    canvas.clear(ColorLinPremul::default());
    canvas.push_filter(FilterEffect::DropShadow(DropShadow {
        offset: [8.0, 0.0],
        blur_radius: 0.0,
        color: SrgbColor::rgba(0, 0, 255, 255),
    }));
    canvas.fill_rect(
        4.0,
        4.0,
        6.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
        1,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let source = pixel(&pixels, width, 6, 8);
    let gap = pixel(&pixels, width, 11, 8);
    let shadow = pixel(&pixels, width, 15, 8);
    assert!(
        source[0] > 250 && source[2] < 5,
        "source changed: {source:?}"
    );
    assert!(gap[3] < 5, "zero blur should keep a sharp gap: {gap:?}");
    assert!(
        shadow[2] > 240 && shadow[3] > 240,
        "shifted shadow missing: {shadow:?}"
    );
}

#[test]
fn backdrop_filter_snapshots_before_later_transparent_content() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap();
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(24, 16);
    canvas.clear(ColorLinPremul::default());
    canvas.fill_rect(
        0.0,
        0.0,
        24.0,
        16.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255, 0, 0, 255])),
        1,
    );
    canvas.backdrop_filter_rect(
        jag_draw::Rect {
            x: 4.0,
            y: 2.0,
            w: 16.0,
            h: 12.0,
        },
        vec![
            FilterEffect::ColorMatrix(ColorMatrix {
                rows: [
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                bias: [0.0; 4],
            }),
            FilterEffect::ColorMatrix(ColorMatrix {
                rows: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
                bias: [0.25, 0.0, 0.0, 0.0],
            }),
        ],
        2,
    );
    canvas.fill_rect(
        12.0,
        2.0,
        8.0,
        12.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([0, 255, 0, 254])),
        3,
    );

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let filtered = pixel(&pixels, width, 8, 8);
    let later = pixel(&pixels, width, 16, 8);
    assert!(filtered[2] > 250, "channel swap missing: {filtered:?}");
    assert!(
        (60..=68).contains(&filtered[0]),
        "chain order wrong: {filtered:?}"
    );
    assert!(later[1] > 250, "later content was filtered: {later:?}");
}

#[test]
fn resolved_texture_mask_applies_alpha_and_luminance_coverage() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .unwrap();
    let device = Arc::new(device);
    let queue = Arc::new(queue);
    let mask_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resolved-mask-test"),
        size: wgpu::Extent3d {
            width: 4,
            height: 8,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mask_pixels = [255, 0, 0, 128].repeat(32);
    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &mask_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &mask_pixels,
        wgpu::ImageDataLayout {
            offset: 0,
            bytes_per_row: Some(16),
            rows_per_image: Some(8),
        },
        wgpu::Extent3d {
            width: 4,
            height: 8,
            depth_or_array_layers: 1,
        },
    );

    let mut surface = JagSurface::new(device, queue, wgpu::TextureFormat::Rgba8UnormSrgb);
    surface.set_frame_cache_enabled(false);
    let mask_id = ExternalTextureId(42);
    surface
        .pass_manager()
        .register_external_texture(mask_id, mask_texture.create_view(&Default::default()));
    let mut canvas = surface.begin_frame(32, 8);
    canvas.clear(ColorLinPremul::default());
    for (x, mode, z) in [(0.0, MaskMode::Alpha, 1), (4.0, MaskMode::Luminance, 2)] {
        canvas.push_filter(FilterEffect::Mask(MaskEffect {
            texture_id: mask_id,
            mode,
            rect: jag_draw::Rect {
                x,
                y: 0.0,
                w: 4.0,
                h: 8.0,
            },
        }));
        canvas.fill_rect(
            x,
            0.0,
            4.0,
            8.0,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
            z,
        );
        canvas.pop_filter();
    }
    canvas.push_filter(FilterEffect::Mask(MaskEffect {
        texture_id: mask_id,
        mode: MaskMode::Alpha,
        rect: jag_draw::Rect {
            x: 8.0,
            y: 0.0,
            w: 2.0,
            h: 8.0,
        },
    }));
    canvas.fill_rect(
        8.0,
        0.0,
        4.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        3,
    );
    canvas.pop_filter();
    assert!(canvas.push_generated_mask_pattern(
        jag_draw::Rect {
            x: 24.0,
            y: 0.0,
            w: 8.0,
            h: 8.0,
        },
        jag_draw::Rect {
            x: 24.0,
            y: 0.0,
            w: 2.0,
            h: 8.0,
        },
        [4.0, 8.0],
        [true, false],
        &Brush::LinearGradient {
            start: [24.0, 0.0],
            end: [26.0, 0.0],
            stops: vec![
                (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
            ],
        },
        MaskMode::Alpha,
    ));
    canvas.fill_rect(
        24.0,
        0.0,
        8.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        7,
    );
    canvas.pop_filter();
    for (rect, brush, z) in [
        (
            jag_draw::Rect {
                x: 16.0,
                y: 0.0,
                w: 4.0,
                h: 8.0,
            },
            Brush::RadialGradient {
                center: [18.0, 4.0],
                radius: 2.0,
                stops: vec![
                    (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                    (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                ],
            },
            5,
        ),
        (
            jag_draw::Rect {
                x: 20.0,
                y: 0.0,
                w: 4.0,
                h: 8.0,
            },
            Brush::ConicGradient {
                center: [22.0, 4.0],
                start_angle: 0.0,
                stops: vec![
                    (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                    (0.5, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                    (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                ],
            },
            6,
        ),
    ] {
        assert!(canvas.push_generated_mask(rect, &brush, MaskMode::Alpha));
        canvas.fill_rect(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
            z,
        );
        canvas.pop_filter();
    }
    assert!(canvas.push_generated_mask(
        jag_draw::Rect {
            x: 12.0,
            y: 0.0,
            w: 4.0,
            h: 8.0,
        },
        &Brush::LinearGradient {
            start: [12.0, 4.0],
            end: [16.0, 4.0],
            stops: vec![
                (0.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 0])),
                (1.0, ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
            ],
        },
        MaskMode::Alpha,
    ));
    canvas.fill_rect(
        12.0,
        0.0,
        4.0,
        8.0,
        Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
        4,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    assert_alpha_near(pixel(&pixels, width, 2, 4), 128);
    assert_alpha_near(pixel(&pixels, width, 6, 4), 27);
    assert_alpha_near(pixel(&pixels, width, 9, 4), 128);
    assert_alpha_near(pixel(&pixels, width, 11, 4), 0);
    assert!(pixel(&pixels, width, 12, 4)[3] < 64);
    assert!(pixel(&pixels, width, 15, 4)[3] > 190);
    assert!(pixel(&pixels, width, 18, 4)[3] < 100);
    assert!(pixel(&pixels, width, 16, 4)[3] > 160);
    let conic_top = pixel(&pixels, width, 22, 1)[3];
    let conic_bottom = pixel(&pixels, width, 22, 6)[3];
    assert!(
        conic_top < 80 && conic_bottom > 160,
        "conic alpha top/bottom: {conic_top}/{conic_bottom}"
    );
    assert!(pixel(&pixels, width, 24, 4)[3] > 240);
    assert!(pixel(&pixels, width, 27, 4)[3] < 10);
    assert!(pixel(&pixels, width, 28, 4)[3] > 240);
}
