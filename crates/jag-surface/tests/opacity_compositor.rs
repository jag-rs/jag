use std::sync::Arc;

use jag_draw::{Brush, ColorLinPremul, ColorMatrix, FilterEffect, wgpu};
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
fn color_matrix_filter_transforms_the_owned_surface() {
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
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
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
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }),
        1,
    );
    canvas.pop_filter();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let transformed = pixel(&pixels, width, 6, 6);
    assert!(
        transformed[0] < 5,
        "red channel should be removed: {transformed:?}"
    );
    assert!(
        transformed[2] > 250,
        "blue channel should receive red: {transformed:?}"
    );
    assert!(
        transformed[3] > 250,
        "alpha should be preserved: {transformed:?}"
    );
}
