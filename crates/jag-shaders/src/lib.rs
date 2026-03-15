//! engine-shaders: WGSL shader sources and helpers.

/// Common WGSL snippet shared across shaders.
pub const COMMON_WGSL: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};
"#;

/// Solid color pipeline for macOS: un-premultiply for straight alpha blending
pub const SOLID_WGSL_MACOS: &str = r#"
struct ViewportUniform {
    scale: vec2<f32>,         // 2/W, -2/H
    translate: vec2<f32>,     // (-1, +1)
    scroll_offset: vec2<f32>, // GPU-side scroll (logical px)
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> vp: ViewportUniform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) in_pos: vec2<f32>, @location(1) in_color: vec4<f32>) -> VsOut {
    var out: VsOut;
    let scrolled = in_pos + vp.scroll_offset;
    let ndc = vec2<f32>(scrolled.x * vp.scale.x + vp.translate.x,
                        scrolled.y * vp.scale.y + vp.translate.y);
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.color = in_color; // premultiplied linear color
    return out;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    // Un-premultiply for straight alpha blending on Metal
    let alpha = inp.color.a;
    if (alpha > 0.001) {
        return vec4<f32>(inp.color.rgb / alpha, alpha);
    }
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
"#;

/// Solid color pipeline: vertices carry color in linear space (premultiplied alpha).
pub const SOLID_WGSL: &str = r#"
struct ViewportUniform {
    scale: vec2<f32>,         // 2/W, -2/H
    translate: vec2<f32>,     // (-1, +1)
    scroll_offset: vec2<f32>, // GPU-side scroll (logical px)
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> vp: ViewportUniform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(@location(0) in_pos: vec2<f32>, @location(1) in_color: vec4<f32>, @location(2) in_z_index: f32) -> VsOut {
    var out: VsOut;
    // in_pos is in local/layout pixel coordinates (y-down)
    let scrolled = in_pos + vp.scroll_offset;
    let ndc = vec2<f32>(scrolled.x * vp.scale.x + vp.translate.x,
                        scrolled.y * vp.scale.y + vp.translate.y);
    // Convert z-index to depth [0.0, 1.0]
    // HIGHER z-index = closer = LOWER depth value (rendered on top)
    // Negate z to invert the mapping: z=30 -> depth closer to 0.0, z=10 -> depth closer to 1.0
    let depth = (-clamp(in_z_index, -1000000.0, 1000000.0) / 1000000.0) * 0.5 + 0.5;
    out.pos = vec4<f32>(ndc, depth, 1.0);
    out.color = in_color; // premultiplied linear color
    return out;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    return inp.color;
}
"#;

/// Gradient utilities (structure only; evaluated in linear space)
pub const GRADIENT_WGSL: &str = r#"
struct Stop { pos: f32, color: vec4<f32> }; // premultiplied linear RGBA

fn eval_linear_gradient(stops: array<Stop>, t: f32) -> vec4<f32> {
    // Naive two-stop mix for illustration; full implementation will handle N stops.
    let clamped = clamp(t, 0.0, 1.0);
    // Assume two stops for now
    let a = stops[0];
    let b = stops[1];
    let tt = (clamped - a.pos) / max(1e-6, (b.pos - a.pos));
    return mix(a.color, b.color, clamp(tt, 0.0, 1.0));
}
"#;

/// Fullscreen textured compositor (premultiplied alpha expected).
pub const COMPOSITOR_WGSL: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

@group(0) @binding(0) var in_tex: texture_2d<f32>;
@group(0) @binding(1) var in_smp: sampler;

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    // Flip V to account for render-target vs texture sampling coord systems
    let uv = vec2<f32>(inp.uv.x, 1.0 - inp.uv.y);
    let c = textureSample(in_tex, in_smp, uv);
    return c; // premultiplied color flows through
}
"#;

/// Fast blit shader for copying intermediate texture to surface (no filtering, nearest neighbor).
/// This is optimized for the resize use case where we want the fastest possible copy.
pub const BLIT_WGSL: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    // Map NDC to UV with correct orientation without needing a fragment flip
    // (-1,-1) -> (0,1), (3,-1) -> (2,1), (-1,3) -> (0,-1)
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

@group(0) @binding(0) var in_tex: texture_2d<f32>;
@group(0) @binding(1) var in_smp: sampler;

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    return textureSample(in_tex, in_smp, inp.uv);
}
"#;

/// SMAA-inspired post-process with separate edge, weight, and resolve passes.
pub const SMAA_WGSL: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Params {
    texel_size: vec2<f32>,
    _pad: vec2<f32>,
};

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

// --- Edge detection pass ---
@group(0) @binding(0) var color_tex: texture_2d<f32>;
@group(0) @binding(1) var color_smp: sampler;
@group(0) @binding(2) var<uniform> params: Params;

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.299, 0.587, 0.114));
}

@fragment
fn fs_edges(inp: VsOut) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(inp.uv.x, 1.0 - inp.uv.y);
    let texel = params.texel_size;
    let c = luma(textureSample(color_tex, color_smp, uv).rgb);
    let l = luma(textureSample(color_tex, color_smp, uv + vec2(-texel.x, 0.0)).rgb);
    let r = luma(textureSample(color_tex, color_smp, uv + vec2(texel.x, 0.0)).rgb);
    let u = luma(textureSample(color_tex, color_smp, uv + vec2(0.0, -texel.y)).rgb);
    let d = luma(textureSample(color_tex, color_smp, uv + vec2(0.0, texel.y)).rgb);

    let dx = abs(r - l);
    let dy = abs(u - d);
    let threshold = 0.05;
    let edge_v = clamp((dx - threshold) * 8.0, 0.0, 1.0);
    let edge_h = clamp((dy - threshold) * 8.0, 0.0, 1.0);
    return vec4<f32>(edge_v, edge_h, c, 1.0);
}

// --- Blend weight pass ---
@group(0) @binding(0) var edge_tex: texture_2d<f32>;
@group(0) @binding(1) var edge_smp: sampler;
@group(0) @binding(2) var<uniform> params_weights: Params;

@fragment
fn fs_weights(inp: VsOut) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(inp.uv.x, 1.0 - inp.uv.y);
    let texel = params_weights.texel_size;
    let edges = textureSample(edge_tex, edge_smp, uv).rg;

    // Spread weights along the dominant edge axis to approximate line length.
    var horiz = edges.y;
    var vert = edges.x;

    let left = textureSample(edge_tex, edge_smp, uv + vec2(-texel.x, 0.0)).y;
    let right = textureSample(edge_tex, edge_smp, uv + vec2(texel.x, 0.0)).y;
    let up = textureSample(edge_tex, edge_smp, uv + vec2(0.0, -texel.y)).x;
    let down = textureSample(edge_tex, edge_smp, uv + vec2(0.0, texel.y)).x;

    horiz = clamp((horiz + left + right) * 0.3333, 0.0, 1.0);
    vert = clamp((vert + up + down) * 0.3333, 0.0, 1.0);

    // Store weights in RG: R = vertical blend (left/right), G = horizontal blend (up/down)
    return vec4<f32>(vert, horiz, 0.0, 0.0);
}

