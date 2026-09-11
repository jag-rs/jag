use jag_draw::{Brush, ColorLinPremul, Rect, Transform2D, wgpu};
use jag_surface::{Canvas, JagSurface};
use std::sync::Arc;

fn surface() -> JagSurface {
    let adapter =
        pollster::block_on(wgpu::Instance::default().request_adapter(&Default::default())).unwrap();
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default(), None)).unwrap();
    JagSurface::new(
        Arc::new(device),
        Arc::new(queue),
        wgpu::TextureFormat::Rgba8UnormSrgb,
    )
}

fn push(c: &mut Canvas, tiled: bool, key: &str, delta: [f32; 2]) {
    if tiled {
        c.push_scroll_layer(key, delta);
    } else {
        c.push_transform(Transform2D::translate(delta[0], delta[1]));
    }
}
fn pop(c: &mut Canvas, tiled: bool) {
    if tiled {
        c.pop_scroll_layer();
    } else {
        c.pop_transform();
    }
}
fn fill(c: &mut Canvas, rect: Rect, color: [u8; 4], z: i32) {
    c.fill_rect(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Brush::Solid(ColorLinPremul::from_srgba_u8(color)),
        z,
    );
}

fn render(
    s: &mut JagSurface,
    tiled: bool,
    dpr: f32,
    outer: f32,
    inner: f32,
    changed: bool,
) -> Vec<u8> {
    s.set_dpi_scale(dpr);
    let mut c = s.begin_frame((240.0 * dpr) as u32, (220.0 * dpr) as u32);
    c.clear(ColorLinPremul::from_srgba_u8([245, 245, 245, 255]));
    c.push_clip_rect(Rect {
        x: 10.0,
        y: 10.0,
        w: 220.0,
        h: 200.0,
    });
    push(&mut c, tiled, "shell", [0.0, -outer]);
    for row in 0..80 {
        fill(
            &mut c,
            Rect {
                x: 10.0,
                y: 10.0 + row as f32 * 20.0,
                w: 220.0,
                h: 20.0,
            },
            if row % 2 == 0 {
                [60, 80, 100, 255]
            } else {
                [100, 120, 140, 255]
            },
            row,
        );
    }
    c.push_clip_rect(Rect {
        x: 40.0,
        y: 40.0,
        w: 160.0,
        h: 100.0,
    });
    push(&mut c, tiled, "iframe/inner", [-inner, -inner]);
    for row in 0..80 {
        fill(
            &mut c,
            Rect {
                x: 40.0,
                y: 40.0 + row as f32 * 13.0,
                w: 600.0,
                h: 10.0,
            },
            if changed && row == 5 {
                [30, 160, 90, 255]
            } else {
                [190, 50, 30, 255]
            },
            100 + row,
        );
    }
    pop(&mut c, tiled);
    c.pop_clip();
    // Counter-translation gives a separately retained fixed header.
    push(&mut c, tiled, "fixed", [0.0, outer]);
    fill(
        &mut c,
        Rect {
            x: 12.0,
            y: 12.0,
            w: 216.0,
            h: 12.0,
        },
        [30, 60, 210, 255],
        500,
    );
    pop(&mut c, tiled);
    pop(&mut c, tiled);
    c.pop_clip();
    s.end_frame_headless(c).unwrap().2
}

fn assert_pixels(actual: &[u8], expected: &[u8], label: &str) {
    let bad = actual
        .iter()
        .zip(expected)
        .enumerate()
        .filter(|(_, (a, b))| a.abs_diff(**b) > 2)
        .take(8)
        .collect::<Vec<_>>();
    assert!(bad.is_empty(), "{label}: differing channels {bad:?}");
}

#[test]
fn direct_tile_presentation_matches_intermediate_for_nested_and_fixed_content() {
    let mut direct = surface();
    direct.set_use_intermediate(false);
    let mut intermediate = surface();
    for dpr in [1.0, 1.25, 2.0, 3.0] {
        for (outer, inner) in [(0.0, 0.0), (13.0, 7.0), (19.0, 522.0)] {
            let actual = render(&mut direct, true, dpr, outer, inner, false);
            let expected = render(&mut intermediate, true, dpr, outer, inner, false);
            assert_pixels(&actual, &expected, "direct mobile tile presentation");
        }
    }
}

