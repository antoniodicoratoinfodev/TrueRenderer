@group(0) @binding(0) var<storage, read> params: array<vec4<u32>>;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> low: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> weights: array<f32>;
@group(0) @binding(4) var<storage, read_write> output: array<vec4<f32>>;
// The unused parameter word is zero. Keep compensated additions observable to
// optimizing HLSL compilers, which may otherwise cancel the residual algebra.
fn rounded4(value: vec4<f32>) -> vec4<f32> {
    return bitcast<vec4<f32>>(bitcast<vec4<u32>>(value) ^ vec4(params[1].w));
}
fn luma(rgb: vec3<f32>) -> f32 {
    let terms = rounded4(vec4(rgb, 0.) * vec4(0.2627, 0.6780, 0.0593, 0.));
    return rounded4(vec4(terms.x + terms.y)).x + terms.z;
}
fn filtered(index: u32, vertical: bool) -> vec4<f32> {
    let size = vec2<i32>(params[0].xy);
    let xy = vec2<i32>(i32(index % params[0].x), i32(index / params[0].x));
    let radius = i32(params[0].w);
    var sum = vec4<f32>(0.);
    var residual = vec4<f32>(0.);
    for (var k = -radius; k <= radius; k += 1) {
        var sample_xy = xy;
        if vertical { sample_xy.y = clamp(xy.y + k, 0, size.y - 1); }
        else { sample_xy.x = clamp(xy.x + k, 0, size.x - 1); }
        var value = source[u32(sample_xy.y * size.x + sample_xy.x)];
        if params[0].z == 1u { value = rounded4(value * weights[u32(k + radius)]); }
        let adjusted = rounded4(value - residual);
        let next = rounded4(sum + adjusted);
        residual = rounded4(rounded4(next - sum) - adjusted);
        sum = next;
    }
    if params[0].z == 0u { sum /= f32(2 * radius + 1); }
    return sum;
}
@compute @workgroup_size(256)
fn horizontal(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params[0].x * params[0].y { return; }
    output[id.x] = filtered(id.x, false);
}
@compute @workgroup_size(256)
fn vertical(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params[0].x * params[0].y { return; }
    output[id.x] = filtered(id.x, true);
}
@compute @workgroup_size(256)
fn spatial(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params[0].x * params[0].y { return; }
    var p = source[id.x];
    let b = low[id.x];
    if p.a > 0. && b.a > 0. {
        let rgb = p.rgb / p.a;
        let y = luma(rgb);
        let difference = y - luma(b.rgb / b.a);
        let detail = sign(difference) * max(abs(difference) - bitcast<f32>(params[1].y), 0.);
        var protection = 1.;
        if params[1].z != 0u {
            let t = clamp(y, 0., 1.);
            protection = sqrt(4. * t * (1. - t));
        }
        let limit = 0.05 + abs(y) * 0.25;
        let delta = clamp(bitcast<f32>(params[1].x) * detail * protection, -limit, limit);
        p = vec4((rgb + vec3(delta)) * p.a, p.a);
    }
    output[id.x] = p;
}
@compute @workgroup_size(256)
fn defringe(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params[0].x * params[0].y { return; }
    var p = source[id.x];
    let b = low[id.x];
    if p.a > 0. && b.a > 0. {
        let rgb = p.rgb / p.a;
        let y = luma(rgb);
        let by = luma(b.rgb / b.a);
        let edge = clamp(abs(y - by) / (0.03 + abs(y)), 0., 1.);
        let purple = max(min(rgb.r, rgb.b) - rgb.g, 0.);
        let green = max(rgb.g - max(rgb.r, rgb.b), 0.);
        let f = bitcast<f32>(params[1].x) * edge * p.a;
        p = vec4(p.rgb - f * vec3(purple, green, purple), p.a);
    }
    output[id.x] = p;
}
@compute @workgroup_size(256)
fn crop(@builtin(global_invocation_id) id: vec3<u32>) {
    let rect = params[2];
    if id.x >= rect.z * rect.w { return; }
    let xy = rect.xy + vec2(id.x % rect.z, id.x / rect.z);
    output[id.x] = source[xy.y * params[0].x + xy.x];
}
