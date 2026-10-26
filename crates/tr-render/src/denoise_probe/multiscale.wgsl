// Full-domain guide, regional coarse correction and confidence-weighted merge.
@group(0) @binding(0) var<storage, read> params: array<vec4<u32>>;
@group(0) @binding(1) var<storage, read> fine: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> coarse: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> guide: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> output: array<vec4<f32>>;
fn fine_pixel(index: u32) -> vec4<f32> { return fine[index]; }

fn luma(rgb: vec3<f32>) -> f32 { return 0.2627 * rgb.x + 0.6780 * rgb.y + 0.0593 * rgb.z; }
fn downsample_at(x: u32, y: u32, size: vec2<u32>) -> vec4<f32> {
    var samples: array<vec4<f32>, 4>;
    for (var dy = 0u; dy < 2u; dy += 1u) {
        for (var dx = 0u; dx < 2u; dx += 1u) {
            let xx = min(x * 2u + dx, size.x - 1u);
            let yy = min(y * 2u + dy, size.y - 1u);
            let p = fine_pixel(yy * size.x + xx);
            if p.w <= 0. { return vec4<f32>(0.); }
            samples[dy * 2u + dx] = p;
        }
    }
    return ((samples[0] + samples[1]) + (samples[2] + samples[3])) * 0.25;
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
        delta = vec4<f32>(b.x - a.x - dy, 0., b.z - a.z - dy, 1.);
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
            let range_y = 0.02 + 0.2 * abs(y0);
            let range_c = 0.015 + 0.08 * sqrt(abs(y0) + 0.003);
            let dy0 = (y0 - gy) / range_y;
            let dr = (rgb.x - y0 - straight.x + gy) / range_c;
            let db = (rgb.z - y0 - straight.z + gy) / range_c;
            let distance = dy0 * dy0 + dr * dr + db * db;
            let weight = wx * wy / ((1. + distance) * (1. + distance));
            sum.x += weight * d.x;
            sum.y += weight * d.z;
            sum.z += weight;
        }
    }
    if sum.z > 1e-12 {
        let confidence = min(sum.z * 2., 1.);
        let dr = sum.x / sum.z * confidence;
        let db = sum.y / sum.z * confidence;
        p.x += dr * p.w;
        p.y -= (0.2627 * dr + 0.0593 * db) / 0.678 * p.w;
        p.z += db * p.w;
    }
    output[id.x] = p;
}