#[test]
fn nested_tiles_match_direct_paint_across_boundaries_and_reuse_while_scrolling() {
    let mut s = surface();
    for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
        for (outer, inner) in [
            (0.0, 0.0),
            (0.0, 4.0),
            (4.0, 4.0),
            (4.0, 520.0),
            (8.0, 520.0),
            (4.0, 4.0),
        ] {
            let outer = outer / dpr;
            let inner = inner / dpr;
            let expected = render(&mut s, false, dpr, outer, inner, false);
            let actual = render(&mut s, true, dpr, outer, inner, false);
            assert_pixels(
                &actual,
                &expected,
                &format!("DPR {dpr}, outer {outer}, inner {inner}"),
            );
            let again = render(
                &mut s,
                true,
                dpr,
                outer + 1.0 / dpr,
                inner + 1.0 / dpr,
                false,
            );
            let stats = s.retained_layer_stats();
            assert!(stats.hits > 0, "scroll must reuse content tiles: {stats:?}");
            assert_pixels(
                &again,
                &render(
                    &mut s,
                    false,
                    dpr,
                    outer + 1.0 / dpr,
                    inner + 1.0 / dpr,
                    false,
                ),
                "scroll movement",
            );
        }
    }
}

#[test]
fn changed_content_invalidates_pixels_and_disabling_retention_still_paints() {
    let mut s = surface();
    render(&mut s, true, 1.0, 0.0, 0.0, false);
    let changed = render(&mut s, true, 1.0, 0.0, 0.0, true);
    let stats = s.retained_layer_stats();
    assert!(
        stats.hits > 0 && stats.misses > 0,
        "only changed owner should repaint: {stats:?}"
    );
    assert_pixels(
        &changed,
        &render(&mut s, false, 1.0, 0.0, 0.0, true),
        "content revision",
    );
    s.set_retained_layer_budget(0);
    assert_eq!(changed, render(&mut s, true, 1.0, 0.0, 0.0, true));
    assert_eq!(s.retained_layer_stats().resident_bytes, 0);
}

fn many_visible_layers(s: &mut JagSurface, tiled: bool, offset: f32) -> Vec<u8> {
    let mut c = s.begin_frame(256, 180);
    c.clear(ColorLinPremul::from_srgba_u8([245, 245, 245, 255]));
    // Separate clip/scroll owners in real shell/iframe content create many
    // small tiles, even when the total visible texture memory is modest.
    for i in 0..160 {
        push(&mut c, tiled, &format!("panel-{i}"), [0.0, -offset]);
        fill(
            &mut c,
            Rect {
                x: 8.0 + (i % 16) as f32 * 15.0,
                y: 8.0 + (i / 16) as f32 * 16.0,
                w: 10.0,
                h: 12.0,
            },
            [40 + (i % 120) as u8, 80, 160, 255],
            i,
        );
        pop(&mut c, tiled);
    }
    s.end_frame_headless(c).unwrap().2
}

#[test]
fn more_than_128_visible_tiles_fit_the_budget_without_raster_churn() {
    let mut s = surface();
    many_visible_layers(&mut s, true, 0.0);
    assert!(s.retained_layer_stats().misses >= 160);
    let pixels = many_visible_layers(&mut s, true, 1.0);
    let stats = s.retained_layer_stats();
    assert!(
        stats.hits >= 160,
        "visible tiles must remain resident: {stats:?}"
    );
    assert_eq!(
        stats.misses, 0,
        "scroll must not redraw unchanged tiles: {stats:?}"
    );
    assert_eq!(
        stats.evictions, 0,
        "the working set fits in memory: {stats:?}"
    );
    assert_pixels(
        &pixels,
        &many_visible_layers(&mut s, false, 1.0),
        "many tiles",
    );
}

#[test]
fn an_oversubscribed_frame_reuses_resident_tiles_without_evicting_its_own_work() {
    let mut s = surface();
    let budget = 16 * 1024;
    s.set_retained_layer_budget(budget);
    many_visible_layers(&mut s, true, 0.0);
    let mut pixels = Vec::new();
    for offset in [1.0, 2.0] {
        pixels = many_visible_layers(&mut s, true, offset);
        let stats = s.retained_layer_stats();
        assert!(
            stats.hits > 0 && stats.misses > 0,
            "bounded partial reuse: {stats:?}"
        );
        assert_eq!(
            stats.evictions, 0,
            "do not cycle the visible working set: {stats:?}"
        );
        assert!(stats.resident_bytes <= budget);
        assert!(stats.budget_usage_bytes <= budget);
    }
    assert_pixels(
        &pixels,
        &many_visible_layers(&mut s, false, 2.0),
        "uncached tiles must still paint under memory pressure",
    );
}