// --- Resolve pass ---
@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_smp: sampler;
@group(0) @binding(2) var weight_tex: texture_2d<f32>;
@group(0) @binding(3) var weight_smp: sampler;
@group(0) @binding(4) var<uniform> params_resolve: Params;

@fragment
fn fs_resolve(inp: VsOut) -> @location(0) vec4<f32> {
    let uv = vec2<f32>(inp.uv.x, 1.0 - inp.uv.y);
    let texel = params_resolve.texel_size;
    let weights = textureSample(weight_tex, weight_smp, uv).rg;

    let base = textureSample(src_tex, src_smp, uv);
    let left = textureSample(src_tex, src_smp, uv + vec2(-texel.x, 0.0));
    let right = textureSample(src_tex, src_smp, uv + vec2(texel.x, 0.0));
    let up = textureSample(src_tex, src_smp, uv + vec2(0.0, -texel.y));
    let down = textureSample(src_tex, src_smp, uv + vec2(0.0, texel.y));

    let horiz = mix(base, 0.5 * (up + down), weights.y);
    let vert = mix(base, 0.5 * (left + right), weights.x);
    return mix(vert, horiz, 0.5);
}
"#;

/// Background fill (solid or linear gradient) drawn via fullscreen triangle.
pub const BACKGROUND_WGSL: &str = r#"
const MAX_STOPS: u32 = 8u;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    var out: VsOut;
    // Backgrounds render at max depth (1.0) to be behind everything else
    out.pos = vec4<f32>(pos[vi], 1.0, 1.0);
    out.uv = uv[vi];
    return out;
}

// Packed to 16-byte boundaries to avoid platform layout mismatches.
struct BgUniform {
    start_end: vec4<f32>,                // start.xy, end.xy
    center_radius_stop: vec4<f32>,       // center.xy, radius, stop_count (f32)
    flags: vec4<f32>,                    // x: mode(0/1/2), y: debug(0/1), z: aspect_ratio, w: unused
};

struct Stop { 
    pos: f32, 
    pad0: f32,
    pad1: f32, 
    pad2: f32,
    color: vec4<f32>,
};

@group(0) @binding(0) var<uniform> bg: BgUniform;
@group(0) @binding(1) var<uniform> stops: array<Stop, 8>;

fn eval_stops(t: f32) -> vec4<f32> {
    let stop_count: u32 = u32(bg.center_radius_stop.w + 0.5);
    
    // Handle edge cases
    if (stop_count == 0u) { 
        return vec4<f32>(1.0, 0.0, 1.0, 1.0); // Magenta for error
    }
    if (stop_count == 1u) { 
        return stops[0u].color; 
    }
    
    // Clamp t to valid range
    let t_clamped = clamp(t, 0.0, 1.0);
    
    // Before first stop
    if (t_clamped <= stops[0u].pos) { 
        return stops[0u].color; 
    }
    
    // After last stop
    let last_idx = stop_count - 1u;
    if (t_clamped >= stops[last_idx].pos) { 
        return stops[last_idx].color; 
    }
    
    // Between stops - find the right interval
    for (var i: u32 = 0u; i < last_idx; i = i + 1u) {
        let curr_stop = stops[i];
        let next_stop = stops[i + 1u];
        
        if (t_clamped >= curr_stop.pos && t_clamped <= next_stop.pos) {
            let range = next_stop.pos - curr_stop.pos;
            if (range < 1e-6) {
                // Stops are at same position, return current color
                return curr_stop.color;
            }
            let local_t = (t_clamped - curr_stop.pos) / range;
            return mix(curr_stop.color, next_stop.color, local_t);
        }
    }
    
    // Fallback - should never reach here
    return vec4<f32>(1.0, 1.0, 0.0, 1.0); // Yellow for error
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    // Normalize UVs to [0,1]
    let uv01 = inp.uv * 0.5;
    let start = bg.start_end.xy;
    let end   = bg.start_end.zw;
    let center = bg.center_radius_stop.xy;
    let radius = bg.center_radius_stop.z;
    let stop_count = u32(bg.center_radius_stop.w + 0.5);
    let mode = u32(bg.flags.x + 0.5);
    let debug = u32(bg.flags.y + 0.5);
    let aspect = bg.flags.z; // width / height

    if (mode == 0u) { return stops[0u].color; }
    if (mode == 1u) {
        let dir = end - start;
        let denom = max(1e-6, dot(dir, dir));
        let t = clamp(dot(uv01 - start, dir) / denom, 0.0, 1.0);
        return eval_stops(t);
    }
    // Radial gradient mode (mode == 2)
    // Aspect-correct radial distance so rings remain circular in screen space.
    // We normalize distances by the smaller screen dimension, so scale the
    // larger axis delta accordingly.
    let dx0 = uv01.x - center.x;
    let dy0 = uv01.y - center.y;
    var d: f32;
    if (aspect >= 1.0) {
        // width >= height: scale X by aspect (W/H)
        let dx = dx0 * aspect;
        d = sqrt(dx * dx + dy0 * dy0);
    } else {
        // height > width: scale Y by 1/aspect (H/W)
        let dy = dy0 / max(1e-6, aspect);
        d = sqrt(dx0 * dx0 + dy * dy);
    }
    let t = clamp(d / max(1e-6, radius), 0.0, 1.0);
    if (debug == 1u) {
        // Debug: show t value as grayscale
        return vec4<f32>(t, t, t, 1.0);
    }
    return eval_stops(t);
} 
"#;

/// Separable Gaussian blur for single-channel mask (R channel). Output is written to the target
/// format; when using `R8Unorm`, only the R component is used.
pub const SHADOW_BLUR_WGSL: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

@group(0) @binding(0) var in_tex: texture_2d<f32>;
@group(0) @binding(1) var in_smp: sampler;

struct BlurParams {
    dir: vec2<f32>,      // direction (1,0) or (0,1)
    texel: vec2<f32>,    // 1/width, 1/height
    sigma: f32,          // blur sigma (radius ~ 3*sigma)
    _pad: f32,
};
@group(0) @binding(2) var<uniform> params: BlurParams;

fn gauss(w: f32, s: f32) -> f32 { return exp(-0.5 * (w*w) / max(1e-6, s*s)); }

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    // Fixed radius based on sigma
    let sigma = max(0.25, params.sigma);
    // Use a slightly wider kernel to better match CSS box-shadow tails
    // and avoid a "band" look at modest blur values.
    let r = i32(clamp(ceil(6.0 * sigma), 1.0, 64.0));
    var acc: f32 = 0.0;
    var norm: f32 = 0.0;
    for (var i: i32 = -r; i <= r; i = i + 1) {
        let fi = f32(i);
        let w = gauss(fi, sigma);
        let ofs = params.dir * params.texel * fi;
        let c = textureSample(in_tex, in_smp, inp.uv + ofs).r;
        acc = acc + c * w;
        norm = norm + w;
    }
    let v = acc / max(1e-6, norm);
    return vec4<f32>(v, v, v, v);
}
"#;

