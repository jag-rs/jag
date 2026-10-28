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
    return encode_output(shade(inp));
}

fn shade(inp: VsOut) -> vec4<f32> {
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

/// Separable Gaussian blur for RGBA or single-channel targets.
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
    var acc: vec4<f32> = vec4<f32>(0.0);
    var norm: f32 = 0.0;
    for (var i: i32 = -r; i <= r; i = i + 1) {
        let fi = f32(i);
        let w = gauss(fi, sigma);
        let ofs = params.dir * params.texel * fi;
        let c = textureSample(in_tex, in_smp, inp.uv + ofs);
        acc = acc + c * w;
        norm = norm + w;
    }
    return acc / max(1e-6, norm);
}
"#;

pub const COLOR_FILTER_WGSL: &str = r#"
struct Params {
    row0: vec4<f32>,
    row1: vec4<f32>,
    row2: vec4<f32>,
    row3: vec4<f32>,
    bias: vec4<f32>,
};
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var uvs = array<vec2<f32>, 3>(vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0));
    return VsOut(vec4(positions[index], 0.0, 1.0), uvs[index]);
}

fn linear_to_srgb(value: vec3<f32>) -> vec3<f32> {
    return select(12.92 * value, 1.055 * pow(value, vec3(1.0 / 2.4)) - 0.055, value > vec3(0.0031308));
}

fn srgb_to_linear(value: vec3<f32>) -> vec3<f32> {
    return select(value / 12.92, pow((value + 0.055) / 1.055, vec3(2.4)), value > vec3(0.04045));
}

@fragment fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    // The filter matrix applies to sRGB values; a gamma-blend target's
    // source already holds them.
    let premul = textureSample(source, source_sampler, input.uv);
    let unpremul = premul.rgb / max(premul.a, 0.00001);
    let straight = vec4(select(linear_to_srgb(unpremul), unpremul, GAMMA_BLEND), premul.a);
    let filtered = clamp(vec4(
        dot(params.row0, straight), dot(params.row1, straight),
        dot(params.row2, straight), dot(params.row3, straight)
    ) + params.bias, vec4(0.0), vec4(1.0));
    let rgb = select(srgb_to_linear(filtered.rgb), filtered.rgb, GAMMA_BLEND);
    return vec4(rgb * filtered.a, filtered.a);
}
"#;

pub const DROP_SHADOW_FILTER_WGSL: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var blurred: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;
struct Params { offset_uv: vec2<f32>, _pad: vec2<f32>, color: vec4<f32> };
@group(0) @binding(3) var<uniform> params: Params;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var uvs = array<vec2<f32>, 3>(vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0));
    return VsOut(vec4(positions[index], 0.0, 1.0), uvs[index]);
}
fn linear_to_srgb(value: vec3<f32>) -> vec3<f32> {
    return select(12.92 * value, 1.055 * pow(value, vec3(1.0 / 2.4)) - 0.055, value > vec3(0.0031308));
}
fn srgb_to_linear(value: vec3<f32>) -> vec3<f32> {
    return select(value / 12.92, pow((value + 0.055) / 1.055, vec3(2.4)), value > vec3(0.04045));
}
@fragment fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    // Composites in sRGB; a gamma-blend target's source already holds it.
    let sampled = textureSample(source, source_sampler, input.uv);
    let unpremul = sampled.rgb / max(sampled.a, 0.00001);
    let source_rgb = select(linear_to_srgb(unpremul), unpremul, GAMMA_BLEND);
    let source_srgb = vec4(source_rgb * sampled.a, sampled.a);
    let shadow_uv = input.uv - params.offset_uv;
    let inside = all(shadow_uv >= vec2(0.0)) && all(shadow_uv <= vec2(1.0));
    let mask = select(0.0, textureSample(blurred, source_sampler, shadow_uv).a, inside);
    let shadow_alpha = mask * params.color.a;
    let shadow = vec4(params.color.rgb * shadow_alpha, shadow_alpha);
    let result = source_srgb + shadow * (1.0 - source_srgb.a);
    if (GAMMA_BLEND) {
        return result;
    }
    let straight = result.rgb / max(result.a, 0.00001);
    return vec4(srgb_to_linear(straight) * result.a, result.a);
}
"#;

pub const MASK_FILTER_WGSL: &str = r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var mask: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;
struct Params {
    surface_origin: vec2<f32>,
    surface_size: vec2<f32>,
    inverse_x: vec4<f32>,
    inverse_y: vec4<f32>,
    paint_rect: vec4<f32>,
    tile_rect: vec4<f32>,
    tile_step: vec2<f32>,
    extra_flags: vec2<u32>,
    flags: vec4<u32>,
};
@group(0) @binding(3) var<uniform> params: Params;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var uvs = array<vec2<f32>, 3>(vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0));
    return VsOut(vec4(positions[index], 0.0, 1.0), uvs[index]);
}
@fragment fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let source_color = textureSample(source, source_sampler, input.uv);
    let world = params.surface_origin + input.uv * params.surface_size;
    let local = vec2(
        dot(params.inverse_x.xyz, vec3(world, 1.0)),
        dot(params.inverse_y.xyz, vec3(world, 1.0))
    );
    let paint_offset = local - params.paint_rect.xy;
    let inside_paint = all(paint_offset >= vec2(0.0))
        && all(paint_offset <= params.paint_rect.zw);
    let raw_offset = local - params.tile_rect.xy;
    let repeated_offset = raw_offset - floor(raw_offset / params.tile_step) * params.tile_step;
    let offset = vec2(
        select(raw_offset.x, repeated_offset.x, params.flags.y == 1u),
        select(raw_offset.y, repeated_offset.y, params.flags.z == 1u)
    );
    let inside_tile = all(offset >= vec2(0.0)) && all(offset <= params.tile_rect.zw);
    var mask_uv = offset / params.tile_rect.zw;
    mask_uv.y = select(mask_uv.y, 1.0 - mask_uv.y, params.flags.w == 1u);
    let inside = inside_paint && inside_tile;
    let mask_color = select(vec4(0.0), textureSample(mask, source_sampler, mask_uv), inside);
    let luminance = dot(mask_color.rgb, vec3(0.2126, 0.7152, 0.0722)) * mask_color.a;
    let coverage = select(mask_color.a, luminance, params.flags.x == 1u);
    return select(source_color * coverage, vec4(coverage), params.extra_flags.x == 1u);
}
"#;

/// Combine two already-resolved mask coverage surfaces using CSS mask-composite.
pub const MASK_COMPOSITE_WGSL: &str = r#"
@group(0) @binding(0) var accumulated: texture_2d<f32>;
@group(0) @binding(1) var next_layer: texture_2d<f32>;
@group(0) @binding(2) var source_sampler: sampler;
@group(0) @binding(3) var<uniform> operation: vec4<u32>;

struct VsOut { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var uvs = array<vec2<f32>, 3>(vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0));
    return VsOut(vec4(positions[index], 0.0, 1.0), uvs[index]);
}
@fragment fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let top = textureSample(accumulated, source_sampler, input.uv).a;
    let below = textureSample(next_layer, source_sampler, input.uv).a;
    var coverage = top + below - top * below;
    if operation.x == 1u { coverage = top * (1.0 - below); }
    if operation.x == 2u { coverage = top * below; }
    if operation.x == 3u { coverage = top + below - 2.0 * top * below; }
    return vec4(coverage);
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
    return encode_output(vec4<f32>(sc.color.rgb * a, sc.color.a * a));
}
"#;
