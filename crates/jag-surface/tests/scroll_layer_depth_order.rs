//! Content painted inside a scroll layer keeps its depth against a layer
//! recorded after it: a fixed overlay at z 5 stays beneath a box at z 10
//! even when the overlay's layer comes last in the display list.

use std::sync::Arc;

use jag_draw::{Brush, ColorLinPremul, wgpu};
use jag_surface::JagSurface;

#[test]
fn later_scroll_layer_does_not_cover_higher_z_content() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        return;
    };
    let Ok((device, queue)) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
    else {
        return;
    };
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8Unorm,
    );
    surface.set_frame_cache_enabled(false);
    let mut canvas = surface.begin_frame(64, 64);
    canvas.clear(ColorLinPremul::from_srgba_u8([0, 0, 0, 255]));
    let solid =
        |rgb: [u8; 3]| Brush::Solid(ColorLinPremul::from_srgba_u8([rgb[0], rgb[1], rgb[2], 255]));

    canvas.push_scroll_layer("root", [0.0, 0.0]);
    canvas.fill_rect(0.0, 0.0, 64.0, 64.0, solid([0, 0, 0]), 1);
    canvas.fill_rect(16.0, 16.0, 32.0, 32.0, solid([255, 255, 255]), 10);
    canvas.push_bound_scroll_layer("root-fixed", "viewport", [0.0, 0.0], [1.0, 1.0]);
    canvas.fill_rect(0.0, 0.0, 64.0, 64.0, solid([255, 0, 0]), 5);
    canvas.pop_scroll_layer();
    canvas.pop_scroll_layer();

    let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    let at = |x: u32, y: u32| {
        let i = ((y * width + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    assert_eq!(
        at(32, 32),
        [255, 255, 255],
        "box must paint above the overlay"
    );
    assert_eq!(at(4, 4), [255, 0, 0], "overlay covers the body");
}