/// Composite blurred mask tinted with a premultiplied color onto the target.
pub const SHADOW_COMPOSITE_WGSL: &str = r#"
struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uv = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(2.0, 0.0),
        vec2<f32>(0.0, 2.0),
    );
    var out: VsOut;
    out.pos = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = uv[vi];
    return out;
}

@group(0) @binding(0) var mask_tex: texture_2d<f32>;
@group(0) @binding(1) var mask_smp: sampler;

struct ShadowColor {
    color: vec4<f32>, // premultiplied linear RGBA
};
@group(0) @binding(2) var<uniform> sc: ShadowColor;

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    // Match compositor orientation: flip V when sampling render-targets as textures
    let uv = vec2<f32>(inp.uv.x, 1.0 - inp.uv.y);
    let a = textureSample(mask_tex, mask_smp, uv).r;
    return vec4<f32>(sc.color.rgb * a, sc.color.a * a);
} 
"#;

/// Text rendering shader: samples an RGB coverage mask (subpixel AA) and tints with a
/// premultiplied linear text color. The output is premultiplied.
///
/// Bindings:
/// - @group(0) @binding(0): Viewport uniform (shared layout with solids)
/// - @group(1) @binding(0): Mask texture (Rgba8Unorm or Rgba16Unorm)
/// - @group(1) @binding(1): Sampler (nearest recommended)
/// - @location(2): Per-vertex color (premultiplied linear RGBA)
pub const TEXT_WGSL: &str = r#"
struct ViewportUniform {
    scale: vec2<f32>,         // 2/W, -2/H
    translate: vec2<f32>,     // (-1, +1)
    scroll_offset: vec2<f32>, // GPU-side scroll (logical px)
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> vp: ViewportUniform;
@group(1) @binding(0) var<uniform> z_index: f32;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(inp: VsIn) -> VsOut {
    var out: VsOut;
    let scrolled = inp.pos + vp.scroll_offset;
    let ndc = vec2<f32>(scrolled.x * vp.scale.x + vp.translate.x,
                        scrolled.y * vp.scale.y + vp.translate.y);
    // Convert z-index to depth [0.0, 1.0]
    // HIGHER z-index = closer = LOWER depth value (rendered on top)
    let depth = (-clamp(z_index, -1000000.0, 1000000.0) / 1000000.0) * 0.5 + 0.5;
    out.pos = vec4<f32>(ndc, depth, 1.0);
    out.uv = inp.uv;
    out.color = inp.color;
    return out;
}

@group(2) @binding(0) var mask_tex: texture_2d<f32>;
@group(2) @binding(1) var mask_smp: sampler;

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    // Nearest sampling prevents color bleeding across subpixels
    let m = textureSample(mask_tex, mask_smp, inp.uv);

    // Detect if this is a color emoji or subpixel text.
    //
    // Subpixel text masks: RGB channels contain coverage values, A channel is 0 (unused).
    // Color emoji: RGBA contains actual premultiplied color data.
    //
    // Detection strategy: If alpha > 0, it's color emoji data. If alpha == 0 but RGB has
    // values, it's a subpixel text mask.
    let has_alpha = m.a > 0.0;
    let has_rgb = (m.r + m.g + m.b) > 0.0;

    if (has_alpha) {
        // Color emoji: mask contains premultiplied RGBA color.
        // Return as-is - the alpha channel controls blending.
        return m;
    } else if (has_rgb) {
        // Subpixel text: RGB contains coverage masks, alpha is unused.
        // Apply text color modulated by coverage.
        let rgb = vec3<f32>(inp.color.r * m.r, inp.color.g * m.g, inp.color.b * m.b);
        let cov = max(m.r, max(m.g, m.b));
        let a = inp.color.a * cov;
        return vec4<f32>(rgb, a);
    } else {
        // Fully transparent mask pixel: discard so we don't write depth for empty texels.
        discard;
    }
}
"#;

