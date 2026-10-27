// Full-domain guide, regional coarse correction and confidence-weighted merge.
@group(0) @binding(0) var<storage, read> params: array<vec4<u32>>;
@group(0) @binding(1) var<storage, read> fine: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> coarse: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> guide: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> output: array<vec4<f32>>;
fn fine_pixel(index: u32) -> vec4<f32> { return fine[index]; }

fn rounded4(value: vec4<f32>) -> vec4<f32> {
    return bitcast<vec4<f32>>(bitcast<vec4<u32>>(value) ^ vec4(params[4].x));
}
fn rounded1(value: f32) -> f32 { return rounded4(vec4(value)).x; }
fn divided(a: f32, b: f32) -> f32 {
    let q = rounded1(a / b);
    return rounded1(q + fma(-q, b, a) / b);
}
fn sum_error(a: vec4<f32>, b: vec4<f32>, sum: vec4<f32>) -> vec4<f32> {
    let virtual_b = rounded4(sum - a);
    return rounded4(a - rounded4(sum - virtual_b)) + rounded4(b - virtual_b);
}
fn luma(rgb: vec3<f32>) -> f32 {
    let terms = rounded4(vec4(rgb, 0.) * vec4(0.2627, 0.6780, 0.0593, 0.));
    return rounded4(vec4(terms.x + terms.y)).x + terms.z;
}
fn downsample_at(x: u32, y: u32, size: vec2<u32>) -> vec4<f32> {
    // Explicit loads avoid FXC's forced unrolling of a dynamically indexed
    // local array around the fused point shader. Preserve order and rounding.
    let x0 = min(x * 2u, size.x - 1u);
    let x1 = min(x * 2u + 1u, size.x - 1u);
    let y0 = min(y * 2u, size.y - 1u);
    let y1 = min(y * 2u + 1u, size.y - 1u);
    let p0 = fine_pixel(y0 * size.x + x0);
    if p0.w <= 0. { return vec4<f32>(0.); }
    let p1 = fine_pixel(y0 * size.x + x1);
    if p1.w <= 0. { return vec4<f32>(0.); }
    let p2 = fine_pixel(y1 * size.x + x0);
    if p2.w <= 0. { return vec4<f32>(0.); }
    let p3 = fine_pixel(y1 * size.x + x1);
    if p3.w <= 0. { return vec4<f32>(0.); }
    let a = rounded4(p0 + p1);
    let b = rounded4(p2 + p3);
    let total = rounded4(a + b);
    let error = rounded4(rounded4(sum_error(p0, p1, a)
        + sum_error(p2, p3, b)) + sum_error(a, b, total));
    return rounded4(total + error) * 0.25;
}

@compute @workgroup_size(256)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params[3].zw;
    if id.x >= size.x * size.y { return; }
    output[id.x] = downsample_at(id.x % size.x, id.x / size.x, params[0].xy);
}

@compute @workgroup_size(256)
fn chroma_delta(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params[3].xy;
    if id.x >= size.x * size.y { return; }
    let p = downsample_at(id.x % size.x, id.x / size.x, params[2].xy);
    let q = coarse[id.x];
    var delta = vec4<f32>(0.);
    if p.w > 0. && q.w > 0. {
        let a = p.xyz / p.w;
        let b = q.xyz / q.w;
        let dy = luma(b) - luma(a);
        let difference = rounded4(vec4(b - a, 0.));
        delta = vec4<f32>(difference.x - dy, 0., difference.z - dy, 1.);
    }
    output[id.x] = delta;
}

@compute @workgroup_size(256)
fn merge_chroma(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params[1].xy;
    if id.x >= size.x * size.y { return; }
    let xy = params[0].zw + vec2<u32>(id.x % size.x, id.x / size.x);
    let fine_xy = xy - params[1].zw;
    var p = fine_pixel(fine_xy.y * params[2].x + fine_xy.x);
    if p.w <= 0. { output[id.x] = p; return; }
    let position = vec2<f32>(xy) * 0.5 - vec2<f32>(0.25);
    let integer = vec2<i32>(floor(position));
    let fraction = position - floor(position);
    let rgb = p.xyz / p.w;
    let y0 = luma(rgb);
    var sum = vec3<f32>(0.);
    for (var dy = 0; dy < 2; dy += 1) {
        let wy = select(1. - fraction.y, fraction.y, dy == 1);
        for (var dx = 0; dx < 2; dx += 1) {
            let wx = select(1. - fraction.x, fraction.x, dx == 1);
            let at = vec2<u32>(clamp(integer + vec2<i32>(dx, dy), vec2<i32>(0), vec2<i32>(params[3].zw) - vec2<i32>(1)));
            let delta_xy = at - params[2].zw;
            let d = coarse[delta_xy.y * params[3].x + delta_xy.x];
            let g = guide[at.y * params[3].z + at.x];
            if d.w <= 0. || g.w <= 0. { continue; }
            let straight = g.xyz / g.w;
            let gy = luma(straight);
            let range_y = rounded1(0.02 + rounded1(0.2 * abs(y0)));
            let range_c = rounded1(0.015 + rounded1(0.08 * sqrt(abs(y0) + 0.003)));
            let dy0 = divided(rounded1(y0 - gy), range_y);
            let dr = divided(rounded1(rounded1(rounded1(rgb.x - y0) - straight.x) + gy), range_c);
            let db = divided(rounded1(rounded1(rounded1(rgb.z - y0) - straight.z) + gy), range_c);
            let distance = rounded1(rounded1(rounded1(dy0 * dy0) + rounded1(dr * dr)) + rounded1(db * db));
            let denominator = rounded1(1. + distance);
            let weight = divided(rounded1(wx * wy), rounded1(denominator * denominator));
            sum.x = rounded1(sum.x + rounded1(weight * d.x));
            sum.y = rounded1(sum.y + rounded1(weight * d.z));
            sum.z = rounded1(sum.z + weight);
        }
    }
    if sum.z > 1e-12 {
        let confidence = min(sum.z * 2., 1.);
        let dr = rounded1(divided(sum.x, sum.z) * confidence);
        let db = rounded1(divided(sum.y, sum.z) * confidence);
        let terms = rounded4(vec4(0.2627 * dr, 0.0593 * db, 0., 0.));
        let green = divided(rounded1(terms.x + terms.y), 0.678);
        let delta = rounded4(rounded4(vec4(dr, -green, db, 0.)) * p.w);
        p += delta;
    }
    output[id.x] = p;
}
