use jag_draw::{
    BoxShadowSpec, Brush, ColorLinPremul, Rect, RoundedRadii, RoundedRect, Transform2D, wgpu,
};
use jag_surface::{Canvas, JagSurface};
use std::sync::Arc;

fn surface() -> JagSurface {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default()))
            .expect("GPU adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default(), None)).expect("GPU device");
    JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    )
}

fn shadow(c: &mut Canvas, y: f32, blur: f32, spread: f32) {
    c.box_shadow(
        RoundedRect {
            rect: Rect {
                x: 20.0,
                y,
                w: 140.0,
                h: 44.0,
            },
            radii: RoundedRadii {
                tl: 12.0,
                tr: 12.0,
                br: 12.0,
                bl: 12.0,
            },
        },
        BoxShadowSpec {
            offset: [-4.0, 7.0],
            blur_radius: blur,
            spread,
            color: ColorLinPremul::from_srgba_u8([30, 50, 80, 180]),
        },
        1,
    );
}

fn cards(c: &mut Canvas, count: usize) {
    c.push_bound_scroll_layer("cards", "offset", [0.0, 0.0], [-1.0, -1.0]);
    for row in 0..count {
        // Per-card paint scopes split the committed stream, as clips and
        // decreasing paint z do in real documents. Their enclosing clip is
        // the scroll content extent, not the individual card's ink bounds.
        c.push_clip_rect(Rect {
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: count as f32 * 80.0,
        });
        shadow(c, 15.0 + row as f32 * 80.0, 8.0, 2.0);
        c.fill_rect(
            20.0,
            15.0 + row as f32 * 80.0,
            140.0,
            44.0,
            Brush::Solid(ColorLinPremul::from_srgba_u8([240, 230, 220, 255])),
            2,
        );
        c.pop_clip();
    }
    c.pop_scroll_layer();
}

#[test]
fn offscreen_shadow_cards_do_not_grow_visible_raster_work_or_residency() {
    let mut s = surface();
    s.set_dpi_scale(2.0);
    let mut baseline = None;
    for count in [20, 200, 2000] {
        s.set_retained_layer_budget(32 * 1024 * 1024);
        let mut c = s.begin_frame(400, 400);
        let capture = c.begin_scroll_scene_capture();
        cards(&mut c, count);
        let scene = c.finish_scroll_scene_capture(capture).unwrap();
        let pixels = s.end_frame_headless(c).unwrap().2;
        let cold = s.retained_layer_stats();
        assert!(cold.misses <= 8, "{count} cards: {cold:?}");
        let measurement = (
            cold.misses,
            cold.rasterized_pixels,
            cold.resident_bytes,
            pixels,
        );
        if let Some(expected) = &baseline {
            assert_eq!(&measurement, expected, "offscreen list length {count}");
        } else {
            baseline = Some(measurement);
        }
        for offset in [4.0, 520.0, 524.0, 4.0] {
            let mut c = s.begin_frame(400, 400);
            c.replay_scroll_scene(
                &scene,
                &[("offset".into(), [0.0, offset])].into_iter().collect(),
            );
            s.end_frame_headless(c).unwrap();
            let stats = s.retained_layer_stats();
            assert!(
                stats.misses <= 8,
                "{count} cards, offset {offset}: {stats:?}"
            );
            assert!(stats.resident_bytes < 4 * 1024 * 1024, "{stats:?}");
            let mut warm = s.begin_frame(400, 400);
            warm.replay_scroll_scene(
                &scene,
                &[("offset".into(), [0.0, offset])].into_iter().collect(),
            );
            s.end_frame_headless(warm).unwrap();
            assert_eq!(s.retained_layer_stats().rasterized_pixels, 0);
        }
    }
}

#[test]
fn retained_shadow_ink_matches_unbounded_reference_at_tile_edges_and_transforms() {
    let mut s = surface();
    // An unbounded hyperlink is a deliberately nonpainting reference marker:
    // it disables tight allocation, preserving the old full-grid-tile raster
    // as a pixel oracle without relying on the new shadow bounds.
    for dpr in [1.0, 1.25, 2.0, 3.0] {
        s.set_dpi_scale(dpr);
        for (blur, spread, transform) in [
            (0.0, -3.0, Transform2D::identity()),
            (12.0, 4.0, Transform2D::identity()),
            (
                20.0,
                -5.0,
                Transform2D {
                    m: [1.3, 0.2, -0.4, 0.8, 15.0, 0.0],
                },
            ),
            (
                8.0,
                1.0,
                Transform2D {
                    m: [-1.0, 0.0, 0.0, 1.0, 190.0, 0.0],
                },
            ),
        ] {
            let mut scenes = Vec::new();
            for unbounded in [false, true] {
                let mut c = s.begin_frame(400, 300);
                let capture = c.begin_scroll_scene_capture();
                c.push_bound_scroll_layer("shadow", "offset", [0.0, 0.0], [-1.0, -1.0]);
                c.push_transform(transform);
                shadow(&mut c, 505.0 / dpr, blur, spread);
                c.pop_transform();
                if unbounded {
                    c.extend_commands(&[unbounded_marker()]);
                }
                c.pop_scroll_layer();
                scenes.push(c.finish_scroll_scene_capture(capture).unwrap());
            }
            for physical_offset in [470.0, 490.0, 512.0, 520.0, 490.0] {
                let mut outputs = Vec::new();
                for scene in &scenes {
                    s.set_retained_layer_budget(0);
                    let mut c = s.begin_frame(400, 300);
                    c.replay_scroll_scene(
                        scene,
                        &[("offset".into(), [0.0, physical_offset / dpr])]
                            .into_iter()
                            .collect(),
                    );
                    outputs.push(s.end_frame_headless(c).unwrap().2);
                }
                let worst = outputs[0]
                    .iter()
                    .zip(&outputs[1])
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap();
                assert!(
                    worst <= 1,
                    "DPR {dpr}, blur {blur}, spread {spread}, offset {physical_offset}, worst {worst}"
                );
            }
        }
    }
}

fn unbounded_marker() -> jag_draw::Command {
    // A zero-opacity shadow still has finite ink; use an empty hyperlink,
    // which the compositor intentionally keeps in the conservative bin.
    jag_draw::Command::DrawHyperlink {
        hyperlink: jag_draw::Hyperlink {
            text: String::new(),
            pos: [0.0, 0.0],
            size: 16.0,
            color: ColorLinPremul::from_srgba_u8([0; 4]),
            url: String::new(),
            weight: 400.0,
            measured_width: Some(0.0),
            underline: false,
            underline_color: None,
            family: None,
            style: jag_draw::FontStyle::Normal,
        },
        z: 2,
        transform: Transform2D::identity(),
        id: 0,
    }
}
