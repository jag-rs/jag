/// Backdrop blur over the already-rendered framebuffer.
pub const BACKDROP_BLUR_WGSL: &str = r#"
struct ViewportUniform {
    scale: vec2<f32>,
    translate: vec2<f32>,
    scroll_offset: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> vp: ViewportUniform;

struct Params {
    texel: vec2<f32>,
    viewport_size: vec2<f32>,
    radius: f32,
    logical: f32,
    _pad: vec2<f32>,
};

@group(1) @binding(0) var src_tex: texture_2d<f32>;
@group(1) @binding(1) var src_smp: sampler;
@group(1) @binding(2) var<uniform> params: Params;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) in_pos: vec2<f32>, @location(1) in_z_index: f32) -> VsOut {
    var out: VsOut;
    let scrolled = in_pos + vp.scroll_offset;
    let ndc = vec2<f32>(
        scrolled.x * vp.scale.x + vp.translate.x,
        scrolled.y * vp.scale.y + vp.translate.y
    );
    let depth = (-clamp(in_z_index, -1000000.0, 1000000.0) / 1000000.0) * 0.5 + 0.5;
    out.pos = vec4<f32>(ndc, depth, 1.0);
    out.uv = vec2<f32>(
        (scrolled.x * params.logical) / params.viewport_size.x,
        (scrolled.y * params.logical) / params.viewport_size.y
    );
    return out;
}

fn weight_for(offset: vec2<f32>) -> f32 {
    return exp(-0.5 * dot(offset, offset));
}

@fragment
fn fs_main(inp: VsOut) -> @location(0) vec4<f32> {
    if (params.radius <= 0.0) {
        return textureSample(src_tex, src_smp, inp.uv);
    }
    let step_uv = max(params.radius, 1.0) * 0.38 * params.texel;
    var acc: vec4<f32> = vec4<f32>(0.0);
    var total: f32 = 0.0;
    for (var y: i32 = -4; y <= 4; y = y + 1) {
        for (var x: i32 = -4; x <= 4; x = x + 1) {
            let ofs_cells = vec2<f32>(f32(x), f32(y));
            let w = weight_for(ofs_cells * 0.42);
            let uv = clamp(inp.uv + ofs_cells * step_uv, vec2<f32>(0.0), vec2<f32>(1.0));
            acc = acc + textureSample(src_tex, src_smp, uv) * w;
            total = total + w;
        }
    }
    return acc / max(total, 0.0001);
}
"#;

/// Text rendering shader: resolves RGB mask coverage to grayscale and tints with a
/// premultiplied linear text color. Single-source alpha blending cannot attenuate
/// the destination per subpixel, so both output color and alpha use mean coverage.
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
        // This pipeline uses single-source premultiplied alpha blending, not
        // per-channel LCD blending. Resolve to mean coverage for both color and
        // alpha: max(RGB) expands dark edges, while separate RGB tinting fringes
        // colored text. Equal-channel grayscale masks pass through unchanged.
        let coverage = dot(m.rgb, vec3<f32>(1.0 / 3.0));
        return encode_output(inp.color * coverage);
    } else {
        // Fully transparent mask pixel: discard so we don't write depth for empty texels.
        discard;
    }
    // `discard` above terminates the invocation, but the DirectX shader compiler
    // (FXC) rejects a fragment entry point unless every control path returns a
    // value (error X3507). Metal/Vulkan accept the discard-only path; Windows
    // does not. This trailing return is unreachable but keeps FXC happy.
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
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
    // 1 when sampling decodes to linear (an `*Srgb` texture); 0 when the
    // texture already holds values in the target's blend space.
    source_linear: f32,
    // Rounded-rect clip in device pixels: (x, y, width, height).
    clip_rect: vec4<f32>,
    // Per-corner radii in device pixels: (top-left, top-right, bottom-right, bottom-left).
    clip_radii: vec4<f32>,
};
@group(3) @binding(0) var<uniform> img_params: ImageParams;

// Filtering happens after color conversion and premultiplication in the page's
// blend space. Sampling a linear texture first would brighten scaled edges.
fn web_texel(p: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(src_tex));
    let c = textureLoad(src_tex, clamp(p, vec2<i32>(0), size - vec2<i32>(1)), 0);
    let pm = select(c.rgb * c.a, c.rgb, img_params.premultiplied_input > 0.5);
    return encode_output(vec4(pm, c.a));
}

fn web_sample(uv: vec2<f32>) -> vec4<f32> {
    let p = uv * vec2<f32>(textureDimensions(src_tex)) - vec2(0.5);
    let base = vec2<i32>(floor(p));
    let f = fract(p);
    let top = mix(web_texel(base), web_texel(base + vec2<i32>(1, 0)), f.x);
    let bottom = mix(web_texel(base + vec2<i32>(0, 1)), web_texel(base + vec2<i32>(1, 1)), f.x);
    return mix(top, bottom, f.y);
}

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
    let web_source = GAMMA_BLEND && img_params.source_linear > 0.5;
    var color: vec4<f32>;
    if web_source {
        color = web_sample(inp.uv) * img_params.opacity;
    } else {
        let c = textureSample(src_tex, src_smp, inp.uv);
        let is_premul = img_params.premultiplied_input > 0.5;
        let rgb_pm = select(c.rgb * c.a, c.rgb, is_premul);
        color = vec4<f32>(rgb_pm * img_params.opacity, c.a * img_params.opacity);
    }

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

    if img_params.source_linear > 0.5 && !web_source {
        return encode_output(color);
    }
    return color;
}
"#;
