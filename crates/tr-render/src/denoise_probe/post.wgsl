@group(0) @binding(3) var<storage, read> masks: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> segments: array<vec4<f32>>;
fn smooth_mask(v: f32) -> f32 {
    let t = clamp(v, 0., 1.);
    return t * t * (3. - 2. * t);
}
fn mask_weight(index: u32, xy: vec2<f32>, guide: vec3<f32>) -> f32 {
    let base = 3u + index * 5u;
    let geometry = masks[base];
    let shape = masks[base + 1u];
    let effect = masks[base + 2u];
    let aspect = masks[1].x;
    var weight = 0.;
    switch u32(shape.x) {
        case 0u: {
            let d = sqrt(square(xy.x - geometry.x) + square(aspect * (xy.y - geometry.y)));
            weight = 1. - smooth_mask((d / geometry.z - (1. - geometry.w)) / geometry.w);
        }
        case 1u: {
            let d = (xy.x - geometry.x) * shape.w + (xy.y - geometry.y) * aspect * shape.z;
            weight = smooth_mask(0.5 + d / (2. * geometry.z));
        }
        case 2u: {
            var best = square(geometry.z);
            let inner = square(geometry.z * (1. - geometry.w));
            let begin = u32(masks[base + 3u].w);
            let count = u32(masks[base + 4u].x);
            for (var i = 0u; i < count; i += 1u) {
                let segment = segments[begin + i];
                let v = segment.zw - segment.xy;
                let len = v.x * v.x + v.y * v.y;
                var t = 0.;
                if len > 1e-12 {
                    t = clamp(((xy.x - segment.x) * v.x + (xy.y * aspect - segment.y) * v.y) / len, 0., 1.);
                }
                best = min(best, square(xy.x - segment.x - v.x * t) + square(xy.y * aspect - segment.y - v.y * t));
                if best <= inner { break; }
            }
            weight = 1. - smooth_mask((sqrt(best) / geometry.z - (1. - geometry.w)) / geometry.w);
        }
        default: {
            var value = luma(guide);
            if shape.x == 4. {
                let high = max(max(guide.r, guide.g), guide.b);
                let chroma = high - min(min(guide.r, guide.g), guide.b);
                value = 0.;
                if chroma > 1e-8 { value = hue(guide, high, chroma) / 360.; }
            }
            let edge = (effect.y - effect.x) * geometry.w * 0.5;
            weight = smooth_mask((value - effect.x) / edge) * smooth_mask((effect.y - value) / edge);
        }
    }
    if shape.y != 0. { return 1. - weight; }
    return weight;
}
@compute @workgroup_size(256)
fn post(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(masks[2].xy);
    if id.x >= size.x * size.y { return; }
    let pixel = source[id.x];
    if pixel.a <= 0. { output[id.x] = pixel; return; }
    let guide = pixel.rgb / pixel.a;
    var rgb = guide;
    if params.data[4].x != 0. { rgb = advanced_color(guide); }
    let xy = (masks[0].xy + vec2<f32>(f32(id.x % size.x), f32(id.x / size.x)) + vec2(0.5)) / masks[0].zw;
    for (var i = 0u; i < u32(masks[1].y); i += 1u) {
        let base = 3u + i * 5u;
        let effect = masks[base + 2u];
        var edited = rgb * effect.z * masks[base + 3u].xyz;
        let y = luma(edited);
        edited = vec3(y) + (edited - vec3(y)) * effect.w;
        rgb += mask_weight(i, xy, guide) * (edited - rgb);
    }
    output[id.x] = vec4(rgb * pixel.a, pixel.a);
}
