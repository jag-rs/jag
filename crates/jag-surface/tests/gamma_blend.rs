//! A target that does not encode sRGB on write blends sRGB-encoded color, the
//! way browsers composite CSS: 5% white over black is 13/255, not the ~63 a
//! linear-light blend gives.

use std::sync::Arc;

use jag_draw::{Brush, ColorLinPremul, SrgbColor, wgpu};
use jag_surface::JagSurface;

fn surface(format: wgpu::TextureFormat) -> Option<JagSurface> {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .ok()?;
    let mut surface = JagSurface::new(Arc::new(device), Arc::new(queue), format);
    surface.set_frame_cache_enabled(false);
    Some(surface)
}

/// Red channel of a 5%-white fill and of a 50% opacity group of white, both
/// over opaque black.
fn render(format: wgpu::TextureFormat) -> Option<(u8, u8)> {
    let mut surface = surface(format)?;
    let mut canvas = surface.begin_frame(16, 8);
    canvas.clear(ColorLinPremul::from_srgba_u8([0, 0, 0, 255]));
    let faint_white = Brush::Solid(SrgbColor::rgba(255, 255, 255, 13).to_linear_premul());
    canvas.fill_rect(0.0, 0.0, 8.0, 8.0, faint_white, 1);
    canvas.push_opacity(0.5);
    let white = Brush::Solid(ColorLinPremul::from_srgba_u8([255, 255, 255, 255]));
    canvas.fill_rect(8.0, 0.0, 8.0, 8.0, white, 2);
    canvas.pop_opacity();
    let (width, _, pixels) = surface.end_frame_headless(canvas).ok()?;
    let red = |x: u32| pixels[((4 * width + x) * 4) as usize];
    Some((red(4), red(12)))
}

#[test]
fn unorm_target_blends_in_srgb_like_a_browser() {
    let Some((faint, half)) = render(wgpu::TextureFormat::Rgba8Unorm) else {
        return;
    };
    assert!(faint.abs_diff(13) <= 1, "5% white over black = {faint}");
    assert!(
        half.abs_diff(128) <= 1,
        "50% opacity white over black = {half}"
    );
}

#[test]
fn srgb_target_keeps_linear_light_blending() {
    let Some((faint, half)) = render(wgpu::TextureFormat::Rgba8UnormSrgb) else {
        return;
    };
    // Linear-light: 0.05 and 0.5 linear encode to about 63 and 188.
    assert!(faint.abs_diff(63) <= 2, "5% white over black = {faint}");
    assert!(
        half.abs_diff(188) <= 2,
        "50% opacity white over black = {half}"
    );
}
