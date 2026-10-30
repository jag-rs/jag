//! W3C compositing in premultiplied encoded RGB. Clip coverage is applied last.
pub const MODES: &[&str] = &[
    "source-over",
    "source-in",
    "source-out",
    "source-atop",
    "destination-over",
    "destination-in",
    "destination-out",
    "destination-atop",
    "lighter",
    "copy",
    "xor",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "color-dodge",
    "color-burn",
    "hard-light",
    "soft-light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
];
pub fn composite(s: [f64; 4], d: [f64; 4], mode: &str) -> [f64; 4] {
    let (a, b) = (s[3], d[3]);
    let factors = match mode {
        "source-over" => Some((1., 1. - a)),
        "source-in" => Some((b, 0.)),
        "source-out" => Some((1. - b, 0.)),
        "source-atop" => Some((b, 1. - a)),
        "destination-over" => Some((1. - b, 1.)),
        "destination-in" => Some((0., a)),
        "destination-out" => Some((0., 1. - a)),
        "destination-atop" => Some((1. - b, a)),
        "copy" => Some((1., 0.)),
        "xor" => Some((1. - b, 1. - a)),
        "lighter" => Some((1., 1.)),
        _ => None,
    };
    if let Some((fs, fd)) = factors {
        return std::array::from_fn(|i| (s[i] * fs + d[i] * fd).clamp(0., 1.));
    }
    let cs = std::array::from_fn(|i| if a > 0. { s[i] / a } else { 0. });
    let cb = std::array::from_fn(|i| if b > 0. { d[i] / b } else { 0. });
    let blend = match mode {
        "hue" => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        "saturation" => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        "color" => set_lum(cs, lum(cb)),
        "luminosity" => set_lum(cb, lum(cs)),
        _ => std::array::from_fn(|i| channel(cb[i], cs[i], mode)),
    };
    [
        (1. - a) * d[0] + (1. - b) * s[0] + a * b * blend[0],
        (1. - a) * d[1] + (1. - b) * s[1] + a * b * blend[1],
        (1. - a) * d[2] + (1. - b) * s[2] + a * b * blend[2],
        a + b - a * b,
    ]
}
fn channel(b: f64, s: f64, mode: &str) -> f64 {
    match mode {
        "multiply" => b * s,
        "screen" => b + s - b * s,
        "overlay" => channel(s, b, "hard-light"),
        "darken" => b.min(s),
        "lighten" => b.max(s),
        "color-dodge" => {
            if b == 0. {
                0.
            } else if s == 1. {
                1.
            } else {
                (b / (1. - s)).min(1.)
            }
        }
        "color-burn" => {
            if b == 1. {
                1.
            } else if s == 0. {
                0.
            } else {
                1. - ((1. - b) / s).min(1.)
            }
        }
        "hard-light" => {
            if s <= 0.5 {
                2. * b * s
            } else {
                1. - 2. * (1. - b) * (1. - s)
            }
        }
        "soft-light" => {
            if s <= 0.5 {
                b - (1. - 2. * s) * b * (1. - b)
            } else {
                let d = if b <= 0.25 {
                    ((16. * b - 12.) * b + 4.) * b
                } else {
                    b.sqrt()
                };
                b + (2. * s - 1.) * (d - b)
            }
        }
        "difference" => (b - s).abs(),
        "exclusion" => b + s - 2. * b * s,
        _ => s,
    }
}
fn lum(c: [f64; 3]) -> f64 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}
fn sat(c: [f64; 3]) -> f64 {
    c.into_iter().fold(0., f64::max) - c.into_iter().fold(1., f64::min)
}
fn set_lum(mut c: [f64; 3], l: f64) -> [f64; 3] {
    let delta = l - lum(c);
    c = c.map(|v| v + delta);
    let n = c.into_iter().fold(f64::INFINITY, f64::min);
    let x = c.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if n < 0. {
        c = c.map(|v| l + (v - l) * l / (l - n));
    }
    if x > 1. {
        c = c.map(|v| l + (v - l) * (1. - l) / (x - l));
    }
    c
}
fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let mut indices = [0, 1, 2];
    indices.sort_by(|a, b| c[*a].total_cmp(&c[*b]));
    let [lo, mid, hi] = indices;
    let mut out = [0.; 3];
    if c[hi] > c[lo] {
        out[mid] = (c[mid] - c[lo]) * s / (c[hi] - c[lo]);
        out[hi] = s;
    }
    out
}
