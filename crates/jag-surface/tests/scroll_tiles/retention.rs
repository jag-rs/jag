use super::*;

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