fn recorded_content(c: &mut Canvas, outer: f32, inner: f32) {
    c.push_bound_scroll_layer("outer", "outer-input", [0.0, outer], [-1.0, -1.0]);
    fill(
        c,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 230.0,
            h: 1500.0,
        },
        [30, 60, 90, 255],
        1,
    );
    c.push_clip_rect(Rect {
        x: 25.0,
        y: 25.0,
        w: 170.0,
        h: 120.0,
    });
    c.push_bound_scroll_layer("inner", "inner-input", [inner, inner], [-1.0, -1.0]);
    for row in 0..100 {
        fill(
            c,
            Rect {
                x: 25.0,
                y: 25.0 + row as f32 * 12.0,
                w: 700.0,
                h: 7.0,
            },
            [180, 60, 40, 255],
            2 + row,
        );
    }
    c.pop_scroll_layer();
    c.pop_clip();
    c.push_bound_scroll_layer("fixed", "outer-input", [0.0, outer], [1.0, 1.0]);
    fill(
        c,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 210.0,
            h: 17.0,
        },
        [20, 190, 80, 255],
        500,
    );
    c.pop_scroll_layer();
    c.pop_scroll_layer();
}

#[test]
fn immutable_scene_rebases_nested_clips_and_counter_transforms_without_repainting() {
    let mut s = surface();
    for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
        s.set_dpi_scale(dpr);
        let make_canvas = |s: &JagSurface, base: f32| {
            let mut c = s.begin_frame((240.0 * dpr) as u32, (220.0 * dpr) as u32);
            c.clear(ColorLinPremul::from_srgba_u8([245, 245, 245, 255]));
            c.push_clip_rect(Rect {
                x: 0.0,
                y: 0.0,
                w: 220.0,
                h: 200.0,
            });
            c.push_transform(Transform2D::translate(base, base));
            c
        };
        let mut first = make_canvas(&s, 0.0);
        let capture = first.begin_scroll_scene_capture();
        recorded_content(&mut first, 0.0, 0.0);
        let scene = first.finish_scroll_scene_capture(capture).unwrap();
        first.pop_transform();
        first.pop_clip();
        s.end_frame_headless(first).unwrap();
        for (base, outer, inner) in [(0.0, 4.0, 3.0), (4.0, 5.0, 9.0), (4.0, 4.0, 520.0)] {
            let (base, outer, inner) = (base / dpr, outer / dpr, inner / dpr);
            let inputs = [
                ("outer-input".into(), [0.0, outer]),
                ("inner-input".into(), [inner, inner]),
            ]
            .into_iter()
            .collect();
            let mut c = make_canvas(&s, base);
            c.replay_scroll_scene(&scene, &inputs);
            c.pop_transform();
            c.pop_clip();
            let actual = s.end_frame_headless(c).unwrap().2;
            assert!(s.retained_layer_stats().hits > 0);
            let mut expected = make_canvas(&s, base);
            recorded_content(&mut expected, outer, inner);
            expected.pop_transform();
            expected.pop_clip();
            let expected = s.end_frame_headless(expected).unwrap().2;
            assert_pixels(
                &actual,
                &expected,
                &format!("committed scene DPR {dpr}, base {base}, outer {outer}, inner {inner}"),
            );
        }
    }
}

#[test]
fn hit_regions_preserve_targeting_without_fragmenting_scroll_tiles() {
    let mut s = surface();
    let paint = |c: &mut Canvas| {
        for row in 0..20 {
            let rect = Rect {
                x: 8.0,
                y: 8.0 + row as f32 * 10.0,
                w: 180.0,
                h: 7.0,
            };
            fill(c, rect, [30 + row as u8 * 5, 60, 90, 255], row);
            // High hit z must neither force a paint boundary nor replace the
            // authoritative hit metadata in the retained source.
            c.hit_region_rect(100 + row as u32, rect, 1000 + row);
        }
    };
    let make_canvas = |s: &JagSurface| {
        let mut c = s.begin_frame(220, 220);
        c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
        c
    };
    let mut c = make_canvas(&s);
    let capture = c.begin_scroll_scene_capture();
    c.push_bound_scroll_layer("content", "scroll", [0.0, 0.0], [-1.0, -1.0]);
    paint(&mut c);
    c.pop_scroll_layer();
    let scene = c.finish_scroll_scene_capture(capture).unwrap();
    let hit = jag_draw::HitIndex::build(c.display_list())
        .topmost_at([20.0, 30.0])
        .unwrap();
    assert_eq!(hit.region_id, Some(102));
    s.end_frame_headless(c).unwrap();
    assert_eq!(
        s.retained_layer_stats().misses,
        1,
        "one paint run, not one per hit marker"
    );
    for scroll in [2.0, 7.0, 2.0] {
        let mut c = make_canvas(&s);
        c.replay_scroll_scene(
            &scene,
            &[("scroll".into(), [0.0, scroll])].into_iter().collect(),
        );
        let hit = jag_draw::HitIndex::build(c.display_list())
            .topmost_at([20.0, 30.0 - scroll])
            .unwrap();
        assert_eq!(hit.region_id, Some(102));
        let actual = s.end_frame_headless(c).unwrap().2;
        assert_eq!(s.retained_layer_stats().misses, 0);
        assert_eq!(s.retained_layer_stats().hits, 1);
        let mut c = make_canvas(&s);
        c.push_transform(Transform2D::translate(0.0, -scroll));
        paint(&mut c);
        c.pop_transform();
        assert_pixels(
            &actual,
            &s.end_frame_headless(c).unwrap().2,
            "hit metadata does not affect pixels",
        );
    }
}

