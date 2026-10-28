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
    return encode_output(inp.color);
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
