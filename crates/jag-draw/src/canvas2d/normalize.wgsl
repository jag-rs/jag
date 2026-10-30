@group(0) @binding(0) var bitmap: texture_2d<f32>;
@vertex fn vertex(@builtin(vertex_index) v: u32) -> @builtin(position) vec4f {
    let p = array<vec2f,3>(vec2f(-1,-1), vec2f(3,-1), vec2f(-1,3));
    return vec4f(p[v],0,1);
}
@fragment fn fragment(@builtin(position) p: vec4f) -> @location(0) vec4f {
    let c = textureLoad(bitmap,vec2i(p.xy),0);
    if (c.a == 0) { return vec4f(0); }
    let rgb = clamp(c.rgb / c.a,vec3f(0),vec3f(1));
    let linear = select(pow((rgb + 0.055) / 1.055,vec3f(2.4)),rgb / 12.92,rgb <= vec3f(0.04045));
    return vec4f(linear * c.a,c.a);
}
