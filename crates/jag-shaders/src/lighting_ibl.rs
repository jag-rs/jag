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
