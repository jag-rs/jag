//! Extended linear colors must reach page blending before SDR projection.
use jag_draw::{Brush, ColorLinPremul, ExternalTextureId, Rect, wgpu};
use jag_surface::JagSurface;
use std::sync::Arc;

fn render(group: bool, format: wgpu::TextureFormat, background: bool) -> [u8; 4] {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .expect("native GPU required for composition regression");
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let (device, queue) = (Arc::new(device), Arc::new(queue));
    let texture = Arc::new(device.create_texture(&wgpu::TextureDescriptor {
        label: Some("extended linear source"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    }));
    // Exact half-float [-0.125, 0.5, -0.0625, 0.5], premultiplied linear RGBA.
    let bytes = [0xb000u16, 0x3800, 0xac00, 0x3800]
        .map(u16::to_le_bytes)
        .concat();
    queue.write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: Some(1),
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    let mut surface = JagSurface::new(device, queue, format);
    surface.set_frame_cache_enabled(false);
    let id = ExternalTextureId(42);
    // Bindings registered before a precision upgrade must survive it.
    surface
        .pass_manager()
        .register_linear_external_texture(id, texture.create_view(&Default::default()));
    let image_key = std::path::Path::new("test://precision-cache-image");
    surface
        .pass_manager()
        .store_loaded_image(image_key, texture.clone(), 1, 1);
    surface.enable_extended_color_composition();
    surface.enable_extended_color_composition();
    assert!(surface.pass_manager().is_image_ready(image_key));
    assert_eq!(surface.surface_format(), format);
    let mut canvas = surface.begin_frame(8, 8);
    canvas.clear(ColorLinPremul::default());
    if background {
        canvas.fill_rect(
            0.0,
            0.0,
            8.0,
            8.0,
            Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
            0,
        );
    }
    if group {
        canvas.push_opacity(0.5);
    }
    // A skipped binding must not shift the valid draw's scissor association.
    canvas.push_clip_rect(Rect {
        x: 0.0,
        y: 0.0,
        w: 2.0,
        h: 8.0,
    });
    canvas.external_texture_with_params(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 8.0,
            h: 8.0,
        },
        ExternalTextureId(999),
        1,
        1.0,
        true,
    );
    canvas.pop_clip();
    canvas.push_clip_rect(Rect {
        x: 4.0,
        y: 0.0,
        w: 4.0,
        h: 8.0,
    });
    canvas.external_texture_with_params(
        Rect {
            x: 0.0,
            y: 0.0,
            w: 8.0,
            h: 8.0,
        },
        id,
        1,
        1.0,
        true,
    );
    canvas.pop_clip();
    if group {
        canvas.pop_opacity();
    }
    let (_, _, pixels) = surface.end_frame_headless(canvas).unwrap();
    assert!(surface.pass_manager().is_image_ready(image_key));
    assert_eq!(surface.surface_format(), format);
    near(
        pixels[(4 * 8 + 2) * 4..(4 * 8 + 2) * 4 + 4]
            .try_into()
            .unwrap(),
        if background { [255; 4] } else { [0; 4] },
    );
    pixels[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4]
        .try_into()
        .unwrap()
}
fn near(actual: [u8; 4], expected: [u8; 4]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, b)| (*a as i32 - b as i32).abs() <= 3),
        "actual {actual:?}, expected {expected:?}"
    );
}
#[test]
fn extended_root_blending_preserves_negative_channels_and_existing_bindings() {
    for format in [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Rgba8Unorm,
    ] {
        near(render(false, format, true), [165, 255, 177, 255]);
    }
}
#[test]
fn extended_effect_groups_preserve_gamut_before_parent_blending() {
    for format in [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Rgba8Unorm,
    ] {
        near(render(true, format, true), [216, 255, 221, 255]);
    }
}

#[test]
fn extended_transparent_output_matches_encoded_premultiplied_rgba_on_both_targets() {
    for format in [
        wgpu::TextureFormat::Rgba8UnormSrgb,
        wgpu::TextureFormat::Rgba8Unorm,
    ] {
        near(render(false, format, false), [0, 188, 0, 128]);
        near(render(true, format, false), [0, 137, 0, 64]);
    }
}
