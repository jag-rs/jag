//! Analytic box-shadow instance shaders (linear-blend + sRGB-composite).
//!
//! Extracted from `lib.rs` to keep that file's WGSL registry manageable.
//! Both constants are re-exported from the crate root.

/// Analytic rounded-box drop shadow, drawn one instanced quad per shadow.
///
/// The fragment shader evaluates the closed-form coverage of a Gaussian-blurred
/// rounded rectangle (Evan Wallace's method: an error-function integral along x,
/// `Y_SAMPLES` Gaussian-weighted samples across y to trace the corner curve) and
/// outputs `premul_color * coverage`. This is the GPU twin of
/// `jag_draw::rounded_box_shadow_coverage`; the math and `Y_SAMPLES` MUST stay
/// in lockstep with that CPU reference. CSS `blur-radius` is `2σ`, so the host
/// passes `sigma = blur / 2`.
///
/// Per-instance vertex attributes (see `ShadowInstance` on the Rust side):
///   loc 0: `lower` (xy)   shadow rect top-left, logical px (after offset+spread)
///   loc 1: `upper` (xy)   shadow rect bottom-right
///   loc 2: `params` (xyzw) = (sigma, corner_radius, z_index, _pad)
///   loc 3: `color`  (rgba) premultiplied linear shadow color
///   loc 4: `clip_min` (xy) clip rect top-left
///   loc 5: `clip_max` (xy) clip rect bottom-right
///   loc 6: `clip_radii` (tl, tr, br, bl) clip corner radii
pub const SHADOW_INSTANCE_WGSL: &str = r#"
struct ViewportUniform {
    scale: vec2<f32>,
    translate: vec2<f32>,
    scroll_offset: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> vp: ViewportUniform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) lower: vec2<f32>,
    @location(2) @interpolate(flat) upper: vec2<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
    @location(5) @interpolate(flat) clip_min: vec2<f32>,
    @location(6) @interpolate(flat) clip_max: vec2<f32>,
    @location(7) @interpolate(flat) clip_radii: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) lower: vec2<f32>,
    @location(1) upper: vec2<f32>,
    @location(2) params: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) clip_min: vec2<f32>,
    @location(5) clip_max: vec2<f32>,
    @location(6) clip_radii: vec4<f32>,
) -> VsOut {
    let sigma = params.x;
    // The Gaussian tail is negligible past 3σ; pad a little for the AA edge.
    let reach = 3.0 * max(sigma, 0.0) + 1.5;
    let lo = lower - vec2<f32>(reach, reach);
    let hi = upper + vec2<f32>(reach, reach);
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let world = mix(lo, hi, c);
    let scrolled = world + vp.scroll_offset;
    let ndc = vec2<f32>(scrolled.x * vp.scale.x + vp.translate.x,
                        scrolled.y * vp.scale.y + vp.translate.y);
    let depth = (-clamp(params.z, -1000000.0, 1000000.0) / 1000000.0) * 0.5 + 0.5;
    var out: VsOut;
    out.pos = vec4<f32>(ndc, depth, 1.0);
    out.world = world;
    out.lower = lower;
    out.upper = upper;
    out.params = params;
    out.color = color;
    out.clip_min = clip_min;
    out.clip_max = clip_max;
    out.clip_radii = clip_radii;
    return out;
}

// erf via Abramowitz & Stegun 7.1.26 (|error| < 1.5e-7).
fn erf1(x: f32) -> f32 {
    let s = sign(x);
    let z = abs(x);
    let t = 1.0 / (1.0 + 0.3275911 * z);
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741
        + t * (-1.453152027 + t * 1.061405429))));
    let e = 1.0 - poly * exp(-z * z);
    return s * e;
}

// Coverage of the 1D span [lo, hi] convolved with a Gaussian of std dev sigma at p.
fn span_cov(p: f32, lo: f32, hi: f32, sigma: f32) -> f32 {
    let inv = 1.0 / (sigma * 1.41421356);
    return 0.5 * (erf1((hi - p) * inv) - erf1((lo - p) * inv));
}

fn gaussian1(x: f32, sigma: f32) -> f32 {
    // 1 / sqrt(2*pi) = 0.3989422804
    return exp(-(x * x) / (2.0 * sigma * sigma)) * (0.3989422804 / sigma);
}

