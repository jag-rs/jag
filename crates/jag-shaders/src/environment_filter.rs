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
