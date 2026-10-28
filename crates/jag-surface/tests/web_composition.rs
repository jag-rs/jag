//! Switching a shared host surface must restore native linear-light blending.
use jag_draw::{ColorLinPremul, ExternalTextureId, wgpu};
use jag_surface::JagSurface;
use std::sync::Arc;

#[test]
fn web_srgb_and_native_linear_modes_switch_without_losing_native_bindings() {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let (device, queue) = (Arc::new(device), Arc::new(queue));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("binding through blend-space transitions"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    for (format, extended, native_green) in [
        (wgpu::TextureFormat::Rgba8UnormSrgb, false, 188u8),
        (wgpu::TextureFormat::Rgba8Unorm, false, 128),
        (wgpu::TextureFormat::Rgba8Unorm, true, 188),
    ] {
        let mut surface = JagSurface::new(device.clone(), queue.clone(), format);
        if extended {
            surface.enable_extended_color_composition();
        }
        surface.set_frame_cache_enabled(false);
        let lifetime = Arc::new(());
        let owner = Arc::new(42usize);
        let lease = Arc::downgrade(&owner);
        surface
            .pass_manager()
            .register_linear_external_texture_with_lifetime(
                ExternalTextureId(19),
                texture.create_view(&Default::default()),
                owner,
                Arc::downgrade(&lifetime),
            );
        for (web, green) in [
            (false, native_green),
            (true, 128),
            (false, native_green),
            (true, 128),
        ] {
            surface.set_web_color_composition(web);
            assert_eq!(surface.pass_manager().external_texture_count(), 1);
            assert!(lease.upgrade().is_some());
            let mut canvas = surface.begin_frame(8, 8);
            canvas.clear(ColorLinPremul::rgba(255, 255, 255, 255));
            canvas.push_opacity(0.5);
            canvas.fill_rect(
                0.0,
                0.0,
                8.0,
                8.0,
                jag_draw::Brush::Solid(ColorLinPremul::rgba(255, 0, 0, 255)),
                0,
            );
            canvas.pop_opacity();
            let (_, _, pixels) = surface.end_frame_headless(canvas).unwrap();
            let actual = &pixels[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4];
            assert!(
                actual
                    .iter()
                    .zip([255, green, green, 255])
                    .all(|(a, b)| (*a as i16 - b as i16).abs() <= 2),
                "web={web}: {actual:?}"
            );
            surface.pass_manager().clear_compositor_textures();
        }
        drop(lifetime);
        assert_eq!(surface.pass_manager().prune_expired_external_textures(), 1);
        assert!(lease.upgrade().is_none());
    }
}