// Horizontal half-width of the rounded rect at vertical offset y_local.
fn half_width(y_local: f32, half_x: f32, half_y: f32, corner: f32) -> f32 {
    let delta = min(half_y - corner - abs(y_local), 0.0);
    return half_x - corner + sqrt(max(corner * corner - delta * delta, 0.0));
}

fn coverage(lower: vec2<f32>, upper: vec2<f32>, point: vec2<f32>, sigma: f32, corner_in: f32) -> f32 {
    let half_size = (upper - lower) * 0.5;
    if (half_size.x <= 0.0 || half_size.y <= 0.0) {
        return 0.0;
    }
    let corner = clamp(corner_in, 0.0, min(half_size.x, half_size.y));
    let center = (lower + upper) * 0.5;
    let p = point - center;

    if (sigma <= 0.0) {
        if (abs(p.x) > half_size.x || abs(p.y) > half_size.y) {
            return 0.0;
        }
        return 1.0;
    }

    let low = p.y - half_size.y;
    let high = p.y + half_size.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    if (step <= 0.0) {
        return 0.0;
    }
    var y = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        let curved = half_width(p.y - y, half_size.x, half_size.y, corner);
        value = value + span_cov(p.x, -curved, curved, sigma) * gaussian1(y, sigma) * step;
        y = y + step;
    }
    return clamp(value, 0.0, 1.0);
}

fn outside_rounded_clip(point: vec2<f32>, clip_min: vec2<f32>, clip_max: vec2<f32>, radii_in: vec4<f32>) -> bool {
    if (point.x < clip_min.x || point.x > clip_max.x ||
        point.y < clip_min.y || point.y > clip_max.y) {
        return true;
    }

    let size = max(clip_max - clip_min, vec2<f32>(0.0));
    let max_r = min(size.x, size.y) * 0.5;
    let radii = clamp(radii_in, vec4<f32>(0.0), vec4<f32>(max_r));

    let tl = radii.x;
    if (tl > 0.0 && point.x < clip_min.x + tl && point.y < clip_min.y + tl) {
        let center = vec2<f32>(clip_min.x + tl, clip_min.y + tl);
        let delta = point - center;
        return dot(delta, delta) > tl * tl;
    }

    let tr = radii.y;
    if (tr > 0.0 && point.x > clip_max.x - tr && point.y < clip_min.y + tr) {
        let center = vec2<f32>(clip_max.x - tr, clip_min.y + tr);
        let delta = point - center;
        return dot(delta, delta) > tr * tr;
    }

    let br = radii.z;
    if (br > 0.0 && point.x > clip_max.x - br && point.y > clip_max.y - br) {
        let center = vec2<f32>(clip_max.x - br, clip_max.y - br);
        let delta = point - center;
        return dot(delta, delta) > br * br;
    }

    let bl = radii.w;
    if (bl > 0.0 && point.x < clip_min.x + bl && point.y > clip_max.y - bl) {
        let center = vec2<f32>(clip_min.x + bl, clip_max.y - bl);
        let delta = point - center;
        return dot(delta, delta) > bl * bl;
    }

    return false;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    if (outside_rounded_clip(inp.world, inp.clip_min, inp.clip_max, inp.clip_radii)) {
        discard;
    }
    let cov = coverage(inp.lower, inp.upper, inp.world, inp.params.x, inp.params.y);
    // color is premultiplied linear; scaling by coverage keeps it premultiplied.
    return inp.color * cov;
}
"#;