/// Image rendering shader: samples an sRGB texture, converts to linear automatically via
/// the sampler, and returns premultiplied color for correct blending. Intended for PNG/JPEG
/// raster images uploaded as `Rgba8UnormSrgb`.
///
/// Bindings:
/// - @group(0) @binding(0): Viewport uniform (shared layout with solids)
/// - @group(1) @binding(0): Source texture (Rgba8UnormSrgb)
/// - @group(1) @binding(1): Sampler (linear recommended)
pub const IMAGE_WGSL: &str = r#"
struct ViewportUniform {
    scale: vec2<f32>,         // 2/W, -2/H
    translate: vec2<f32>,     // (-1, +1)
    scroll_offset: vec2<f32>, // GPU-side scroll (logical px)
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> vp: ViewportUniform;
@group(1) @binding(0) var<uniform> z_index: f32;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(inp: VsIn) -> VsOut {
    var out: VsOut;
    let scrolled = inp.pos + vp.scroll_offset;
    let ndc = vec2<f32>(scrolled.x * vp.scale.x + vp.translate.x,
                        scrolled.y * vp.scale.y + vp.translate.y);
    // Convert z-index to depth [0.0, 1.0]
    // HIGHER z-index = closer = LOWER depth value (rendered on top)
    let depth = (-clamp(z_index, -1000000.0, 1000000.0) / 1000000.0) * 0.5 + 0.5;
    out.pos = vec4<f32>(ndc, depth, 1.0);
    out.uv = inp.uv;
    return out;
}

@group(2) @binding(0) var src_tex: texture_2d<f32>;
@group(2) @binding(1) var src_smp: sampler;

struct ImageParams {
    opacity: f32,
    premultiplied_input: f32,
    clip_enabled: f32,
    _pad1: f32,
    // Rounded-rect clip in device pixels: (x, y, width, height).
    clip_rect: vec4<f32>,
    // Per-corner radii in device pixels: (top-left, top-right, bottom-right, bottom-left).
    clip_radii: vec4<f32>,
};
@group(3) @binding(0) var<uniform> img_params: ImageParams;

// Signed distance to a rounded rectangle.
// `p` is the test point, `center` the rect center, `half` the half-extents,
// `r` the corner radius for the quadrant containing `p`.
fn rounded_box_sdf(p: vec2<f32>, center: vec2<f32>, half: vec2<f32>, radii: vec4<f32>) -> f32 {
    let q = p - center;
    // Pick the corner radius for this quadrant (tl, tr, br, bl).
    let r = select(
        select(radii.z, radii.w, q.x < 0.0),   // bottom: br or bl
        select(radii.y, radii.x, q.x < 0.0),   // top:    tr or tl
        q.y < 0.0
    );
    let d = abs(q) - half + vec2<f32>(r, r);
    return min(max(d.x, d.y), 0.0) + length(max(d, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(src_tex, src_smp, inp.uv);
    let is_premul = img_params.premultiplied_input > 0.5;
    let rgb_pm = select(c.rgb * c.a, c.rgb, is_premul);
    let out_alpha = c.a * img_params.opacity;
    var color = vec4<f32>(rgb_pm * img_params.opacity, out_alpha);

    // Rounded-rect clip: discard fragments outside the boundary.
    if img_params.clip_enabled > 0.5 {
        let cr = img_params.clip_rect;
        let center = vec2<f32>(cr.x + cr.z * 0.5, cr.y + cr.w * 0.5);
        let half = vec2<f32>(cr.z * 0.5, cr.w * 0.5);
        let d = rounded_box_sdf(inp.pos.xy, center, half, img_params.clip_radii);
        if d > 0.5 {
            discard;
        }
        // Anti-aliased edge (smooth over ~1px).
        let aa = 1.0 - smoothstep(-0.5, 0.5, d);
        color = vec4<f32>(color.rgb * aa, color.a * aa);
    }

    return color;
}
"#;

// ─── 3D Shaders ───────────────────────────────────────────────────────────

/// Blinn-Phong instanced 3D shader.
///
/// Vertex inputs (per-vertex): position, normal, color.
/// Vertex inputs (per-instance): model matrix (4 columns), instance color.
/// Bind group 0: CameraUniform (view_proj mat4x4, eye_pos vec3, pad).
/// Bind group 1: LightUniform  (direction vec3, color vec3, ambient, specular params).
///
/// To switch to PBR: add a new shader with bind group 2 (MaterialUniform) and
/// a Cook-Torrance fragment — groups 0 and 1 stay the same.
pub const SOLID_3D_WGSL: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_pos: vec3<f32>,
    _pad: f32,
};

struct LightUniform {
    direction: vec3<f32>,
    _pad0: f32,
    color: vec3<f32>,
    ambient: f32,
    specular_power: f32,
    specular_strength: f32,
    _pad1: vec2<f32>,
};

@group(0) @binding(0) var<uniform> cam: CameraUniform;
@group(1) @binding(0) var<uniform> light: LightUniform;

struct VsIn {
    // Per-vertex
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) v_color: vec4<f32>,
    // Per-instance
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) i_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(inp: VsIn) -> VsOut {
    let model = mat4x4<f32>(inp.model_0, inp.model_1, inp.model_2, inp.model_3);

    let world4 = model * vec4<f32>(inp.pos, 1.0);
    let world_pos = world4.xyz;

    // For rigid transforms (rotation + translation + uniform scale), the normal
    // can be transformed by the upper-left 3x3 of the model matrix.
    let model3 = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let world_normal = normalize(model3 * inp.normal);

    // Instance color overrides vertex color when instance alpha > 0.
    let base_color = select(inp.v_color, inp.i_color, inp.i_color.a > 0.0);

    var out: VsOut;
    out.clip_pos = cam.view_proj * world4;
    out.world_pos = world_pos;
    out.world_normal = world_normal;
    out.color = base_color;
    return out;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let N = normalize(inp.world_normal);
    let light_dir = normalize(light.direction);
    let V = normalize(cam.eye_pos - inp.world_pos);
    let H = normalize(light_dir + V);

    let diffuse = max(dot(N, light_dir), 0.0);
    let spec = pow(max(dot(N, H), 0.0), light.specular_power) * light.specular_strength;

    let lighting = light.ambient + diffuse * light.color + spec * light.color;
    let rgb = inp.color.rgb * lighting;

    return vec4<f32>(rgb, inp.color.a);
}
"#;

/// Flat (unlit) instanced 3D shader — debug toggle.
///
/// Same vertex inputs as `SOLID_3D_WGSL`, but the fragment shader outputs
/// the base color directly without any lighting calculation.
pub const FLAT_3D_WGSL: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_pos: vec3<f32>,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> cam: CameraUniform;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) v_color: vec4<f32>,
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) i_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(inp: VsIn) -> VsOut {
    let model = mat4x4<f32>(inp.model_0, inp.model_1, inp.model_2, inp.model_3);
    let world4 = model * vec4<f32>(inp.pos, 1.0);
    let base_color = select(inp.v_color, inp.i_color, inp.i_color.a > 0.0);

    var out: VsOut;
    out.clip_pos = cam.view_proj * world4;
    out.color = base_color;
    return out;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    return inp.color;
}
"#;

/// PBR (Cook-Torrance) instanced 3D shader with multi-light support.
///
/// Bind group 0: CameraUniform (view_proj mat4x4, eye_pos vec3).
/// Bind group 1: LightsUniform (ambient header + up to 8 lights).
/// Bind group 2: MaterialUniform (base_color, metallic, roughness, emissive).
///
/// Same vertex/instance inputs as the Blinn-Phong shader. Instance color
/// tints the material's base_color.
pub const PBR_3D_WGSL: &str = r#"
const PI: f32 = 3.14159265358979323846;
const MAX_LIGHTS: u32 = 8u;

// ─── Uniforms ─────────────────────────────────────────────────

struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_pos: vec3<f32>,
    _pad: f32,
};

struct LightEntry {
    pos_or_dir: vec3<f32>,
    light_type: f32,
    color: vec3<f32>,
    intensity: f32,
    params: vec4<f32>,
};

struct LightsUniform {
    ambient_color: vec3<f32>,
    count: u32,
    ambient_intensity: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    lights: array<LightEntry, 8>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    metallic: f32,
    roughness: f32,
    _pad0: vec2<f32>,
    emissive: vec3<f32>,
    _pad1: f32,
};

@group(0) @binding(0) var<uniform> cam: CameraUniform;
@group(1) @binding(0) var<uniform> lights: LightsUniform;
@group(2) @binding(0) var<uniform> material: MaterialUniform;

// ─── Vertex ───────────────────────────────────────────────────

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) v_color: vec4<f32>,
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) i_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(inp: VsIn) -> VsOut {
    let model = mat4x4<f32>(inp.model_0, inp.model_1, inp.model_2, inp.model_3);
    let world4 = model * vec4<f32>(inp.pos, 1.0);
    let model3 = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let world_normal = normalize(model3 * inp.normal);
    let base_color = select(inp.v_color, inp.i_color, inp.i_color.a > 0.0);

    var out: VsOut;
    out.clip_pos = cam.view_proj * world4;
    out.world_pos = world4.xyz;
    out.world_normal = world_normal;
    out.color = base_color;
    return out;
}

// ─── PBR Functions ────────────────────────────────────────────

// GGX/Trowbridge-Reitz Normal Distribution Function
fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}

