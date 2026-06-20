//! Tests for SVG `<text>` font resolution, exercising the iOS "no system
//! fonts" condition without a GPU device.

use super::{build_svg_fontdb, render_svg_to_pixmap};

/// A static (non-variable) face so usvg can shape it directly.
const GEIST_REGULAR: &[u8] = include_bytes!("../../../fonts/Geist/static/Geist-Regular.ttf");

/// An SVG whose only paint is a `<text>` run — so any non-transparent pixels in
/// the raster come from rendered glyphs.
const TEXT_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40" viewBox="0 0 200 40"><text x="8" y="28" font-size="24" fill="black" font-family="sans-serif">Jan</text></svg>"#;

fn opaque_pixel_count(pixmap: &tiny_skia::Pixmap) -> usize {
    pixmap
        .data()
        .chunks_exact(4)
        .filter(|rgba| rgba[3] != 0)
        .count()
}

#[test]
fn svg_text_is_dropped_without_any_font() {
    // Reproduces iOS: `fontdb::load_system_fonts()` finds nothing and no
    // fallback is registered, so usvg has no glyph outlines for the `<text>`.
    let fonts = build_svg_fontdb(false, std::iter::empty::<&[u8]>());
    let pixmap =
        render_svg_to_pixmap(TEXT_SVG.as_bytes(), 1.0, &fonts, None, 4096).expect("rasterized svg");
    assert_eq!(
        opaque_pixel_count(&pixmap),
        0,
        "without a font the SVG text must produce no glyph pixels (the bug)"
    );
}

#[test]
fn registered_fallback_font_renders_svg_text() {
    // The fix: with a host-registered fallback font (and no system fonts),
    // the same `<text>` resolves and rasterizes visible glyphs.
    let fonts = build_svg_fontdb(false, [GEIST_REGULAR]);
    assert!(
        fonts.default_family.is_some(),
        "fallback family should be wired in as the default when system fonts are absent"
    );
    let pixmap =
        render_svg_to_pixmap(TEXT_SVG.as_bytes(), 1.0, &fonts, None, 4096).expect("rasterized svg");
    assert!(
        opaque_pixel_count(&pixmap) > 50,
        "registered fallback font must render visible glyph pixels for SVG text"
    );
}
