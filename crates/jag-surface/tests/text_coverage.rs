use std::sync::Arc;

use jag_draw::{
    Brush, ColorLinPremul, GlyphMask, MaskFormat, RasterizedGlyph, SubpixelMask, TextProvider,
    TextRun, wgpu,
};
use jag_surface::JagSurface;

struct LcdEdge;

impl TextProvider for LcdEdge {
    fn rasterize_run(&self, _: &TextRun) -> Vec<RasterizedGlyph> {
        vec![RasterizedGlyph {
            offset: [0.0, 0.0],
            mask: GlyphMask::Subpixel(SubpixelMask {
                width: 4,
                height: 4,
                format: MaskFormat::Rgba8,
                // Half-pixel average coverage, with one fully covered subpixel.
                data: [255, 128, 0, 0].repeat(16),
            }),
        }]
    }
}

fn srgb(linear: f32) -> u8 {
    let encoded = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

#[test]
fn lcd_coverage_keeps_its_average_without_widening_or_tinting_edges() {
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
    let coverage = (255.0 + 128.0) / (3.0 * 255.0);
    for cached in [false, true] {
        surface.set_frame_cache_enabled(cached);
        for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
            surface.set_dpi_scale(dpr);
            for grouped in [false, true] {
                for (foreground, background) in [
                    ([0, 0, 0, 255], [255; 4]),
                    ([255; 4], [20, 20, 20, 255]),
                    ([0, 60, 210, 255], [255; 4]),
                    ([0, 0, 0, 128], [255; 4]),
                    ([0, 60, 210, 128], [255; 4]),
                ] {
                    let fg = ColorLinPremul::from_srgba_u8(foreground);
                    let bg = ColorLinPremul::from_srgba_u8(background);
                    let expected = [
                        srgb(fg.r * coverage + bg.r * (1.0 - fg.a * coverage)),
                        srgb(fg.g * coverage + bg.g * (1.0 - fg.a * coverage)),
                        srgb(fg.b * coverage + bg.b * (1.0 - fg.a * coverage)),
                    ];
                    // Render twice to exercise cached as well as fresh-frame paths.
                    for _ in 0..2 {
                        let mut canvas =
                            surface.begin_frame((24.0 * dpr) as u32, (24.0 * dpr) as u32);
                        canvas.set_text_provider(Arc::new(LcdEdge));
                        canvas.fill_rect(0.0, 0.0, 24.0, 24.0, Brush::Solid(bg), 0);
                        if grouped {
                            canvas.push_opacity(1.0);
                        }
                        canvas.draw_text_run([4.0, 4.0], "edge".into(), 16.0, fg, 1);
                        if grouped {
                            canvas.pop_opacity();
                        }
                        let (width, _, pixels) = surface.end_frame_headless(canvas).unwrap();
                        let xy = (4.0 * dpr) as u32 + 1;
                        let i = ((xy * width + xy) * 4) as usize;
                        let actual = &pixels[i..i + 4];
                        assert!(
                            actual[..3]
                                .iter()
                                .zip(expected)
                                .all(|(a, b)| a.abs_diff(b) <= 3),
                            "LCD edge gained weight/color: actual={actual:?}, expected={expected:?}, dpr={dpr}, grouped={grouped}, cached={cached}"
                        );
                        assert_eq!(actual[3], 255);
                    }
                }
            }
        }
    }
}
