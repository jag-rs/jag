use jag_draw::{Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect, wgpu};
use jag_surface::JagSurface;
use std::sync::Arc;

fn surface() -> JagSurface {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("GPU adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default(), None)).expect("GPU device");
    let mut surface = JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    );
    surface.set_logical_pixels(true);
    surface.set_frame_cache_enabled(false);
    surface
}

#[test]
fn svg_antialiasing_preserves_white_edge_color() {
    let mut surface = surface();
    for offscreen in [false, true] {
        for size in [16.0, 20.0, 24.0] {
            for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
                surface.set_dpi_scale(dpr);
                let mut canvas = surface.begin_frame((32.0 * dpr) as u32, (32.0 * dpr) as u32);
                if offscreen {
                    // A framebuffer effect selects the same intermediate path Studio
                    // uses. Keep it outside the icon so it does not alter its pixels.
                    canvas.backdrop_blur_rect(
                        Rect {
                            x: 0.0,
                            y: 0.0,
                            w: 1.0,
                            h: 1.0,
                        },
                        1.0,
                        0,
                    );
                }
                canvas.draw_svg(
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/tests/fixtures/white-circle.svg"
                    ),
                    [4.0, 4.0],
                    [size, size],
                    1,
                );
                let (_, _, pixels) = surface.end_frame_headless(canvas).unwrap();
                let edges: Vec<_> = pixels
                    .chunks_exact(4)
                    .filter(|p| p[3] > 30 && p[3] < 220)
                    .collect();
                assert!(!edges.is_empty(), "missing AA at DPR {dpr}");
                for p in edges {
                    // White premultiplied in linear light, then encoded as sRGB,
                    // must never be darker than its coverage on transparent black.
                    assert!(
                        u16::from(p[0]) + 2 >= u16::from(p[3]),
                        "dark SVG fringe at DPR {dpr}, size {size}, offscreen {offscreen}: {p:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn rounded_stroke_is_centered_on_the_requested_path() {
    let mut surface = surface();
    for stroke_width in [1.0, 2.0] {
        let expected_center = if stroke_width == 1.0 { 8.5 } else { 8.0 };
        for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
            surface.set_dpi_scale(dpr);
            let mut canvas = surface.begin_frame((48.0 * dpr) as u32, (48.0 * dpr) as u32);
            canvas.stroke_rounded_rect(
                RoundedRect {
                    rect: Rect {
                        x: 8.0,
                        y: expected_center,
                        w: 32.0,
                        h: 32.0,
                    },
                    radii: RoundedRadii {
                        tl: 6.0,
                        tr: 6.0,
                        br: 6.0,
                        bl: 6.0,
                    },
                },
                stroke_width,
                Brush::Solid(ColorLinPremul::from_srgba_u8([255; 4])),
                1,
            );
            let (width, height, pixels) = surface.end_frame_headless(canvas).unwrap();
            let x = (24.0 * dpr) as u32;
            let mut mass = 0.0;
            let mut moment = 0.0;
            for y in 0..height / 2 {
                let a = pixels[((y * width + x) * 4 + 3) as usize] as f32;
                mass += a;
                moment += a * (y as f32 + 0.5);
            }
            assert!(mass > 0.0);
            let center = moment / mass / dpr;
            assert!(
                (center - expected_center).abs() < 0.15,
                "stroke shifted inward at DPR {dpr}: {center}"
            );
        }
    }
}

#[test]
fn iframe_opacity_layer_preserves_svg_antialiasing() {
    let mut surface = surface();
    for transform in [
        jag_draw::Transform2D::identity(),
        jag_draw::Transform2D {
            m: [1.5, 0.0, 0.0, 1.5, 3.0, 5.0],
        },
    ] {
        for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
            surface.set_dpi_scale(dpr);
            let mut frames = Vec::new();
            for grouped in [false, true] {
                let mut canvas = surface.begin_frame((48.0 * dpr) as u32, (48.0 * dpr) as u32);
                canvas.push_transform(transform);
                if grouped {
                    canvas.push_opacity(1.0);
                }
                canvas.fill_rect(
                    0.0,
                    0.0,
                    48.0,
                    48.0,
                    Brush::Solid(ColorLinPremul::from_srgba_u8([0, 0, 0, 255])),
                    0,
                );
                canvas.draw_svg(
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/tests/fixtures/white-circle.svg"
                    ),
                    [14.0, 14.0],
                    [20.0, 20.0],
                    1,
                );
                if grouped {
                    canvas.pop_opacity();
                }
                canvas.pop_transform();
                let (_, _, pixels) = surface.end_frame_headless(canvas).unwrap();
                frames.push(pixels);
            }
            let max_difference = frames[0]
                .iter()
                .zip(&frames[1])
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(
                max_difference <= 2,
                "opacity-1 layer changes SVG edges at DPR {dpr}: max channel difference {max_difference}"
            );
        }
    }
}