/// Analytic box-shadow shader that composites in sRGB (gamma) space to match
/// Chrome, instead of the hardware-blended LINEAR `SHADOW_INSTANCE_WGSL`.
///
/// GPU fixed-function blending only works in the offscreen's stored (LINEAR)
/// space, which makes colored glows ~3.5x too bright vs a real browser. Chrome
/// composites shadows in sRGB. Since hardware can't blend in gamma, this shader
/// reads a SNAPSHOT of the destination (group 1 binding 0), performs the sRGB
/// straight-alpha "over" in-shader, and writes the full result. The pipeline
/// uses REPLACE blend (no hardware blend) so the shader output is authoritative.
///
/// The vertex stage, ViewportUniform, and all coverage/erf/gaussian helpers are
/// identical to `SHADOW_INSTANCE_WGSL`; only the fragment differs and a dst
/// texture binding is added.
///
/// Limitation: overlapping shadows all read the same pre-shadow snapshot, so
/// they do not accumulate against each other. This matches the single-snapshot
/// design and is an accepted trade-off.
pub const SHADOW_INSTANCE_COMPOSITE_WGSL: &str = r#"
struct ViewportUniform {
    scale: vec2<f32>,
    translate: vec2<f32>,
    scroll_offset: vec2<f32>,
    _pad: vec2<f32>,
};
@group(0) @binding(0) var<uniform> vp: ViewportUniform;

@group(1) @binding(0) var dst_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec2<f32>,
    @location(1) @interpolate(flat) lower: vec2<f32>,
    @location(2) @interpolate(flat) upper: vec2<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) color: vec4<f32>,
    @location(5) @interpolate(flat) clip_min: vec2<f32>,
    @location(6) @interpolate(flat) clip_max: vec2<f32>,
    @location(7) @interpolate(flat) clip_radii: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) lower: vec2<f32>,
    @location(1) upper: vec2<f32>,
    @location(2) params: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) clip_min: vec2<f32>,
    @location(5) clip_max: vec2<f32>,
    @location(6) clip_radii: vec4<f32>,
) -> VsOut {
    let sigma = params.x;
    // The Gaussian tail is negligible past 3σ; pad a little for the AA edge.
    let reach = 3.0 * max(sigma, 0.0) + 1.5;
    let lo = lower - vec2<f32>(reach, reach);
    let hi = upper + vec2<f32>(reach, reach);
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let c = corners[vi];
    let world = mix(lo, hi, c);
    let scrolled = world + vp.scroll_offset;
    let ndc = vec2<f32>(scrolled.x * vp.scale.x + vp.translate.x,
                        scrolled.y * vp.scale.y + vp.translate.y);
    let depth = (-clamp(params.z, -1000000.0, 1000000.0) / 1000000.0) * 0.5 + 0.5;
    var out: VsOut;
    out.pos = vec4<f32>(ndc, depth, 1.0);
    out.world = world;
    out.lower = lower;
    out.upper = upper;
    out.params = params;
    out.color = color;
    out.clip_min = clip_min;
    out.clip_max = clip_max;
    out.clip_radii = clip_radii;
    return out;
}

// erf via Abramowitz & Stegun 7.1.26 (|error| < 1.5e-7).
fn erf1(x: f32) -> f32 {
    let s = sign(x);
    let z = abs(x);
    let t = 1.0 / (1.0 + 0.3275911 * z);
    let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741
        + t * (-1.453152027 + t * 1.061405429))));
    let e = 1.0 - poly * exp(-z * z);
    return s * e;
}

// Coverage of the 1D span [lo, hi] convolved with a Gaussian of std dev sigma at p.
fn span_cov(p: f32, lo: f32, hi: f32, sigma: f32) -> f32 {
    let inv = 1.0 / (sigma * 1.41421356);
    return 0.5 * (erf1((hi - p) * inv) - erf1((lo - p) * inv));
}

fn gaussian1(x: f32, sigma: f32) -> f32 {
    // 1 / sqrt(2*pi) = 0.3989422804
    return exp(-(x * x) / (2.0 * sigma * sigma)) * (0.3989422804 / sigma);
}

// Horizontal half-width of the rounded rect at vertical offset y_local.
fn half_width(y_local: f32, half_x: f32, half_y: f32, corner: f32) -> f32 {
    let delta = min(half_y - corner - abs(y_local), 0.0);
    return half_x - corner + sqrt(max(corner * corner - delta * delta, 0.0));
}