#[test]
fn fixed_opacity_layer_ignores_scrolling_ancestor_transform_in_cache_key() {
    let mut s = surface();
    let make = |s: &JagSurface, offset: f32| {
        let mut c = s.begin_frame(200, 200);
        c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
        c.push_transform(Transform2D::translate(0.0, -offset));
        c
    };
    let mut c = make(&s, 0.0);
    let capture = c.begin_scroll_scene_capture();
    c.push_scroll_layer("page", [0.0, 0.0]);
    c.push_opacity(0.7);
    c.push_bound_scroll_layer("fixed", "page-scroll", [0.0, 0.0], [1.0, 1.0]);
    fill(
        &mut c,
        Rect {
            x: 20.0,
            y: 150.0,
            w: 80.0,
            h: 20.0,
        },
        [40, 80, 120, 255],
        1,
    );
    c.pop_scroll_layer();
    c.pop_opacity();
    c.pop_scroll_layer();
    let scene = c.finish_scroll_scene_capture(capture).unwrap();
    c.pop_transform();
    let expected = s.end_frame_headless(c).unwrap().2;
    for offset in [2.0, 7.0, 13.0, 2.0] {
        let mut c = make(&s, offset);
        c.replay_scroll_scene(
            &scene,
            &[("page-scroll".into(), [0.0, offset])]
                .into_iter()
                .collect(),
        );
        c.pop_transform();
        assert_pixels(
            &s.end_frame_headless(c).unwrap().2,
            &expected,
            "fixed opacity pixels",
        );
        let stats = s.retained_layer_stats();
        assert_eq!(
            stats.misses, 0,
            "fixed child and its opacity surface must both reuse: {stats:?}"
        );
        assert_eq!(stats.hits, 2);
    }
}

struct OverhangingGlyph;
impl jag_draw::TextProvider for OverhangingGlyph {
    fn rasterize_run(&self, _: &jag_draw::TextRun) -> Vec<jag_draw::RasterizedGlyph> {
        vec![jag_draw::RasterizedGlyph {
            offset: [-3.0, -7.0],
            mask: jag_draw::GlyphMask::Subpixel(jag_draw::SubpixelMask {
                width: 48,
                height: 32,
                format: jag_draw::MaskFormat::Rgba8,
                data: [255, 128, 0, 0].repeat(48 * 32),
            }),
        }]
    }
}

#[test]
fn compiled_text_retains_full_ink_and_coverage_across_tile_boundaries() {
    let mut s = surface();
    let provider = Arc::new(OverhangingGlyph);
    for dpr in [1.0, 1.25, 1.5, 2.0, 3.0] {
        s.set_dpi_scale(dpr);
        let make_canvas = |s: &JagSurface| {
            let mut c = s.begin_frame(120, 120);
            c.set_text_provider(provider.clone());
            c.clear(ColorLinPremul::from_srgba_u8([255; 4]));
            c
        };
        let text = |c: &mut Canvas| {
            c.draw_text_run(
                [503.0 / dpr, 500.0 / dpr],
                "i".into(),
                16.0,
                ColorLinPremul::from_srgba_u8([0, 0, 0, 255]),
                1,
            )
        };
        let mut c = make_canvas(&s);
        let capture = c.begin_scroll_scene_capture();
        c.push_bound_scroll_layer("text", "offset", [0.0, 0.0], [-1.0, -1.0]);
        text(&mut c);
        c.pop_scroll_layer();
        let scene = c.finish_scroll_scene_capture(capture).unwrap();
        s.end_frame_headless(c).unwrap();
        for physical_offset in [470.0, 475.0, 490.0, 500.0, 520.0, 475.0] {
            let offset = physical_offset / dpr;
            let mut c = make_canvas(&s);
            c.replay_scroll_scene(
                &scene,
                &[("offset".into(), [offset, offset])].into_iter().collect(),
            );
            let actual = s.end_frame_headless(c).unwrap().2;
            let mut direct = make_canvas(&s);
            direct.push_transform(Transform2D::translate(-offset, -offset));
            text(&mut direct);
            direct.pop_transform();
            let expected = s.end_frame_headless(direct).unwrap().2;
            assert_pixels(
                &actual,
                &expected,
                &format!("text ink DPR {dpr}, offset {physical_offset}"),
            );
        }
    }
}
