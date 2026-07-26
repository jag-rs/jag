//! Unit tests for effect-group layer geometry and clip localization.
//!
//! Kept beside the implementation as a private child module so the tests
//! can reach `layer_geometry` and `localize_clips` without exporting them.

use super::*;

#[test]
fn rebuilt_frames_reuse_effect_texture_ids() {
    let instance = wgpu::Instance::default();
    let Some(adapter) =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("skipping effect texture ID test: no GPU adapter available");
        return;
    };
    let Ok((device, queue)) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
    else {
        eprintln!("skipping effect texture ID test: GPU device unavailable");
        return;
    };
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_frame_cache_enabled(false);

    let render_effect_frame = |surface: &mut JagSurface| {
        let mut canvas = surface.begin_frame(8, 8);
        canvas.push_opacity(1.0);
        canvas.fill_rect(
            0.0,
            0.0,
            8.0,
            8.0,
            jag_draw::Brush::Solid(jag_draw::ColorLinPremul::from_srgba_u8([255; 4])),
            1,
        );
        canvas.pop_opacity();
        surface.end_frame_headless(canvas).unwrap();
        surface.next_synthetic_external_texture_id
    };

    let after_first = render_effect_frame(&mut surface);
    let after_second = render_effect_frame(&mut surface);
    assert_eq!(after_first, SYNTHETIC_EXTERNAL_TEXTURE_ID_START + 1);
    assert_eq!(after_second, after_first);
}

#[test]
fn layer_bounds_align_outward_to_device_pixels() {
    let geometry = layer_geometry(
        Rect {
            x: 10.25,
            y: 20.75,
            w: 5.5,
            h: 4.5,
        },
        Viewport {
            width: 200,
            height: 200,
        },
        2.0,
    )
    .unwrap();
    assert_eq!(geometry.origin, [10.0, 20.5]);
    assert_eq!(geometry.logical_size, [6.0, 5.0]);
    assert_eq!(geometry.pixel_size, [12, 10]);
}

#[test]
fn layer_bounds_are_clamped_to_the_viewport() {
    let geometry = layer_geometry(
        Rect {
            x: -5.0,
            y: 8.0,
            w: 20.0,
            h: 10.0,
        },
        Viewport {
            width: 10,
            height: 12,
        },
        1.0,
    )
    .unwrap();
    assert_eq!(geometry.origin, [0.0, 8.0]);
    assert_eq!(geometry.logical_size, [10.0, 4.0]);
    assert_eq!(geometry.pixel_size, [10, 4]);
}

#[test]
fn transformed_clips_are_normalized_into_layer_space() {
    let mut commands = vec![
        Command::PushTransform(Transform2D {
            m: [2.0, 0.0, 0.0, 3.0, 10.0, 15.0],
        }),
        Command::PushClip(jag_draw::ClipRect(Rect {
            x: 1.0,
            y: 1.0,
            w: 5.0,
            h: 6.0,
        })),
        Command::PopTransform,
    ];
    localize_clips(&mut commands, [10.0, 15.0], 1.0);
    let Command::PushClip(clip) = &commands[1] else {
        unreachable!()
    };
    assert_eq!(
        clip.0,
        Rect {
            x: 2.0,
            y: 3.0,
            w: 10.0,
            h: 18.0
        }
    );
}

#[test]
fn localized_clips_are_scaled_into_layer_device_pixels() {
    // A group whose layer starts at logical (288.5, 873.0) on a 2x display,
    // wrapped by the inherited full-viewport clip. Translating without
    // scaling would put the clip's bottom edge at 44 device rows instead of
    // 88, slicing composited content in half.
    let mut commands = vec![Command::PushClip(jag_draw::ClipRect(Rect {
        x: 0.0,
        y: 0.0,
        w: 1512.0,
        h: 917.0,
    }))];

    localize_clips(&mut commands, [288.5, 873.0], 2.0);

    let Command::PushClip(clip) = &commands[0] else {
        unreachable!()
    };
    assert_eq!(
        clip.0,
        Rect {
            x: -577.0,
            y: -1746.0,
            w: 3024.0,
            h: 1834.0
        }
    );
    // Bottom edge lands below a 64px-tall layer, so it clips nothing.
    assert!(clip.0.y + clip.0.h >= 64.0);
}

#[test]
fn layer_bounds_align_outward_at_fractional_device_scale() {
    let geometry = layer_geometry(
        Rect {
            x: 2.2,
            y: 3.4,
            w: 4.1,
            h: 5.2,
        },
        Viewport {
            width: 100,
            height: 100,
        },
        1.25,
    )
    .unwrap();
    assert_eq!(geometry.pixel_size, [6, 7]);
    assert_eq!(geometry.origin, [1.6, 3.2]);
    assert_eq!(geometry.logical_size, [4.8, 5.6]);
}

#[test]
fn invalid_layer_bounds_are_rejected() {
    let viewport = Viewport {
        width: 100,
        height: 100,
    };
    for bounds in [
        Rect {
            x: 0.0,
            y: 0.0,
            w: f32::NAN,
            h: 1.0,
        },
        Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: f32::INFINITY,
        },
        Rect {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 1.0,
        },
    ] {
        assert_eq!(layer_geometry(bounds, viewport, 1.0), None);
    }
}