fn coverage(lower: vec2<f32>, upper: vec2<f32>, point: vec2<f32>, sigma: f32, corner_in: f32) -> f32 {
    let half_size = (upper - lower) * 0.5;
    if (half_size.x <= 0.0 || half_size.y <= 0.0) {
        return 0.0;
    }
    let corner = clamp(corner_in, 0.0, min(half_size.x, half_size.y));
    let center = (lower + upper) * 0.5;
    let p = point - center;

    if (sigma <= 0.0) {
        if (abs(p.x) > half_size.x || abs(p.y) > half_size.y) {
            return 0.0;
        }
        return 1.0;
    }

    let low = p.y - half_size.y;
    let high = p.y + half_size.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    if (step <= 0.0) {
        return 0.0;
    }
    var y = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        let curved = half_width(p.y - y, half_size.x, half_size.y, corner);
        value = value + span_cov(p.x, -curved, curved, sigma) * gaussian1(y, sigma) * step;
        y = y + step;
    }
    return clamp(value, 0.0, 1.0);
}

fn outside_rounded_clip(point: vec2<f32>, clip_min: vec2<f32>, clip_max: vec2<f32>, radii_in: vec4<f32>) -> bool {
    if (point.x < clip_min.x || point.x > clip_max.x ||
        point.y < clip_min.y || point.y > clip_max.y) {
        return true;
    }

    let size = max(clip_max - clip_min, vec2<f32>(0.0));
    let max_r = min(size.x, size.y) * 0.5;
    let radii = clamp(radii_in, vec4<f32>(0.0), vec4<f32>(max_r));

    let tl = radii.x;
    if (tl > 0.0 && point.x < clip_min.x + tl && point.y < clip_min.y + tl) {
        let center = vec2<f32>(clip_min.x + tl, clip_min.y + tl);
        let delta = point - center;
        return dot(delta, delta) > tl * tl;
    }

    let tr = radii.y;
    if (tr > 0.0 && point.x > clip_max.x - tr && point.y < clip_min.y + tr) {
        let center = vec2<f32>(clip_max.x - tr, clip_min.y + tr);
        let delta = point - center;
        return dot(delta, delta) > tr * tr;
    }

    let br = radii.z;
    if (br > 0.0 && point.x > clip_max.x - br && point.y > clip_max.y - br) {
        let center = vec2<f32>(clip_max.x - br, clip_max.y - br);
        let delta = point - center;
        return dot(delta, delta) > br * br;
    }

    let bl = radii.w;
    if (bl > 0.0 && point.x < clip_min.x + bl && point.y > clip_max.y - bl) {
        let center = vec2<f32>(clip_min.x + bl, clip_max.y - bl);
        let delta = point - center;
        return dot(delta, delta) > bl * bl;
    }

    return false;
}

fn lin_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0/2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}
fn srgb_to_lin(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3<f32>(0.0)) + 0.055) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    if (outside_rounded_clip(inp.world, inp.clip_min, inp.clip_max, inp.clip_radii)) {
        discard;
    }
    let cov = coverage(inp.lower, inp.upper, inp.world, inp.params.x, inp.params.y);
    let color_a = inp.color.a;                       // shadow's own alpha
    // instance color is premultiplied LINEAR; recover straight linear then sRGB
    let straight_lin = select(vec3<f32>(0.0), inp.color.rgb / max(color_a, 1e-4), color_a > 0.0);
    let color_srgb = lin_to_srgb(clamp(straight_lin, vec3<f32>(0.0), vec3<f32>(1.0)));
    let a = cov * color_a;                            // effective shadow alpha here

    let dpix = vec2<i32>(i32(inp.pos.x), i32(inp.pos.y));
    let dst = textureLoad(dst_tex, dpix, 0);          // premultiplied LINEAR
    let dst_a = dst.a;
    let dst_straight_lin = select(vec3<f32>(0.0), dst.rgb / max(dst_a, 1e-4), dst_a > 0.0);
    let dst_srgb = lin_to_srgb(clamp(dst_straight_lin, vec3<f32>(0.0), vec3<f32>(1.0)));

    // sRGB-space straight-alpha "over"
    let out_srgb = color_srgb * a + dst_srgb * (1.0 - a);
    let out_a = a + dst_a * (1.0 - a);
    let out_lin = srgb_to_lin(clamp(out_srgb, vec3<f32>(0.0), vec3<f32>(1.0)));
    return vec4<f32>(out_lin * out_a, out_a);          // premultiplied LINEAR
}
"#;
