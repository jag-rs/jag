use super::*;
use crate::scene::TextRun;
use crate::text::SubpixelMask;

fn rgb(r: f32, g: f32, b: f32) -> ColorLinPremul {
    ColorLinPremul { r, g, b, a: 1.0 }
}

fn red_to_blue(start: [f32; 2], end: [f32; 2]) -> Brush {
    Brush::LinearGradient {
        start,
        end,
        stops: vec![(0.0, rgb(1.0, 0.0, 0.0)), (1.0, rgb(0.0, 0.0, 1.0))],
    }
}

/// Opaque `w`×`h` subpixel glyph.
fn solid_glyph(w: u32, h: u32) -> RasterizedGlyph {
    RasterizedGlyph {
        offset: [0.0, 0.0],
        mask: GlyphMask::Subpixel(SubpixelMask {
            width: w,
            height: h,
            format: MaskFormat::Rgba8,
            data: vec![255; (w * h * 4) as usize],
        }),
    }
}

fn draw(transform: Transform2D, fill: Option<Brush>) -> ExtractedTextDraw {
    ExtractedTextDraw {
        run: TextRun {
            text: "x".into(),
            pos: [0.0, 0.0],
            size: 16.0,
            logical_size: 16.0,
            color: rgb(0.0, 1.0, 0.0),
            weight: 400.0,
            style: Default::default(),
            family: None,
        },
        z: 0,
        transform,
        clip: None,
        fill,
    }
}

fn pixel(glyph: &RasterizedGlyph, w: usize, col: usize, row: usize) -> [u8; 4] {
    let GlyphMask::Color(mask) = &glyph.mask else {
        panic!("filled glyph must be a color mask");
    };
    let i = (row * w + col) * 4;
    [
        mask.data[i],
        mask.data[i + 1],
        mask.data[i + 2],
        mask.data[i + 3],
    ]
}

#[test]
fn vertical_linear_gradient_varies_by_row_not_column() {
    let brush = red_to_blue([0.0, 0.0], [0.0, 4.0]);
    let text = draw(Transform2D::identity(), Some(brush));
    let (glyph, color) = text.glyph_for_draw(&solid_glyph(4, 4), [0.0, 0.0], 1.0);
    assert_eq!(color, WHITE);
    assert_eq!(pixel(&glyph, 4, 0, 0), pixel(&glyph, 4, 3, 0));
    let top = pixel(&glyph, 4, 0, 0);
    let bottom = pixel(&glyph, 4, 0, 3);
    assert!(top[0] > top[2] && bottom[2] > bottom[0]);
}

#[test]
fn gradient_stays_fixed_to_text_when_scrolled() {
    let brush = red_to_blue([0.0, 0.0], [4.0, 0.0]);
    let at_rest = draw(Transform2D::identity(), Some(brush.clone()));
    let scrolled = draw(Transform2D::translate(0.0, -300.0), Some(brush));
    let (rest, _) = at_rest.glyph_for_draw(&solid_glyph(4, 1), [0.0, 0.0], 1.0);
    let (moved, _) = scrolled.glyph_for_draw(&solid_glyph(4, 1), [0.0, -300.0], 1.0);
    assert_eq!(rest.mask.width(), moved.mask.width());
    for col in 0..4 {
        assert_eq!(pixel(&rest, 4, col, 0), pixel(&moved, 4, col, 0));
    }
}

#[test]
fn coverage_scales_the_fill_and_empty_pixels_stay_clear() {
    let glyph = RasterizedGlyph {
        offset: [0.0, 0.0],
        mask: GlyphMask::Subpixel(SubpixelMask {
            width: 2,
            height: 1,
            format: MaskFormat::Rgba8,
            data: vec![128, 128, 128, 0, 0, 0, 0, 0],
        }),
    };
    let text = draw(
        Transform2D::identity(),
        Some(Brush::Solid(rgb(0.0, 1.0, 0.0))),
    );
    let (tinted, _) = text.glyph_for_draw(&glyph, [0.0, 0.0], 1.0);
    assert_eq!(pixel(&tinted, 2, 0, 0), [0, 128, 0, 128]);
    assert_eq!(pixel(&tinted, 2, 1, 0), [0, 0, 0, 0]);
}

#[test]
fn unfilled_text_keeps_mask_and_run_color() {
    let text = draw(Transform2D::identity(), None);
    let source = solid_glyph(2, 2);
    let (glyph, color) = text.glyph_for_draw(&source, [0.0, 0.0], 1.0);
    assert_eq!(color, rgb(0.0, 1.0, 0.0));
    assert!(matches!(glyph.mask, GlyphMask::Subpixel(_)));
}

#[test]
fn radial_and_conic_sample_by_distance_and_angle() {
    let stops = vec![(0.0, rgb(1.0, 0.0, 0.0)), (1.0, rgb(0.0, 0.0, 1.0))];
    let radial = Brush::RadialGradient {
        center: [0.0, 0.0],
        radius: 10.0,
        stops: stops.clone(),
    };
    assert_eq!(brush_color_at(&radial, [0.0, 0.0]), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(brush_color_at(&radial, [0.0, 10.0]), [0.0, 0.0, 1.0, 1.0]);

    let conic = Brush::ConicGradient {
        center: [0.0, 0.0],
        start_angle: 0.0,
        stops,
    };
    // Just clockwise of north is the start; just counter-clockwise is the end.
    let start = brush_color_at(&conic, [0.01, -10.0]);
    let end = brush_color_at(&conic, [-0.01, -10.0]);
    assert!(start[0] > 0.99 && end[2] > 0.99);
    // East is a quarter turn.
    let east = brush_color_at(&conic, [10.0, 0.0]);
    assert!(east[0] > east[2]);
}

#[test]
fn filled_texels_are_srgb_encoded_like_color_glyphs() {
    let red = ColorLinPremul::rgba(239, 68, 68, 255);
    let text = draw(Transform2D::identity(), Some(Brush::Solid(red)));
    let (tinted, _) = text.glyph_for_draw(&solid_glyph(1, 1), [0.0, 0.0], 1.0);
    assert_eq!(pixel(&tinted, 1, 0, 0), [239, 68, 68, 255]);
}
