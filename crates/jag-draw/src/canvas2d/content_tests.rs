use super::{
    geometry::IDENTITY,
    raster::{PatternImage, Raster, image},
    text::{Fonts, TextStyle},
};
fn style() -> TextStyle {
    TextStyle {
        families: vec!["PinnedCanvas".into()],
        size: 20.,
        weight: 400,
        italic: false,
        direction: "ltr".into(),
        align: "left".into(),
        baseline: "alphabetic".into(),
        stretch: "normal".into(),
        caps: "normal".into(),
        rendering: "auto".into(),
        kerning: "normal".into(),
        letter: 0.,
        word: 0.,
        lang: "en".into(),
    }
}
#[test]
fn canvas_text_metrics_and_outlines_share_real_glyphs() {
    let font = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fonts/Noto_Sans/static/NotoSans-Regular.ttf"),
    )
    .unwrap();
    let mut fonts = Fonts::default();
    fonts.register("PinnedCanvas", font, 400, false).unwrap();
    let s = style();
    let prepared = fonts.prepare("Ag", &s).unwrap();
    assert!(!prepared.path.is_empty());
    assert!((prepared.metrics.width - 25.08).abs() < 0.05);
    let bounds = super::geometry::path(&prepared.path)
        .unwrap()
        .compute_tight_bounds()
        .unwrap();
    assert!((prepared.metrics.actual_bounding_box_ascent + f64::from(bounds.top())).abs() < 0.01);
    let mut centered = style();
    centered.align = "center".into();
    let other = fonts.prepare("Ag", &centered).unwrap();
    assert_eq!(other.metrics.width, prepared.metrics.width);
    assert!(
        (other.metrics.actual_bounding_box_left
            - prepared.metrics.actual_bounding_box_left
            - prepared.metrics.width / 2.)
            .abs()
            < 0.01
    );
    fonts.remove("PinnedCanvas", 400, false);
    assert!(!Fonts::validate(b"bad font"));
}
#[test]
fn canvas_image_negative_rectangles_and_self_snapshot_are_immutable() {
    let source = PatternImage {
        width: 2,
        height: 1,
        pixels: vec![255, 0, 0, 255, 0, 0, 255, 255].into(),
    };
    let mut r = Raster::new(8, 4, true).unwrap();
    image::draw(
        &mut r,
        source,
        [2., 1., -2., -1.],
        [8., 4., -8., -4.],
        IDENTITY,
        vec![],
        1.,
        false,
    );
    assert_eq!(&r.pixels()[0..4], &[255, 0, 0, 255]);
    assert_eq!(&r.pixels()[28..32], &[0, 0, 255, 255]);
    let snapshot = r.snapshot();
    r.clear();
    image::draw(
        &mut r,
        snapshot,
        [0., 0., 4., 4.],
        [4., 0., 4., 4.],
        IDENTITY,
        vec![],
        1.,
        false,
    );
    assert_eq!(&r.pixels()[0..4], &[0; 4]);
    assert_eq!(&r.pixels()[16..20], &[255, 0, 0, 255]);
}
#[test]
fn canvas_svg_source_scales_vectors_and_disallows_external_files() {
    let svg=b"<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'><rect width='4' height='4' fill='red'/></svg>";
    let raster = image::decode_svg_scaled(svg, Some([16, 8])).unwrap();
    assert_eq!((raster.width, raster.height), (16, 8));
    assert!(raster.pixels.chunks_exact(4).all(|p| p == [255, 0, 0, 255]));
    assert!(image::decode_svg_scaled(svg, Some([8192, 8192])).is_none());
}