// Schlick-GGX Geometry sub-function
fn geometry_schlick_ggx(n_dot_x: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_x / (n_dot_x * (1.0 - k) + k);
}

// Smith's method — combined geometry for light and view
fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    return geometry_schlick_ggx(n_dot_v, roughness) * geometry_schlick_ggx(n_dot_l, roughness);
}

// Fresnel-Schlick approximation
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

// Unpack spot direction from packed f32 (10-bit unsigned per component)
fn unpack_direction(packed: f32) -> vec3<f32> {
    let bits = bitcast<u32>(packed);
    let x = f32(bits & 0x3FFu) / 1023.0 * 2.0 - 1.0;
    let y = f32((bits >> 10u) & 0x3FFu) / 1023.0 * 2.0 - 1.0;
    let z = f32((bits >> 20u) & 0x3FFu) / 1023.0 * 2.0 - 1.0;
    return normalize(vec3<f32>(x, y, z));
}

// Point light attenuation with range falloff
fn attenuation(distance: f32, range: f32) -> f32 {
    let d2 = distance * distance;
    if range > 0.0 {
        let ratio = distance / range;
        let falloff = clamp(1.0 - ratio * ratio, 0.0, 1.0);
        return (falloff * falloff) / max(d2, 0.0001);
    }
    return 1.0 / max(d2, 0.0001);
}

// Compute Cook-Torrance BRDF for a single light contribution
fn cook_torrance(
    N: vec3<f32>,
    V: vec3<f32>,
    L: vec3<f32>,
    albedo: vec3<f32>,
    metallic: f32,
    roughness: f32,
    light_color: vec3<f32>,
    light_intensity: f32,
) -> vec3<f32> {
    let H = normalize(V + L);
    let n_dot_l = max(dot(N, L), 0.0);
    let n_dot_v = max(dot(N, V), 0.001);
    let n_dot_h = max(dot(N, H), 0.0);
    let h_dot_v = max(dot(H, V), 0.0);

    // Fresnel reflectance at normal incidence
    let f0 = mix(vec3<f32>(0.04, 0.04, 0.04), albedo, metallic);

    let D = distribution_ggx(n_dot_h, roughness);
    let G = geometry_smith(n_dot_v, n_dot_l, roughness);
    let F = fresnel_schlick(h_dot_v, f0);

    let numerator = D * G * F;
    let denominator = 4.0 * n_dot_v * n_dot_l + 0.0001;
    let specular = numerator / denominator;

    // Energy conservation
    let k_s = F;
    let k_d = (vec3<f32>(1.0) - k_s) * (1.0 - metallic);
    let diffuse = k_d * albedo / PI;

    return (diffuse + specular) * light_color * light_intensity * n_dot_l;
}

// ─── Fragment ─────────────────────────────────────────────────

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let N = normalize(inp.world_normal);
    let V = normalize(cam.eye_pos - inp.world_pos);

    let albedo = material.base_color.rgb * inp.color.rgb;
    let metallic = material.metallic;
    let roughness = max(material.roughness, 0.04);

    var lo = vec3<f32>(0.0);
    let count = min(lights.count, MAX_LIGHTS);

    for (var i = 0u; i < count; i = i + 1u) {
        let light = lights.lights[i];
        let lt = light.light_type;

        if lt < 0.5 {
            // Directional
            let L = normalize(light.pos_or_dir);
            lo += cook_torrance(N, V, L, albedo, metallic, roughness,
                                light.color, light.intensity);
        } else if lt < 1.5 {
            // Point
            let to_light = light.pos_or_dir - inp.world_pos;
            let dist = length(to_light);
            let L = to_light / max(dist, 0.0001);
            let atten = attenuation(dist, light.params.x);
            lo += cook_torrance(N, V, L, albedo, metallic, roughness,
                                light.color, light.intensity * atten);
        } else if lt < 2.5 {
            // Spot
            let to_light = light.pos_or_dir - inp.world_pos;
            let dist = length(to_light);
            let L = to_light / max(dist, 0.0001);
            let spot_dir = unpack_direction(light.params.w);
            let theta = dot(L, normalize(-spot_dir));
            let cos_inner = light.params.x;
            let cos_outer = light.params.y;
            let epsilon = cos_inner - cos_outer;
            let spot_factor = clamp((theta - cos_outer) / max(epsilon, 0.0001), 0.0, 1.0);
            let atten = attenuation(dist, light.params.z);
            lo += cook_torrance(N, V, L, albedo, metallic, roughness,
                                light.color, light.intensity * atten * spot_factor);
        }
    }

    let ambient = lights.ambient_color * albedo;
    let emissive = material.emissive;
    let color = ambient + lo + emissive;

    // Reinhard tonemap
    let mapped = color / (color + vec3<f32>(1.0));

    return vec4<f32>(mapped, inp.color.a * material.base_color.a);
}
"#;

// ═══════════════════════════════════════════════════════════════
// Skybox + IBL shaders
// ═══════════════════════════════════════════════════════════════

/// Skybox shader — renders a cubemap as the scene background.
///
/// Uses a fullscreen triangle with inverse view-projection to compute
/// world-space ray directions. Rendered after geometry with depth write
/// disabled and depth compare LessEqual (geometry clears to 1.0).
pub const SKYBOX_WGSL: &str = r#"
struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_pos: vec3<f32>,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> cam: CameraUniform;
@group(1) @binding(0) var env_texture: texture_cube<f32>;
@group(1) @binding(1) var env_sampler: sampler;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

// Fullscreen triangle: 3 vertices cover the entire viewport.
@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos[id], 1.0, 1.0);
    out.ndc = pos[id];
    return out;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    // Reconstruct world-space ray direction from NDC.
    let inv_vp = inverse_mat4(cam.view_proj);
    let near_pt = inv_vp * vec4<f32>(inp.ndc, 0.0, 1.0);
    let far_pt  = inv_vp * vec4<f32>(inp.ndc, 1.0, 1.0);
    let dir = normalize(far_pt.xyz / far_pt.w - near_pt.xyz / near_pt.w);
    let color = textureSample(env_texture, env_sampler, dir);
    return vec4<f32>(color.rgb, 1.0);
}

