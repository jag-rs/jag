//! Analytic rounded-box drop-shadow coverage.
//!
//! Computes the coverage (`0..=1`) of a Gaussian-blurred, rounded rectangle at
//! a point. This is the closed-form drop shadow used by browsers: convolving a
//! filled rounded rect with a 2D Gaussian. The result follows Evan Wallace's
//! method — the convolution is analytic along the x axis (an error-function
//! integral of the box span) and a few Gaussian-weighted samples across the y
//! axis trace the corner curve.
//!
//! Reference: <https://madebyevan.com/shaders/fast-rounded-rectangle-shadows/>
//!
//! CSS defines `blur-radius` as `2σ`, so callers pass `sigma = blur / 2`.
//!
//! This is the CPU twin of the GPU shadow shader (added in a later slice).
//! Keeping the two in lockstep — same sample count, same math — lets the
//! profile be validated headlessly before any rasterization, and gives a
//! reference for the GPU port. `Y_SAMPLES` here MUST match the shader's loop.

/// Number of Gaussian-weighted samples taken across the y axis to trace the
/// corner curve. Evan Wallace reports four is visually sufficient; the GPU
/// shader uses the same count so CPU and GPU agree.
pub const Y_SAMPLES: usize = 4;

/// Error function via Abramowitz & Stegun 7.1.26 (`|error| < 1.5e-7`). The
/// coefficients are that formula's constants, not free parameters. Evaluated
/// in f64 because they carry more precision than f32 represents.
#[inline]
fn erf(x: f32) -> f32 {
    let z = (x as f64).abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * z);
    let poly = t
        * (0.254_829_592
            + t * (-0.284_496_736
                + t * (1.421_413_741 + t * (-1.453_152_027 + t * 1.061_405_429))));
    let erf = 1.0 - poly * (-z * z).exp();
    (if x >= 0.0 { erf } else { -erf }) as f32
}

/// Unit-area Gaussian with standard deviation `sigma`, evaluated at `x`.
#[inline]
fn gaussian(x: f32, sigma: f32) -> f32 {
    let two_pi = 2.0 * std::f32::consts::PI;
    (-(x * x) / (2.0 * sigma * sigma)).exp() / (two_pi.sqrt() * sigma)
}

/// Coverage of the 1D span `[lo, hi]` convolved with a Gaussian of standard
/// deviation `sigma`, evaluated at `p`: `Φ((hi-p)/σ) - Φ((lo-p)/σ)`.
#[inline]
fn span_coverage(p: f32, lo: f32, hi: f32, sigma: f32) -> f32 {
    let inv = 1.0 / (sigma * std::f32::consts::SQRT_2);
    0.5 * (erf((hi - p) * inv) - erf((lo - p) * inv))
}

/// Horizontal half-width of the rounded rect at vertical offset `y_local` from
/// the center: full `half_x` in the straight section, curving inward by the
/// circular corner once `|y_local|` enters the corner band.
#[inline]
fn rounded_half_width(y_local: f32, half: [f32; 2], corner: f32) -> f32 {
    // `delta` is how far `|y_local|` has pushed past the straight section into
    // the corner band (0 while still in the straight section).
    let delta = (half[1] - corner - y_local.abs()).min(0.0);
    half[0] - corner + (corner * corner - delta * delta).max(0.0).sqrt()
}

/// Coverage of a sharp (un-blurred) rounded rect at a centered point — used
/// when `sigma` is ~0 so the Gaussian collapses to a hard edge.
#[inline]
fn sharp_inside(px: f32, py: f32, half: [f32; 2], corner: f32) -> f32 {
    if px.abs() > half[0] || py.abs() > half[1] {
        return 0.0;
    }
    if corner <= 0.0 {
        return 1.0;
    }
    // Inside the corner band on both axes: test against the corner circle.
    let dx = px.abs() - (half[0] - corner);
    let dy = py.abs() - (half[1] - corner);
    if dx <= 0.0 || dy <= 0.0 {
        return 1.0;
    }
    if dx * dx + dy * dy <= corner * corner {
        1.0
    } else {
        0.0
    }
}

/// Coverage (`0..=1`) of a rounded rect spanning `lower..upper` (top-left to
/// bottom-right, same coordinate space as `point`), blurred by a Gaussian of
/// standard deviation `sigma`, with circular corners of radius `corner`.
///
/// `corner` is clamped to half the shorter side. `sigma <= 0` yields the sharp
/// (un-blurred) mask.
pub fn rounded_box_shadow_coverage(
    lower: [f32; 2],
    upper: [f32; 2],
    point: [f32; 2],
    sigma: f32,
    corner: f32,
) -> f32 {
    let half = [(upper[0] - lower[0]) * 0.5, (upper[1] - lower[1]) * 0.5];
    if half[0] <= 0.0 || half[1] <= 0.0 {
        return 0.0;
    }
    let corner = corner.clamp(0.0, half[0].min(half[1]));
    let center = [(lower[0] + upper[0]) * 0.5, (lower[1] + upper[1]) * 0.5];
    let px = point[0] - center[0];
    let py = point[1] - center[1];

    if sigma <= 0.0 {
        return sharp_inside(px, py, half, corner);
    }

    // The Gaussian tail past 3σ is negligible; only integrate the y range where
    // the box and the kernel overlap.
    let low = py - half[1];
    let high = py + half[1];
    let start = (-3.0 * sigma).clamp(low, high);
    let end = (3.0 * sigma).clamp(low, high);
    let step = (end - start) / Y_SAMPLES as f32;
    if step <= 0.0 {
        return 0.0;
    }

    let mut y = start + step * 0.5;
    let mut value = 0.0;
    for _ in 0..Y_SAMPLES {
        let curved = rounded_half_width(py - y, half, corner);
        value += span_coverage(px, -curved, curved, sigma) * gaussian(y, sigma) * step;
        y += step;
    }
    value.clamp(0.0, 1.0)
}
