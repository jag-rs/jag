// Exact rectangle/pixel intersection coverage, including affine transforms.
struct Op { rect: vec4f, inverse_x: vec4f, inverse_y: vec4f, color: vec4f, options: vec4f }
@group(0) @binding(0) var<storage, read> ops: array<Op>;
struct Out { @builtin(position) position: vec4f, @location(0) @interpolate(flat) index: u32 }
@vertex fn vertex(@builtin(vertex_index) v: u32, @builtin(instance_index) i: u32) -> Out {
    let p = array<vec2f,3>(vec2f(-1,-1), vec2f(3,-1), vec2f(-1,3));
    var out: Out; out.position = vec4f(p[v],0,1); out.index = i; return out;
}
struct Polygon { points: array<vec2f,12>, count: u32 }
fn clip(poly: Polygon, axis: u32, boundary: f32, greater: bool) -> Polygon {
    var out: Polygon;
    if (poly.count == 0) { return out; }
    var prev = poly.points[poly.count - 1];
    var prev_in = select(prev[axis] <= boundary,prev[axis] >= boundary,greater);
    for (var i = 0u; i < poly.count; i++) {
        let curr = poly.points[i];
        let curr_in = select(curr[axis] <= boundary,curr[axis] >= boundary,greater);
        if (curr_in != prev_in) {
            let t = (boundary - prev[axis]) / (curr[axis] - prev[axis]);
            out.points[out.count] = mix(prev,curr,t); out.count++;
        }
        if (curr_in) {out.points[out.count] = curr; out.count++;}
        prev = curr; prev_in = curr_in;
    }
    return out;
}
fn coverage(op: Op, pixel: vec2f, rect: vec4f) -> f32 {
    if (any(rect.zw <= vec2f(0))) {return 0;}
    let det = op.inverse_x.x * op.inverse_y.y - op.inverse_x.y * op.inverse_y.x;
    let aligned = (abs(op.inverse_x.y) < 0.000001 && abs(op.inverse_y.x) < 0.000001) || (abs(op.inverse_x.x) < 0.000001 && abs(op.inverse_y.y) < 0.000001);
    if (aligned) {
        let q0 = rect.xy - vec2f(op.inverse_x.z,op.inverse_y.z);
        let q1 = rect.xy + rect.zw - vec2f(op.inverse_x.z,op.inverse_y.z);
        let a = vec2f(op.inverse_y.y*q0.x-op.inverse_x.y*q0.y,-op.inverse_y.x*q0.x+op.inverse_x.x*q0.y) / det;
        let b = vec2f(op.inverse_y.y*q1.x-op.inverse_x.y*q1.y,-op.inverse_y.x*q1.x+op.inverse_x.x*q1.y) / det;
        let amount = clamp(min(pixel+vec2f(1),max(a,b))-max(pixel,min(a,b)),vec2f(0),vec2f(1));
        // Browser rectangle AA uses the minimum edge coverage at corners.
        return min(amount.x,amount.y);
    }
    var poly: Polygon;poly.count = 4;
    let corners = array<vec2f,4>(vec2f(0,0),vec2f(1,0),vec2f(1,1),vec2f(0,1));
    for (var i = 0u;i < 4;i++) {
        let p = vec3f(pixel + corners[i],1);
        poly.points[i] = vec2f(dot(op.inverse_x.xyz,p),dot(op.inverse_y.xyz,p));
    }
    poly = clip(poly,0,rect.x,true);poly = clip(poly,0,rect.x + rect.z,false);
    poly = clip(poly,1,rect.y,true);poly = clip(poly,1,rect.y + rect.w,false);
    if (poly.count < 3) {return 0;}
    // Translate before the area calculation to avoid cancellation far from origin.
    let origin = poly.points[0]; var area = 0.0;
    for(var i = 1u;i + 1 < poly.count;i++) {
        let a = poly.points[i] - origin;let b = poly.points[i+1] - origin;
        area += a.x * b.y - a.y * b.x;
    }
    let inverse_det = op.inverse_x.x * op.inverse_y.y - op.inverse_x.y * op.inverse_y.x;
    return clamp(abs(area / inverse_det) * 0.5,0,1);
}
@fragment fn fragment(input: Out) -> @location(0) vec4f {
    let op = ops[input.index];let pixel = floor(input.position.xy);
    if (op.options.y > 0) {
        let p=vec3f(input.position.xy,1);
        let local=vec2f(dot(op.inverse_x.xyz,p),dot(op.inverse_y.xyz,p));
        if (all(local >= op.rect.xy) && all(local < op.rect.xy+op.rect.zw)){return vec4f(0,0,0,1);}
        discard;
    }
    var amount = coverage(op,pixel,op.rect);
    if (op.options.x > 0) {
        let half_width = op.options.x * 0.5;
        var outer = vec4f(op.rect.xy - half_width,op.rect.zw + op.options.x);
        if(op.rect.z == 0){outer=vec4f(op.rect.x-half_width,op.rect.y,op.options.x,op.rect.w);}
        if(op.rect.w == 0){outer=vec4f(op.rect.x,op.rect.y-half_width,op.rect.z,op.options.x);}
        let inner = vec4f(op.rect.xy + half_width,op.rect.zw - op.options.x);
        amount = clamp(coverage(op,pixel,outer)-coverage(op,pixel,inner),0,1);
    }
    if (amount == 0) { discard; }
    if (op.options.y > 0) { return vec4f(0,0,0,amount); }
    let alpha = op.color.a * amount;
    return vec4f(op.color.rgb * alpha,alpha);
}