// Manual 4x4 matrix inverse (WGSL has no built-in inverse).
fn inverse_mat4(m: mat4x4<f32>) -> mat4x4<f32> {
    let a00 = m[0][0]; let a01 = m[0][1]; let a02 = m[0][2]; let a03 = m[0][3];
    let a10 = m[1][0]; let a11 = m[1][1]; let a12 = m[1][2]; let a13 = m[1][3];
    let a20 = m[2][0]; let a21 = m[2][1]; let a22 = m[2][2]; let a23 = m[2][3];
    let a30 = m[3][0]; let a31 = m[3][1]; let a32 = m[3][2]; let a33 = m[3][3];

    let b00 = a00*a11 - a01*a10;  let b01 = a00*a12 - a02*a10;
    let b02 = a00*a13 - a03*a10;  let b03 = a01*a12 - a02*a11;
    let b04 = a01*a13 - a03*a11;  let b05 = a02*a13 - a03*a12;
    let b06 = a20*a31 - a21*a30;  let b07 = a20*a32 - a22*a30;
    let b08 = a20*a33 - a23*a30;  let b09 = a21*a32 - a22*a31;
    let b10 = a21*a33 - a23*a31;  let b11 = a22*a33 - a23*a32;

    let det = b00*b11 - b01*b10 + b02*b09 + b03*b08 - b04*b07 + b05*b06;
    let inv_det = 1.0 / det;

    return mat4x4<f32>(
        vec4<f32>(
            ( a11*b11 - a12*b10 + a13*b09) * inv_det,
            (-a01*b11 + a02*b10 - a03*b09) * inv_det,
            ( a31*b05 - a32*b04 + a33*b03) * inv_det,
            (-a21*b05 + a22*b04 - a23*b03) * inv_det,
        ),
        vec4<f32>(
            (-a10*b11 + a12*b08 - a13*b07) * inv_det,
            ( a00*b11 - a02*b08 + a03*b07) * inv_det,
            (-a30*b05 + a32*b02 - a33*b01) * inv_det,
            ( a20*b05 - a22*b02 + a23*b01) * inv_det,
        ),
        vec4<f32>(
            ( a10*b10 - a11*b08 + a13*b06) * inv_det,
            (-a00*b10 + a01*b08 - a03*b06) * inv_det,
            ( a30*b04 - a31*b02 + a33*b00) * inv_det,
            (-a20*b04 + a21*b02 - a23*b00) * inv_det,
        ),
        vec4<f32>(
            (-a10*b09 + a11*b07 - a12*b06) * inv_det,
            ( a00*b09 - a01*b07 + a02*b06) * inv_det,
            (-a30*b03 + a31*b01 - a32*b00) * inv_det,
            ( a20*b03 - a21*b01 + a22*b00) * inv_det,
        ),
    );
}
"#;

/// Equirectangular-to-cubemap conversion shader.
///
/// Renders an equirectangular HDR image onto each face of a cubemap.
/// Used as a compute-like render pass (one face at a time).
pub const EQUIRECT_TO_CUBEMAP_WGSL: &str = r#"
struct FaceUniform {
    face_rotation: mat4x4<f32>,
};

@group(0) @binding(0) var equirect_tex: texture_2d<f32>;
@group(0) @binding(1) var equirect_sampler: sampler;
@group(1) @binding(0) var<uniform> face: FaceUniform;

const PI: f32 = 3.14159265358979323846;
const TWO_PI: f32 = 6.28318530717958647692;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos[id], 0.0, 1.0);
    out.ndc = pos[id];
    return out;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    // Map NDC to cube face direction.
    let dir3 = normalize((face.face_rotation * vec4<f32>(inp.ndc.x, inp.ndc.y, 1.0, 0.0)).xyz);

    // Convert direction to equirectangular UV.
    let phi = atan2(dir3.z, dir3.x);
    let theta = asin(clamp(dir3.y, -1.0, 1.0));
    let u = phi / TWO_PI + 0.5;
    let v = 0.5 - theta / PI;

    return textureSample(equirect_tex, equirect_sampler, vec2<f32>(u, v));
}
"#;

/// Irradiance convolution shader — diffuse IBL.
///
/// Convolves a cubemap with a cosine lobe to produce an irradiance map
/// for Lambertian diffuse lighting. One face rendered at a time.
pub const IRRADIANCE_WGSL: &str = r#"
const PI: f32 = 3.14159265358979323846;

struct FaceUniform {
    face_rotation: mat4x4<f32>,
};

@group(0) @binding(0) var env_texture: texture_cube<f32>;
@group(0) @binding(1) var env_sampler: sampler;
@group(1) @binding(0) var<uniform> face: FaceUniform;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos[id], 0.0, 1.0);
    out.ndc = pos[id];
    return out;
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let normal = normalize((face.face_rotation * vec4<f32>(inp.ndc.x, inp.ndc.y, 1.0, 0.0)).xyz);

    // Build tangent frame from normal.
    var up = vec3<f32>(0.0, 1.0, 0.0);
    if abs(normal.y) > 0.999 {
        up = vec3<f32>(0.0, 0.0, 1.0);
    }
    let right = normalize(cross(up, normal));
    let up2 = cross(normal, right);

    // Hemisphere integral via uniform sampling.
    var irradiance = vec3<f32>(0.0);
    let sample_delta: f32 = 0.025;
    var n_samples: f32 = 0.0;

    var phi: f32 = 0.0;
    loop {
        if phi >= 2.0 * PI { break; }
        var theta: f32 = 0.0;
        loop {
            if theta >= 0.5 * PI { break; }
            let sin_theta = sin(theta);
            let cos_theta = cos(theta);
            let tangent_sample = vec3<f32>(
                sin_theta * cos(phi),
                sin_theta * sin(phi),
                cos_theta,
            );
            let sample_dir = tangent_sample.x * right + tangent_sample.y * up2 + tangent_sample.z * normal;
            irradiance += textureSample(env_texture, env_sampler, sample_dir).rgb * cos_theta * sin_theta;
            n_samples += 1.0;
            theta += sample_delta;
        }
        phi += sample_delta;
    }

    irradiance = PI * irradiance / n_samples;
    return vec4<f32>(irradiance, 1.0);
}
"#;

/// Prefilter shader — specular IBL with roughness mip levels.
///
/// Importance-samples the GGX distribution to convolve the environment
/// cubemap at a given roughness level. Rendered per-face, per-mip.
pub const PREFILTER_WGSL: &str = r#"
const PI: f32 = 3.14159265358979323846;
const SAMPLE_COUNT: u32 = 1024u;

struct FaceUniform {
    face_rotation: mat4x4<f32>,
};

struct RoughnessUniform {
    roughness: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var env_texture: texture_cube<f32>;
@group(0) @binding(1) var env_sampler: sampler;
@group(1) @binding(0) var<uniform> face: FaceUniform;
@group(2) @binding(0) var<uniform> params: RoughnessUniform;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos[id], 0.0, 1.0);
    out.ndc = pos[id];
    return out;
}

// Van der Corput radical inverse (base 2).
fn radical_inverse(bits_in: u32) -> f32 {
    var bits = bits_in;
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return f32(bits) * 2.3283064365386963e-10;
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(n), radical_inverse(i));
}

