//! Validates the analytic rounded-box-shadow coverage against an independent
//! brute-force 2D Gaussian convolution (the ground truth a browser's shadow
//! approximates), and documents the linear-vs-sRGB compositing gap that the
//! GPU shadow pass must close.

use jag_draw::rounded_box_shadow_coverage as coverage;
use jag_draw::{
    BoxShadowSpec, ColorLinPremul, Rect, RoundedRadii, RoundedRect, ShadowInstance, Transform2D,
};

fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> RoundedRect {
    RoundedRect {
        rect: Rect { x, y, w, h },
        radii: RoundedRadii {
            tl: r,
            tr: r,
            br: r,
            bl: r,
        },
    }
}

fn spec(offset: [f32; 2], spread: f32, blur: f32) -> BoxShadowSpec {
    BoxShadowSpec {
        offset,
        spread,
        blur_radius: blur,
        color: ColorLinPremul {
            r: 0.3,
            g: 0.6,
            b: 0.2,
            a: 0.55,
        },
    }
}

#[test]
fn shadow_instance_identity_transform_geometry() {
    // 200x40 pill at (100,200), radius 20, offset (0,8), spread -10, blur 24.
    let inst = ShadowInstance::from_box_shadow(
        rrect(100.0, 200.0, 200.0, 40.0, 20.0),
        spec([0.0, 8.0], -10.0, 24.0),
        7,
        Transform2D::identity(),
        None,
    );
    // Bounds = rect + offset, expanded by spread (-10 shrinks).
    assert_eq!(inst.lower, [100.0 + 0.0 - -10.0, 200.0 + 8.0 - -10.0]); // [110, 218]
    assert_eq!(
        inst.upper,
        [100.0 + 200.0 + 0.0 + -10.0, 200.0 + 40.0 + 8.0 + -10.0]
    ); // [290, 238]
    assert!(
        (inst.params[0] - 12.0).abs() < 1e-4,
        "sigma = blur/2 = 12, got {}",
        inst.params[0]
    );
    // corner = (radius + spread).max(0) = 20 - 10 = 10.
    assert!(
        (inst.params[1] - 10.0).abs() < 1e-4,
        "corner, got {}",
        inst.params[1]
    );
    assert_eq!(inst.params[2], 7.0); // z
    assert_eq!(inst.color, [0.3, 0.6, 0.2, 0.55]);
}

#[test]
fn shadow_instance_translation_shifts_bounds_only() {
    let base = ShadowInstance::from_box_shadow(
        rrect(0.0, 0.0, 100.0, 40.0, 8.0),
        spec([0.0, 0.0], 0.0, 16.0),
        0,
        Transform2D::identity(),
        None,
    );
    let moved = ShadowInstance::from_box_shadow(
        rrect(0.0, 0.0, 100.0, 40.0, 8.0),
        spec([0.0, 0.0], 0.0, 16.0),
        0,
        Transform2D::translate(50.0, 30.0),
        None,
    );
    assert_eq!(moved.lower, [base.lower[0] + 50.0, base.lower[1] + 30.0]);
    assert_eq!(moved.upper, [base.upper[0] + 50.0, base.upper[1] + 30.0]);
    // sigma and corner unchanged under pure translation.
    assert!((moved.params[0] - base.params[0]).abs() < 1e-4);
    assert!((moved.params[1] - base.params[1]).abs() < 1e-4);
}

#[test]
fn shadow_instance_uniform_scale_scales_sigma_and_corner() {
    let inst = ShadowInstance::from_box_shadow(
        rrect(0.0, 0.0, 100.0, 40.0, 8.0),
        spec([0.0, 0.0], 0.0, 16.0),
        0,
        Transform2D::scale(2.0, 2.0),
        None,
    );
    // sigma = (16/2) * 2 = 16; corner = 8 * 2 = 16.
    assert!(
        (inst.params[0] - 16.0).abs() < 1e-3,
        "sigma, got {}",
        inst.params[0]
    );
    assert!(
        (inst.params[1] - 16.0).abs() < 1e-3,
        "corner, got {}",
        inst.params[1]
    );
    // Bounds scaled by 2.
    assert_eq!(inst.upper, [200.0, 80.0]);
}

