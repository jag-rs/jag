//! Image geometry and premultiplied sampling. Sources are immutable call-time snapshots.
use super::*;
pub fn draw(
    raster: &mut Raster,
    image: PatternImage,
    source: [f64; 4],
    dest: [f64; 4],
    transform: [f64; 6],
    clips: Vec<Clip>,
    alpha: f64,
    smooth: bool,
) {
    let _ = draw_effects(
        raster,
        image,
        source,
        dest,
        transform,
        clips,
        alpha,
        smooth,
        Effects::default(),
    );
}

pub fn draw_effects(
    raster: &mut Raster,
    image: PatternImage,
    source: [f64; 4],
    dest: [f64; 4],
    transform: [f64; 6],
    clips: Vec<Clip>,
    alpha: f64,
    smooth: bool,
    effects: Effects,
) -> Result<(), &'static str> {
    let positive = |mut r: [f64; 4]| {
        if r[2] < 0. {
            r[0] += r[2];
            r[2] = -r[2];
        }
        if r[3] < 0. {
            r[1] += r[3];
            r[3] = -r[3];
        }
        r
    };
    let source = positive(source);
    let dest = positive(dest);
    if source[2] == 0. || source[3] == 0. || dest[2] == 0. || dest[3] == 0. {
        return Ok(());
    }
    let x0 = source[0].max(0.);
    let y0 = source[1].max(0.);
    let x1 = (source[0] + source[2]).min(image.width as f64);
    let y1 = (source[1] + source[3]).min(image.height as f64);
    if x1 <= x0 || y1 <= y0 {
        return Ok(());
    }
    let x = dest[0] + (x0 - source[0]) * dest[2] / source[2];
    let y = dest[1] + (y0 - source[1]) * dest[3] / source[3];
    let w = (x1 - x0) * dest[2] / source[2];
    let h = (y1 - y0) * dest[3] / source[3];
    let [a, b, c, d, e, f] = transform;
    let mut path: geometry::Commands = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
        .into_iter()
        .enumerate()
        .map(|(i, [x, y])| {
            vec![
                if i == 0 { 0. } else { 1. },
                a * x + c * y + e,
                b * x + d * y + f,
            ]
        })
        .collect();
    path.push(vec![4.]);
    let op = Draw {
        effects,
        path,
        rule: "nonzero".into(),
        stroke: None,
        transform,
        clips,
        paint: Paint::Image {
            image: 0,
            matrix: transform,
            source,
            dest,
            smooth,
        },
        alpha,
        clear: false,
    };
    raster.draw_checked(&op, &std::collections::HashMap::from([(0, image)]))
}
pub(super) fn sample(p: &PatternImage, x: f64, y: f64, smooth: bool) -> [f64; 4] {
    if p.width == 0 || p.height == 0 || p.pixels.len() != p.width as usize * p.height as usize * 4 {
        return [0.; 4];
    }
    let pixel = |x: f64, y: f64| {
        let x = x.clamp(0., p.width as f64 - 1.) as usize;
        let y = y.clamp(0., p.height as f64 - 1.) as usize;
        let i = (y * p.width as usize + x) * 4;
        std::array::from_fn(|j| p.pixels[i + j] as f64 / 255.)
    };
    let mut out: [f64; 4] = if smooth {
        let x = x - 0.5;
        let y = y - 0.5;
        let (ix, iy) = (x.floor(), y.floor());
        let (fx, fy) = (x - ix, y - iy);
        let (a, b, c, d) = (
            pixel(ix, iy),
            pixel(ix + 1., iy),
            pixel(ix, iy + 1.),
            pixel(ix + 1., iy + 1.),
        );
        std::array::from_fn(|i| {
            (a[i] * (1. - fx) + b[i] * fx) * (1. - fy) + (c[i] * (1. - fx) + d[i] * fx) * fy
        })
    } else {
        pixel(x.floor(), y.floor())
    };
    if out[3] > 0. {
        for i in 0..3 {
            out[i] /= out[3];
        }
    }
    out
}

/// Decode standalone SVG image sources using JAG's existing SVG renderer.
/// External file/URL references are disabled; embedded data remains handled by usvg.
pub fn decode_svg(bytes: &[u8]) -> Option<PatternImage> {
    decode_svg_scaled(bytes, None)
}
pub fn decode_svg_scaled(bytes: &[u8], requested: Option<[u32; 2]>) -> Option<PatternImage> {
    let mut options = usvg::Options::default();
    options.fontdb = crate::svg_fontdb::svg_font_db().db;
    options.image_href_resolver.resolve_string = Box::new(|_, _| None);
    let tree = usvg::Tree::from_data(bytes, &options).ok()?;
    let size = tree.size().to_int_size();
    let [width, height] = requested.unwrap_or([size.width(), size.height()]);
    if width == 0
        || height == 0
        || width > 8192
        || height > 8192
        || u64::from(width) * u64::from(height) > 4 * 1024 * 1024
    {
        return None;
    }
    let mut pixels = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(
            width as f32 / size.width() as f32,
            height as f32 / size.height() as f32,
        ),
        &mut pixels.as_mut(),
    );
    Some(PatternImage {
        width,
        height,
        pixels: pixels.data().into(),
    })
}
