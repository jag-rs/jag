use std::sync::Arc;

use jag_draw::{Brush, ColorLinPremul, wgpu};
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