#[test]
fn shadow_instance_sharp_box_has_zero_corner() {
    let inst = ShadowInstance::from_box_shadow(
        rrect(0.0, 0.0, 100.0, 40.0, 0.0),
        spec([0.0, 0.0], 4.0, 8.0),
        0,
        Transform2D::identity(),
        None,
    );
    assert_eq!(
        inst.params[1], 0.0,
        "sharp box keeps zero corner even with spread"
    );
}

#[test]
fn shadow_instance_transforms_clip_bounds() {
    let inst = ShadowInstance::from_box_shadow(
        rrect(0.0, 0.0, 100.0, 40.0, 8.0),
        spec([0.0, 0.0], 0.0, 16.0),
        0,
        Transform2D::translate(50.0, -30.0),
        Some(Rect {
            x: 10.0,
            y: 20.0,
            w: 70.0,
            h: 25.0,
        }),
    );

    assert_eq!(inst.clip_min, [60.0, -10.0]);
    assert_eq!(inst.clip_max, [130.0, 15.0]);
}

/// Brute-force ground truth: convolve the sharp rounded-rect mask with a 2D
/// Gaussian (std `sigma`) by dense sampling. Independent of the implementation
/// under test (no separable/erf shortcut), so agreement is meaningful.
fn brute_force_coverage(
    lower: [f32; 2],
    upper: [f32; 2],
    point: [f32; 2],
    sigma: f32,
    corner: f32,
) -> f32 {
    let inside = |x: f32, y: f32| -> f32 {
        if x < lower[0] || x > upper[0] || y < lower[1] || y > upper[1] {
            return 0.0;
        }
        // Corner circle test.
        let cx = if x < lower[0] + corner {
            lower[0] + corner
        } else if x > upper[0] - corner {
            upper[0] - corner
        } else {
            x
        };
        let cy = if y < lower[1] + corner {
            lower[1] + corner
        } else if y > upper[1] - corner {
            upper[1] - corner
        } else {
            y
        };
        let dx = x - cx;
        let dy = y - cy;
        if dx * dx + dy * dy <= corner * corner {
            1.0
        } else {
            0.0
        }
    };

    // Integrate over ±4σ around the point with a fine step.
    let reach = 4.0 * sigma;
    let n = 200; // 200x200 samples across the window
    let step = (2.0 * reach) / n as f32;
    let norm = 1.0 / (2.0 * std::f32::consts::PI * sigma * sigma);
    let mut acc = 0.0f32;
    for iy in 0..n {
        let sy = point[1] - reach + (iy as f32 + 0.5) * step;
        for ix in 0..n {
            let sx = point[0] - reach + (ix as f32 + 0.5) * step;
            let m = inside(sx, sy);
            if m == 0.0 {
                continue;
            }
            let dx = sx - point[0];
            let dy = sy - point[1];
            let g = norm * (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp();
            acc += m * g * step * step;
        }
    }
    acc.clamp(0.0, 1.0)
}

#[test]
fn sharp_box_has_unit_center_half_edge_and_zero_outside() {
    let sigma = 12.0; // blur 24 -> sigma 12

    // Deep inside a box much larger than the kernel (every edge >3σ away)
    // -> fully covered.
    let big_lo = [0.0, 0.0];
    let big_hi = [200.0, 200.0];
    let center = coverage(big_lo, big_hi, [100.0, 100.0], sigma, 0.0);
    assert!(
        center > 0.99,
        "center of a thick box should be ~1, got {center}"
    );

    let lower = [0.0, 0.0];
    let upper = [200.0, 40.0];

    // On the long edge, far from corners -> Gaussian half coverage (the far
    // edge is >3σ away so only the near half-plane contributes).
    let edge = coverage(lower, upper, [100.0, 40.0], sigma, 0.0);
    assert!(
        (edge - 0.5).abs() < 0.01,
        "edge coverage should be ~0.5, got {edge}"
    );

    // Far outside (>3σ) -> ~0.
    let outside = coverage(lower, upper, [100.0, 40.0 + 4.0 * sigma], sigma, 0.0);
    assert!(outside < 0.01, "far outside should be ~0, got {outside}");

    // A box only as tall as ~3σ is never fully covered even at its center —
    // the blur leaks both edges inward. Verify that thin-box behavior tracks
    // the independent brute-force convolution rather than naively assuming ~1.
    let thin_center = coverage(lower, upper, [100.0, 20.0], sigma, 0.0);
    let thin_truth = brute_force_coverage(lower, upper, [100.0, 20.0], sigma, 0.0);
    assert!(
        (thin_center - thin_truth).abs() < 0.02,
        "thin-box center {thin_center} should match brute force {thin_truth}"
    );
    assert!(
        thin_center < 0.95,
        "thin box center should be attenuated by the blur, got {thin_center}"
    );
}

#[test]
fn coverage_fades_monotonically_outward() {
    let lower = [0.0, 0.0];
    let upper = [200.0, 40.0];
    let sigma = 12.0;
    let mut prev = 1.0;
    let mut d = 0.0;
    while d <= 3.0 * sigma {
        let c = coverage(lower, upper, [100.0, 40.0 + d], sigma, 0.0);
        assert!(
            c <= prev + 1e-4,
            "coverage should not increase moving outward (d={d}): {c} > {prev}"
        );
        prev = c;
        d += 2.0;
    }
}

#[test]
fn analytic_matches_brute_force_for_rounded_corners() {
    let lower = [0.0, 0.0];
    let upper = [180.0, 44.0];
    let sigma = 10.0;
    let corner = 22.0; // pill-ish

    // Sample a grid of points spanning inside, the corner penumbra, and outside.
    let mut max_err = 0.0f32;
    let mut worst = ([0.0, 0.0], 0.0, 0.0);
    for &py in &[-10.0f32, 0.0, 10.0, 22.0, 34.0, 44.0, 54.0] {
        for &px in &[-10.0f32, 0.0, 20.0, 90.0, 160.0, 180.0, 190.0] {
            let p = [px, py];
            let got = coverage(lower, upper, p, sigma, corner);
            let truth = brute_force_coverage(lower, upper, p, sigma, corner);
            let err = (got - truth).abs();
            if err > max_err {
                max_err = err;
                worst = (p, got, truth);
            }
        }
    }
    // Evan Wallace's 4-sample y integral tracks the dense convolution closely.
    assert!(
        max_err < 0.03,
        "analytic coverage deviates from brute force by {max_err} at {:?} (got {}, truth {})",
        worst.0,
        worst.1,
        worst.2
    );
}

// --- sRGB transfer (IEC 61966-2-1) ---
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Documents the root cause this whole jag effort exists to fix: for the repro
/// glow `0 8px 24px -10px rgba(99,217,106,0.55)` on `#0c1810`, compositing the
/// SAME analytic coverage in linear space (what jag does today) is markedly
/// brighter than compositing in sRGB (what Chrome does). The analytic coverage
/// is correct; only the blend space is wrong — which is why slice 3 must
/// composite the shadow in gamma space.
#[test]
fn linear_composite_is_brighter_than_srgb_for_the_repro_glow() {
    // Shadow origin rect: button ~200x40 pill, blur 24 (sigma 12), spread -10
    // (shrinks the origin by 10 per side), offset_y 8. Evaluate a point in the
    // penumbra just below the button.
    let lower = [10.0, 18.0];
    let upper = [190.0, 30.0];
    let sigma = 12.0;
    let corner = 6.0;
    let point = [100.0, 40.0];

    let c = coverage(lower, upper, point, sigma, corner);
    assert!(
        c > 0.05 && c < 0.6,
        "expect a mid-penumbra coverage, got {c}"
    );

    let color_alpha = 0.55;
    let a = c * color_alpha;
    let shadow_g_srgb = 217.0 / 255.0;
    let bg_g_srgb = 24.0 / 255.0;

    // sRGB-space blend (Chrome).
    let srgb_g = 255.0 * (shadow_g_srgb * a + bg_g_srgb * (1.0 - a));

    // Linear-space blend (jag today): blend in linear, encode for display.
    let lin = srgb_to_linear(shadow_g_srgb) * a + srgb_to_linear(bg_g_srgb) * (1.0 - a);
    let linear_g = 255.0 * linear_to_srgb(lin);

    assert!(
        linear_g > srgb_g + 15.0,
        "linear composite ({linear_g}) should be much brighter than sRGB ({srgb_g}) \
         for the lime glow — this is the parity gap slice 3 closes"
    );
}
