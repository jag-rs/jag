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
