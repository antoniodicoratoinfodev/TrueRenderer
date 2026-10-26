// CPU computes the canonical inverse map in f64. GPU uses the same fp32
// bilinear arithmetic on the layer output, before the display resampler.
@group(0) @binding(0) var<storage, read> parameters: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> coordinates: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> output: array<vec4<f32>>;
fn pixel(at: vec2<u32>) -> vec4<f32> {
    let xy = min(at, vec2<u32>(parameters[1].xy) - vec2(1u)) - vec2<u32>(parameters[0].yz);
    return source[xy.y * u32(parameters[0].x) + xy.x];
}
fn sample_at(s: vec4<f32>) -> vec4<f32> {
    if s.x < 0. { return vec4(0.); }
    let at = vec2<u32>(s.xy);
    let a = pixel(at);
    let b = pixel(at + vec2(1u, 0u));
    let c = pixel(at + vec2(0u, 1u));
    let d = pixel(at + vec2(1u, 1u));
    return (a + (b - a) * s.z) * (1. - s.w) + (c + (d - c) * s.z) * s.w;
}
@compute @workgroup_size(256)
fn geometry(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= arrayLength(&output) { return; }
    var pixel_out = sample_at(coordinates[i * 3u + 1u]);
    if parameters[1].z != 0. && pixel_out.a > 0. {
        let colors = array(sample_at(coordinates[i * 3u]), pixel_out, sample_at(coordinates[i * 3u + 2u]));
        let alpha = min(min(min(1., colors[0].a), colors[1].a), colors[2].a);
        for (var c = 0u; c < 3u; c += 1u) {
            pixel_out[c] = 0.;
            if colors[c].a > 0. { pixel_out[c] = colors[c][c] / colors[c].a * alpha; }
        }
        pixel_out.a = alpha;
    }
    output[i] = pixel_out;
}
