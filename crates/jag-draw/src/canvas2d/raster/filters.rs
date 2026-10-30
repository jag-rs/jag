//! Filter functions operate on premultiplied floating-point scratch pixels.
use super::blend;
use serde::Deserialize;
pub type Pixel = [f64; 4];
#[derive(Clone, Debug, Deserialize)]
pub struct Filter {
    pub kind: String,
    pub value: f64,
    #[serde(default)]
    pub offset: [f64; 2],
    #[serde(default)]
    pub color: Pixel,
}
impl Filter {
    pub fn valid(&self) -> bool {
        [
            "blur",
            "brightness",
            "contrast",
            "grayscale",
            "hue-rotate",
            "invert",
            "opacity",
            "saturate",
            "sepia",
            "drop-shadow",
        ]
        .contains(&self.kind.as_str())
            && self.value.is_finite()
            && (self.kind == "hue-rotate" || self.value >= 0.)
            && self.offset.iter().all(|v| v.is_finite())
            && self
                .color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    }
    pub fn pad(&self) -> f64 {
        if self.kind == "blur" {
            self.value.min(256.) * 3. + 2.
        } else if self.kind == "drop-shadow" {
            self.value.min(256.) * 3. + self.offset[0].abs().max(self.offset[1].abs()) + 2.
        } else {
            0.
        }
    }
}
pub fn apply(pixels: &mut Vec<Pixel>, w: usize, h: usize, filter: &Filter) {
    if filter.kind == "blur" {
        blur(pixels, w, h, filter.value);
        return;
    }
    if filter.kind == "drop-shadow" {
        let shadow = shadow(pixels, w, h, filter.value, filter.offset, filter.color);
        for (s, b) in pixels.iter_mut().zip(shadow) {
            *s = blend::composite(*s, b, "source-over");
        }
        return;
    }
    let v = filter.value;
    for p in pixels {
        if filter.kind == "opacity" {
            *p = p.map(|c| c * v.min(1.));
            continue;
        }
        if p[3] == 0. {
            continue;
        }
        let c = [p[0] / p[3], p[1] / p[3], p[2] / p[3]];
        let out = match filter.kind.as_str() {
            "brightness" => c.map(|c| c * v),
            "contrast" => c.map(|c| (c - 0.5) * v + 0.5),
            "invert" => c.map(|c| c * (1. - v.min(1.)) + (1. - c) * v.min(1.)),
            "grayscale" | "saturate" => {
                let s = if filter.kind == "grayscale" {
                    1. - v.min(1.)
                } else {
                    v
                };
                let l = c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722;
                c.map(|c| l + s * (c - l))
            }
            "sepia" => {
                let s = v.min(1.);
                let sepia = [
                    0.393 * c[0] + 0.769 * c[1] + 0.189 * c[2],
                    0.349 * c[0] + 0.686 * c[1] + 0.168 * c[2],
                    0.272 * c[0] + 0.534 * c[1] + 0.131 * c[2],
                ];
                std::array::from_fn(|i| c[i] * (1. - s) + sepia[i] * s)
            }
            "hue-rotate" => {
                let (s, t) = v.sin_cos();
                let matrix = [
                    [
                        0.213 + 0.787 * t - 0.213 * s,
                        0.715 - 0.715 * t - 0.715 * s,
                        0.072 - 0.072 * t + 0.928 * s,
                    ],
                    [
                        0.213 - 0.213 * t + 0.143 * s,
                        0.715 + 0.285 * t + 0.140 * s,
                        0.072 - 0.072 * t - 0.283 * s,
                    ],
                    [
                        0.213 - 0.213 * t - 0.787 * s,
                        0.715 - 0.715 * t + 0.715 * s,
                        0.072 + 0.928 * t + 0.072 * s,
                    ],
                ];
                matrix.map(|r| r[0] * c[0] + r[1] * c[1] + r[2] * c[2])
            }
            _ => c,
        };
        for i in 0..3 {
            p[i] = out[i].clamp(0., 1.) * p[3];
        }
    }
}
pub fn shadow(
    source: &[Pixel],
    w: usize,
    h: usize,
    sigma: f64,
    offset: [f64; 2],
    color: Pixel,
) -> Vec<Pixel> {
    let mut pixels: Vec<Pixel> = source.iter().map(|p| [0., 0., 0., p[3]]).collect();
    blur(&mut pixels, w, h, sigma);
    let mut out = vec![[0.; 4]; source.len()];
    for y in 0..h {
        for x in 0..w {
            let a = sample(&pixels, w, h, x as f64 - offset[0], y as f64 - offset[1])[3] * color[3];
            out[y * w + x] = [color[0] * a, color[1] * a, color[2] * a, a];
        }
    }
    out
}
fn sample(pixels: &[Pixel], w: usize, h: usize, x: f64, y: f64) -> Pixel {
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let at = |x: i64, y: i64| {
        if x >= 0 && y >= 0 && x < w as i64 && y < h as i64 {
            pixels[y as usize * w + x as usize]
        } else {
            [0.; 4]
        }
    };
    let (a, b, c, d) = (
        at(ix, iy),
        at(ix + 1, iy),
        at(ix, iy + 1),
        at(ix + 1, iy + 1),
    );
    std::array::from_fn(|i| {
        (a[i] * (1. - fx) + b[i] * fx) * (1. - fy) + (c[i] * (1. - fx) + d[i] * fx) * fy
    })
}
/// Three-box Gaussian approximation, constant work per pixel independent of radius.
/// SVG's approximation has three centered odd boxes or two offset even boxes and
/// one centered odd box. Transparent samples outside the scratch bitmap contribute zero.
fn blur(pixels: &mut Vec<Pixel>, w: usize, h: usize, sigma: f64) {
    let d = (sigma.min(256.) * 3. * (2. * std::f64::consts::PI).sqrt() / 4. + 0.5).floor() as usize;
    if sigma <= 4. {
        if sigma <= 0. {
            return;
        }
        // Small radii use the discrete Gaussian. Three boxes converge for large
        // sigma, but introduce noticeable error for small-radius combinations.
        let radius = (sigma * 3.).ceil() as i32;
        let mut kernel: Vec<f64> = (-radius..=radius)
            .map(|i| (-(i * i) as f64 / (2. * sigma * sigma)).exp())
            .collect();
        let sum: f64 = kernel.iter().sum();
        for v in &mut kernel {
            *v /= sum;
        }
        let mut out = vec![[0.; 4]; pixels.len()];
        for horizontal in [true, false] {
            for y in 0..h {
                for x in 0..w {
                    let mut p = [0.; 4];
                    for (index, k) in kernel.iter().enumerate() {
                        let delta = index as i32 - radius;
                        let (sx, sy) = if horizontal {
                            (x as i32 + delta, y as i32)
                        } else {
                            (x as i32, y as i32 + delta)
                        };
                        if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 {
                            for j in 0..4 {
                                p[j] += pixels[sy as usize * w + sx as usize][j] * k;
                            }
                        }
                    }
                    out[y * w + x] = p;
                }
            }
            std::mem::swap(pixels, &mut out);
        }
        return;
    }
    let boxes = if d % 2 == 1 {
        [(d / 2, d / 2); 3]
    } else {
        [(d / 2, d / 2 - 1), (d / 2 - 1, d / 2), (d / 2, d / 2)]
    };
    let mut scratch = vec![[0.; 4]; pixels.len()];
    for horizontal in [true, false] {
        for (left, right) in boxes {
            let (lines, len) = if horizontal { (h, w) } else { (w, h) };
            let at = |line: usize, i: usize| {
                if horizontal {
                    line * w + i
                } else {
                    i * w + line
                }
            };
            let divisor = (left + right + 1) as f64;
            for line in 0..lines {
                let mut sum = [0.; 4];
                for i in 0..=right.min(len - 1) {
                    for (j, v) in sum.iter_mut().enumerate() {
                        *v += pixels[at(line, i)][j];
                    }
                }
                for i in 0..len {
                    scratch[at(line, i)] = sum.map(|v| (v / divisor).max(0.));
                    if i >= left {
                        for (j, v) in sum.iter_mut().enumerate() {
                            *v -= pixels[at(line, i - left)][j];
                        }
                    }
                    if i + right + 1 < len {
                        for (j, v) in sum.iter_mut().enumerate() {
                            *v += pixels[at(line, i + right + 1)][j];
                        }
                    }
                }
            }
            std::mem::swap(pixels, &mut scratch);
        }
    }
}
