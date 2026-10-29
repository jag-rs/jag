//! Canvas paint coordinate spaces and straight-alpha sRGB interpolation.
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
#[derive(Clone, Debug)]
pub struct PatternImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum Paint {
    #[serde(rename = "solid")]
    Solid { color: [f64; 4] },
    #[serde(rename = "linear")]
    Linear {
        args: Vec<f64>,
        matrix: [f64; 6],
        stops: Vec<(f64, [f64; 4])>,
    },
    #[serde(rename = "radial")]
    Radial {
        args: Vec<f64>,
        matrix: [f64; 6],
        stops: Vec<(f64, [f64; 4])>,
    },
    #[serde(rename = "conic")]
    Conic {
        args: Vec<f64>,
        matrix: [f64; 6],
        stops: Vec<(f64, [f64; 4])>,
    },
    #[serde(rename = "pattern")]
    Pattern {
        image: u64,
        matrix: [f64; 6],
        repeat: String,
    },
}
impl Paint {
    pub fn sample(&self, x: f64, y: f64, images: &HashMap<u64, PatternImage>) -> [f64; 4] {
        let (args, matrix, stops) = match self {
            Self::Solid { color } => return *color,
            Self::Linear {
                args,
                matrix,
                stops,
            }
            | Self::Radial {
                args,
                matrix,
                stops,
            }
            | Self::Conic {
                args,
                matrix,
                stops,
            } => (args, matrix, stops),
            Self::Pattern {
                image,
                matrix,
                repeat,
            } => {
                let Some(p) = images.get(image) else {
                    return [0.; 4];
                };
                if p.width == 0
                    || p.height == 0
                    || (p.width as usize)
                        .checked_mul(p.height as usize)
                        .and_then(|v| v.checked_mul(4))
                        != Some(p.pixels.len())
                {
                    return [0.; 4];
                }
                let Some((x, y)) = inverse(*matrix, x, y) else {
                    return [0.; 4];
                };
                // Bilinear filtering, transparent outside non-repeated axes.
                let (x, y) = (x - 0.5, y - 0.5);
                let (ix, iy) = (x.floor(), y.floor());
                let (fx, fy) = (x - ix, y - iy);
                let sample = |x: f64, y: f64| {
                    let (w, h) = (p.width as i64, p.height as i64);
                    let (mut x, mut y) = (x as i64, y as i64);
                    if repeat == "repeat" || repeat == "repeat-x" {
                        x = x.rem_euclid(w);
                    }
                    if repeat == "repeat" || repeat == "repeat-y" {
                        y = y.rem_euclid(h);
                    }
                    if x < 0 || y < 0 || x >= w || y >= h {
                        return [0.; 4];
                    }
                    let i = (y * w + x) as usize * 4;
                    std::array::from_fn(|j| f64::from(p.pixels[i + j]) / 255.)
                };
                let (a, b, c, d) = (
                    sample(ix, iy),
                    sample(ix + 1., iy),
                    sample(ix, iy + 1.),
                    sample(ix + 1., iy + 1.),
                );
                let mut out: [f64; 4] = std::array::from_fn(|j| {
                    (a[j] * (1. - fx) + b[j] * fx) * (1. - fy) + (c[j] * (1. - fx) + d[j] * fx) * fy
                });
                if out[3] > 0. {
                    for j in 0..3 {
                        out[j] /= out[3];
                    }
                }
                return out;
            }
        };
        let Some((x, y)) = inverse(*matrix, x, y) else {
            return [0.; 4];
        };
        let t = match self {
            Self::Linear { .. } => {
                let (dx, dy) = (args[2] - args[0], args[3] - args[1]);
                let n = dx * dx + dy * dy;
                if n == 0. {
                    return [0.; 4];
                }
                ((x - args[0]) * dx + (y - args[1]) * dy) / n
            }
            Self::Conic { .. } => {
                ((y - args[2]).atan2(x - args[1]) - args[0]).rem_euclid(std::f64::consts::TAU)
                    / std::f64::consts::TAU
            }
            Self::Radial { .. } => {
                let (px, py, dx, dy, dr) = (
                    x - args[0],
                    y - args[1],
                    args[3] - args[0],
                    args[4] - args[1],
                    args[5] - args[2],
                );
                let (a, b, c) = (
                    dx * dx + dy * dy - dr * dr,
                    -2. * (px * dx + py * dy + args[2] * dr),
                    px * px + py * py - args[2] * args[2],
                );
                let mut roots = Vec::new();
                if a.abs() < 1e-12 {
                    if b != 0. {
                        roots.push(-c / b);
                    }
                } else {
                    let discriminant = b * b - 4. * a * c;
                    if discriminant >= 0. {
                        let s = discriminant.sqrt();
                        roots.extend([(-b - s) / (2. * a), (-b + s) / (2. * a)]);
                    }
                }
                let Some(t) = roots
                    .into_iter()
                    .filter(|t| args[2] + *t * dr >= 0.)
                    .max_by(f64::total_cmp)
                else {
                    return [0.; 4];
                };
                t
            }
            _ => unreachable!(),
        };
        interpolate(stops, t)
    }
}
fn interpolate(stops: &[(f64, [f64; 4])], t: f64) -> [f64; 4] {
    if stops.is_empty() {
        return [0.; 4];
    }
    if t < stops[0].0 {
        return stops[0].1;
    }
    for pair in stops.windows(2) {
        if t < pair[1].0 {
            let k = (t - pair[0].0) / (pair[1].0 - pair[0].0);
            return std::array::from_fn(|j| pair[0].1[j] * (1. - k) + pair[1].1[j] * k);
        }
    }
    stops.last().unwrap().1
}
fn inverse(m: [f64; 6], x: f64, y: f64) -> Option<(f64, f64)> {
    let d = m[0] * m[3] - m[1] * m[2];
    if d == 0. || !d.is_finite() {
        return None;
    }
    let (x, y) = (x - m[4], y - m[5]);
    Some(((m[3] * x - m[2] * y) / d, (-m[1] * x + m[0] * y) / d))
}
impl Paint {
    pub fn valid(&self) -> bool {
        let color = |c: &[f64; 4]| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v));
        match self {
            Self::Solid { color: c } => color(c),
            Self::Pattern { matrix, repeat, .. } => {
                matrix.iter().all(|n| n.is_finite())
                    && ["repeat", "repeat-x", "repeat-y", "no-repeat"].contains(&repeat.as_str())
            }
            Self::Linear {
                args,
                matrix,
                stops,
            }
            | Self::Radial {
                args,
                matrix,
                stops,
            }
            | Self::Conic {
                args,
                matrix,
                stops,
            } => {
                let len = match self {
                    Self::Linear { .. } => 4,
                    Self::Radial { .. } => 6,
                    _ => 3,
                };
                args.len() == len
                    && args.iter().all(|n| n.is_finite())
                    && matrix.iter().all(|n| n.is_finite())
                    && stops.len() <= 4096
                    && stops
                        .iter()
                        .all(|(n, c)| n.is_finite() && (0.0..=1.0).contains(n) && color(c))
                    && stops.windows(2).all(|w| w[0].0 <= w[1].0)
            }
        }
    }
}