fn importance_sample_ggx(xi: vec2<f32>, n: vec3<f32>, roughness: f32) -> vec3<f32> {
    let a = roughness * roughness;
    let phi = 2.0 * PI * xi.x;
    let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    let sin_theta = sqrt(1.0 - cos_theta * cos_theta);

    let h_tangent = vec3<f32>(cos(phi) * sin_theta, sin(phi) * sin_theta, cos_theta);

    var up = vec3<f32>(0.0, 1.0, 0.0);
    if abs(n.y) > 0.999 {
        up = vec3<f32>(0.0, 0.0, 1.0);
    }
    let tangent = normalize(cross(up, n));
    let bitangent = cross(n, tangent);

    return normalize(tangent * h_tangent.x + bitangent * h_tangent.y + n * h_tangent.z);
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let N = normalize((face.face_rotation * vec4<f32>(inp.ndc.x, inp.ndc.y, 1.0, 0.0)).xyz);
    let R = N;
    let V = R;
    let roughness = params.roughness;

    var prefiltered_color = vec3<f32>(0.0);
    var total_weight: f32 = 0.0;

    for (var i = 0u; i < SAMPLE_COUNT; i = i + 1u) {
        let xi = hammersley(i, SAMPLE_COUNT);
        let H = importance_sample_ggx(xi, N, roughness);
        let L = normalize(2.0 * dot(V, H) * H - V);
        let n_dot_l = max(dot(N, L), 0.0);

        if n_dot_l > 0.0 {
            prefiltered_color += textureSample(env_texture, env_sampler, L).rgb * n_dot_l;
            total_weight += n_dot_l;
        }
    }

    prefiltered_color /= max(total_weight, 0.001);
    return vec4<f32>(prefiltered_color, 1.0);
}
"#;

/// BRDF integration LUT shader.
///
/// Generates a 2D lookup table (NdotV × roughness → scale + bias)
/// for the split-sum approximation of the rendering equation.
pub const BRDF_LUT_WGSL: &str = r#"
const PI: f32 = 3.14159265358979323846;
const SAMPLE_COUNT: u32 = 1024u;

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) id: u32) -> VsOut {
    var pos = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VsOut;
    out.clip_pos = vec4<f32>(pos[id], 0.0, 1.0);
    // Map from NDC [-1,1] to UV [0,1].
    out.uv = pos[id] * 0.5 + 0.5;
    return out;
}

fn radical_inverse(bits_in: u32) -> f32 {
    var bits = bits_in;
    bits = (bits << 16u) | (bits >> 16u);
    bits = ((bits & 0x55555555u) << 1u) | ((bits & 0xAAAAAAAAu) >> 1u);
    bits = ((bits & 0x33333333u) << 2u) | ((bits & 0xCCCCCCCCu) >> 2u);
    bits = ((bits & 0x0F0F0F0Fu) << 4u) | ((bits & 0xF0F0F0F0u) >> 4u);
    bits = ((bits & 0x00FF00FFu) << 8u) | ((bits & 0xFF00FF00u) >> 8u);
    return f32(bits) * 2.3283064365386963e-10;
}

fn hammersley(i: u32, n: u32) -> vec2<f32> {
    return vec2<f32>(f32(i) / f32(n), radical_inverse(i));
}

fn importance_sample_ggx(xi: vec2<f32>, n: vec3<f32>, roughness: f32) -> vec3<f32> {
    let a = roughness * roughness;
    let phi = 2.0 * PI * xi.x;
    let cos_theta = sqrt((1.0 - xi.y) / (1.0 + (a * a - 1.0) * xi.y));
    let sin_theta = sqrt(1.0 - cos_theta * cos_theta);

    return vec3<f32>(cos(phi) * sin_theta, sin(phi) * sin_theta, cos_theta);
}

fn geometry_schlick_ggx_ibl(n_dot_v: f32, roughness: f32) -> f32 {
    let a = roughness;
    let k = (a * a) / 2.0;
    return n_dot_v / (n_dot_v * (1.0 - k) + k);
}

fn geometry_smith_ibl(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    return geometry_schlick_ggx_ibl(n_dot_v, roughness)
         * geometry_schlick_ggx_ibl(n_dot_l, roughness);
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let n_dot_v = max(inp.uv.x, 0.001);
    let roughness = max(inp.uv.y, 0.001);

    let V = vec3<f32>(sqrt(1.0 - n_dot_v * n_dot_v), 0.0, n_dot_v);
    let N = vec3<f32>(0.0, 0.0, 1.0);

    var a: f32 = 0.0;
    var b: f32 = 0.0;

    for (var i = 0u; i < SAMPLE_COUNT; i = i + 1u) {
        let xi = hammersley(i, SAMPLE_COUNT);
        let H = importance_sample_ggx(xi, N, roughness);
        let L = normalize(2.0 * dot(V, H) * H - V);

        let n_dot_l = max(L.z, 0.0);
        let n_dot_h = max(H.z, 0.0);
        let v_dot_h = max(dot(V, H), 0.0);

        if n_dot_l > 0.0 {
            let G = geometry_smith_ibl(n_dot_v, n_dot_l, roughness);
            let G_vis = (G * v_dot_h) / (n_dot_h * n_dot_v);
            let Fc = pow(1.0 - v_dot_h, 5.0);
            a += (1.0 - Fc) * G_vis;
            b += Fc * G_vis;
        }
    }

    a /= f32(SAMPLE_COUNT);
    b /= f32(SAMPLE_COUNT);

    return vec4<f32>(a, b, 0.0, 1.0);
}
"#;

/// PBR shader with IBL environment lighting.
///
/// Extends the base PBR shader with bind group 3 for environment maps:
/// irradiance cubemap (diffuse), prefiltered cubemap (specular), BRDF LUT.
pub const PBR_IBL_3D_WGSL: &str = r#"
const PI: f32 = 3.14159265358979323846;
const MAX_LIGHTS: u32 = 8u;
const MAX_PREFILTER_LOD: f32 = 4.0;

// ─── Uniforms ─────────────────────────────────────────────────

struct CameraUniform {
    view_proj: mat4x4<f32>,
    eye_pos: vec3<f32>,
    _pad: f32,
};

struct LightEntry {
    pos_or_dir: vec3<f32>,
    light_type: f32,
    color: vec3<f32>,
    intensity: f32,
    params: vec4<f32>,
};

struct LightsUniform {
    ambient_color: vec3<f32>,
    count: u32,
    ambient_intensity: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    lights: array<LightEntry, 8>,
};

struct MaterialUniform {
    base_color: vec4<f32>,
    metallic: f32,
    roughness: f32,
    _pad0: vec2<f32>,
    emissive: vec3<f32>,
    _pad1: f32,
};

struct EnvUniform {
    ibl_intensity: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var<uniform> cam: CameraUniform;
@group(1) @binding(0) var<uniform> lights: LightsUniform;
@group(2) @binding(0) var<uniform> material: MaterialUniform;
@group(3) @binding(0) var irradiance_map: texture_cube<f32>;
@group(3) @binding(1) var prefilter_map: texture_cube<f32>;
@group(3) @binding(2) var brdf_lut: texture_2d<f32>;
@group(3) @binding(3) var env_sampler: sampler;
@group(3) @binding(4) var<uniform> env_params: EnvUniform;

// ─── Vertex ───────────────────────────────────────────────────

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) v_color: vec4<f32>,
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    @location(7) i_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

