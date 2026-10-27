@group(0) @binding(5) var<storage, read> graph: array<vec4<f32>>;
fn color_range(guide: vec3<f32>, reference: vec4<f32>, widths: vec4<f32>) -> f32 {
    let high = max(max(guide.r, guide.g), guide.b);
    var chroma = high - min(min(guide.r, guide.g), guide.b);
    var h = 0.;
    if chroma > 1e-8 { h = hue(guide, high, chroma); }
    else { chroma = 0.; }
    let inner = 1. - widths.w;
    var hue_weight = 1.;
    if reference.y > 1e-6 {
        let delta = rem(h - reference.x + 180., 360.) - 180.;
        hue_weight = (1. - smooth_mask((abs(delta) / widths.x - inner) / widths.w))
            * smooth_mask(chroma / reference.w);
    }
    return hue_weight * (1. - smooth_mask((abs(chroma - reference.y) / widths.y - inner) / widths.w))
        * (1. - smooth_mask((abs(luma(guide) - reference.z) / widths.z - inner) / widths.w));
}
@compute @workgroup_size(256)
fn coverage(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(masks[2].xy);
    if id.x >= size.x * size.y { return; }
    let pixel = source[id.x];
    if pixel.a <= 0. { output[id.x] = 0.; return; }
    var guide = pixel.rgb;
    if masks[2].z == 0. { guide /= pixel.a; }
    let xy = (masks[0].xy + vec2(f32(id.x % size.x), f32(id.x / size.x)) + vec2(0.5)) / masks[0].zw;
    if graph[0].y == 0. { output[id.x] = 1.; return; }
    var values: array<f32, 64>;
    for (var i = 0u; i < u32(graph[0].y); i += 1u) {
        let at = 1u + i * 4u;
        let node = graph[at];
        switch u32(node.x) {
            case 0u: { values[i] = mask_weight(u32(node.y), xy, guide); }
            case 1u: { values[i] = color_range(guide, graph[at + 1u], graph[at + 2u]); }
            case 2u: { values[i] = node.y; }
            case 3u: { values[i] = 1. - (1. - values[u32(node.y)]) * (1. - values[u32(node.z)]); }
            case 4u: { values[i] = values[u32(node.y)] * (1. - values[u32(node.z)]); }
            case 5u: { values[i] = values[u32(node.y)] * values[u32(node.z)]; }
            default: { values[i] = 1. - values[u32(node.y)]; }
        }
    }
    output[id.x] = values[u32(graph[0].x)];
}
