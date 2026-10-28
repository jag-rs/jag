//! Headless screenshots promise RGBA bytes for both RGBA and BGRA targets.
use jag_draw::{ColorLinPremul, wgpu};
use jag_surface::JagSurface;
use std::sync::Arc;
#[test]
fn headless_readback_normalizes_bgra_surface_formats() {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let (device, queue) = (Arc::new(device), Arc::new(queue));
    for format in [
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Bgra8UnormSrgb,
    ] {
        let mut surface = JagSurface::new(device.clone(), queue.clone(), format);
        for color in [[255, 0, 0, 255], [0, 255, 255, 255]] {
            let mut canvas = surface.begin_frame(3, 2);
            canvas.clear(ColorLinPremul::from_srgba_u8(color));
            let (w, h, bytes) = surface.end_frame_headless(canvas).unwrap();
            assert_eq!((w, h), (3, 2));
            assert_eq!(bytes.len(), 24);
            assert!(
                bytes.chunks_exact(4).all(|pixel| pixel == color),
                "{format:?}: {bytes:?}"
            );
        }
    }
}
