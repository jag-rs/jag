//! engine-shaders: WGSL shader sources and helpers.

/// Output encoding for shaders that generate color.
///
/// Pipelines receive premultiplied linear colors. A target that encodes sRGB
/// on write (`*UnormSrgb`) blends them in linear light. A target that does not
/// stores whatever the shader writes and blends it as-is, so these shaders
/// write sRGB-encoded premultiplied color and the GPU then blends in sRGB, as
/// browsers composite CSS. [`with_blend_space`] prepends this with the mode.
const BLEND_SPACE_WGSL: &str = r#"
fn jag_linear_to_srgb(value: vec3<f32>) -> vec3<f32> {
    let magnitude = abs(value);
    return sign(value) * select(12.92 * magnitude, 1.055 * pow(magnitude, vec3(1.0 / 2.4)) - 0.055, magnitude > vec3(0.0031308));
}

fn jag_srgb_to_linear(value: vec3<f32>) -> vec3<f32> {
    let magnitude = abs(value);
    return sign(value) * select(magnitude / 12.92, pow((magnitude + 0.055) / 1.055, vec3(2.4)), magnitude > vec3(0.04045));
}

fn decode_output(color: vec4<f32>) -> vec4<f32> {
    if (color.a <= 0.0) { return color; }
    return vec4(jag_srgb_to_linear(color.rgb / color.a) * color.a, color.a);
}

// Premultiplied linear color, re-expressed in the target's blend space.
fn encode_output(color: vec4<f32>) -> vec4<f32> {
    if (!GAMMA_BLEND || color.a <= 0.0) {
        return color;
    }
    // Project each source into the sRGB page gamut before CSS blending.
    let straight = clamp(color.rgb / color.a, vec3(0.0), vec3(1.0));
    return vec4(jag_linear_to_srgb(straight) * color.a, color.a);
}
"#;

/// Prefix a color-generating shader with its blend-space helpers.
/// `gamma_blend` is true when the pipeline's color target does not encode
/// sRGB on write (see [`BLEND_SPACE_WGSL`]).
pub fn with_blend_space(source: &str, gamma_blend: bool) -> String {
    format!("const GAMMA_BLEND: bool = {gamma_blend};\n{BLEND_SPACE_WGSL}\n{source}")
}