@vertex
fn vs_main(inp: VsIn) -> VsOut {
    let model = mat4x4<f32>(inp.model_0, inp.model_1, inp.model_2, inp.model_3);
    let world4 = model * vec4<f32>(inp.pos, 1.0);
    let model3 = mat3x3<f32>(model[0].xyz, model[1].xyz, model[2].xyz);
    let world_normal = normalize(model3 * inp.normal);
    let base_color = select(inp.v_color, inp.i_color, inp.i_color.a > 0.0);

    var out: VsOut;
    out.clip_pos = cam.view_proj * world4;
    out.world_pos = world4.xyz;
    out.world_normal = world_normal;
    out.color = base_color;
    return out;
}

// ─── PBR Functions ────────────────────────────────────────────

fn distribution_ggx(n_dot_h: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let denom = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}

fn geometry_schlick_ggx(n_dot_x: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return n_dot_x / (n_dot_x * (1.0 - k) + k);
}

fn geometry_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    return geometry_schlick_ggx(n_dot_v, roughness) * geometry_schlick_ggx(n_dot_l, roughness);
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

fn fresnel_schlick_roughness(cos_theta: f32, f0: vec3<f32>, roughness: f32) -> vec3<f32> {
    return f0 + (max(vec3<f32>(1.0 - roughness), f0) - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

fn unpack_direction(packed: f32) -> vec3<f32> {
    let bits = bitcast<u32>(packed);
    let x = f32(bits & 0x3FFu) / 1023.0 * 2.0 - 1.0;
    let y = f32((bits >> 10u) & 0x3FFu) / 1023.0 * 2.0 - 1.0;
    let z = f32((bits >> 20u) & 0x3FFu) / 1023.0 * 2.0 - 1.0;
    return normalize(vec3<f32>(x, y, z));
}

fn attenuation(distance: f32, range: f32) -> f32 {
    let d2 = distance * distance;
    if range > 0.0 {
        let ratio = distance / range;
        let falloff = clamp(1.0 - ratio * ratio, 0.0, 1.0);
        return (falloff * falloff) / max(d2, 0.0001);
    }
    return 1.0 / max(d2, 0.0001);
}

fn cook_torrance(
    N: vec3<f32>, V: vec3<f32>, L: vec3<f32>,
    albedo: vec3<f32>, metallic: f32, roughness: f32,
    light_color: vec3<f32>, light_intensity: f32,
) -> vec3<f32> {
    let H = normalize(V + L);
    let n_dot_l = max(dot(N, L), 0.0);
    let n_dot_v = max(dot(N, V), 0.001);
    let n_dot_h = max(dot(N, H), 0.0);
    let h_dot_v = max(dot(H, V), 0.0);

    let f0 = mix(vec3<f32>(0.04, 0.04, 0.04), albedo, metallic);

    let D = distribution_ggx(n_dot_h, roughness);
    let G = geometry_smith(n_dot_v, n_dot_l, roughness);
    let F = fresnel_schlick(h_dot_v, f0);

    let numerator = D * G * F;
    let denominator = 4.0 * n_dot_v * n_dot_l + 0.0001;
    let specular = numerator / denominator;

    let k_s = F;
    let k_d = (vec3<f32>(1.0) - k_s) * (1.0 - metallic);
    let diffuse = k_d * albedo / PI;

    return (diffuse + specular) * light_color * light_intensity * n_dot_l;
}

// ─── Fragment ─────────────────────────────────────────────────

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    let N = normalize(inp.world_normal);
    let V = normalize(cam.eye_pos - inp.world_pos);
    let R = reflect(-V, N);
    let n_dot_v = max(dot(N, V), 0.001);

    let albedo = material.base_color.rgb * inp.color.rgb;
    let metallic = material.metallic;
    let roughness = max(material.roughness, 0.04);

    // Direct lighting (same as non-IBL PBR).
    var lo = vec3<f32>(0.0);
    let count = min(lights.count, MAX_LIGHTS);

    for (var i = 0u; i < count; i = i + 1u) {
        let light = lights.lights[i];
        let lt = light.light_type;

        if lt < 0.5 {
            let L = normalize(light.pos_or_dir);
            lo += cook_torrance(N, V, L, albedo, metallic, roughness,
                                light.color, light.intensity);
        } else if lt < 1.5 {
            let to_light = light.pos_or_dir - inp.world_pos;
            let dist = length(to_light);
            let L = to_light / max(dist, 0.0001);
            let atten = attenuation(dist, light.params.x);
            lo += cook_torrance(N, V, L, albedo, metallic, roughness,
                                light.color, light.intensity * atten);
        } else if lt < 2.5 {
            let to_light = light.pos_or_dir - inp.world_pos;
            let dist = length(to_light);
            let L = to_light / max(dist, 0.0001);
            let spot_dir = unpack_direction(light.params.w);
            let theta = dot(L, normalize(-spot_dir));
            let cos_inner = light.params.x;
            let cos_outer = light.params.y;
            let epsilon = cos_inner - cos_outer;
            let spot_factor = clamp((theta - cos_outer) / max(epsilon, 0.0001), 0.0, 1.0);
            let atten = attenuation(dist, light.params.z);
            lo += cook_torrance(N, V, L, albedo, metallic, roughness,
                                light.color, light.intensity * atten * spot_factor);
        }
    }

    // IBL ambient: diffuse irradiance + specular prefilter.
    let f0 = mix(vec3<f32>(0.04, 0.04, 0.04), albedo, metallic);
    let F_env = fresnel_schlick_roughness(n_dot_v, f0, roughness);
    let k_s_env = F_env;
    let k_d_env = (vec3<f32>(1.0) - k_s_env) * (1.0 - metallic);

    let irradiance = textureSample(irradiance_map, env_sampler, N).rgb;
    let diffuse_ibl = k_d_env * irradiance * albedo;

    let prefilter_lod = roughness * MAX_PREFILTER_LOD;
    let prefiltered = textureSampleLevel(prefilter_map, env_sampler, R, prefilter_lod).rgb;
    let brdf = textureSample(brdf_lut, env_sampler, vec2<f32>(n_dot_v, roughness)).rg;
    let specular_ibl = prefiltered * (F_env * brdf.x + brdf.y);

    let ambient = (diffuse_ibl + specular_ibl) * env_params.ibl_intensity;
    let emissive = material.emissive;
    let color = ambient + lo + emissive;

    // Reinhard tonemap.
    let mapped = color / (color + vec3<f32>(1.0));

    return vec4<f32>(mapped, inp.color.a * material.base_color.a);
}
"#;
