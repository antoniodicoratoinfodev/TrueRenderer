// Extended-linear photographic layer operators. The input of each layer is its
// guide; masks affect the effect only, never coverage. No display quantization.
@group(0) @binding(0) var<storage, read> parameters: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> mask: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<vec4<f32>>;

fn luma(rgb: vec3<f32>) -> f32 { return 0.2627 * rgb.r + 0.6780 * rgb.g + 0.0593 * rgb.b; }
fn rem(x: f32, n: f32) -> f32 { return x - floor(x / n) * n; }
fn smooth01(v: f32) -> f32 { let t = clamp(v, 0., 1.); return t * t * (3. - 2. * t); }
fn hue_delta(a: f32, b: f32) -> f32 { return rem(a - b + 180., 360.) - 180.; }
fn hue_chroma(rgb: vec3<f32>) -> vec2<f32> {
    let high = max(max(rgb.r, rgb.g), rgb.b);
    let c = high - min(min(rgb.r, rgb.g), rgb.b);
    if c <= 1e-8 { return vec2(0., 0.); }
    if high == rgb.r { return vec2(rem((rgb.g - rgb.b) / c, 6.) * 60., c); }
    if high == rgb.g { return vec2(((rgb.b - rgb.r) / c + 2.) * 60., c); }
    return vec2(((rgb.r - rgb.g) / c + 4.) * 60., c);
}
fn hue_rgb(degrees: f32) -> vec3<f32> {
    let h = rem(degrees, 360.) / 60.;
    let x = 1. - abs(rem(h, 2.) - 1.);
    switch u32(h) {
        case 0u: { return vec3(1., x, 0.); }
        case 1u: { return vec3(x, 1., 0.); }
        case 2u: { return vec3(0., 1., x); }
        case 3u: { return vec3(0., x, 1.); }
        case 4u: { return vec3(x, 0., 1.); }
        default: { return vec3(1., 0., x); }
    }
}
fn falloff(distance: f32, width: f32, inner: f32, softness: f32) -> f32 {
    return 1. - smooth01((abs(distance) / width - inner) / softness);
}
fn sample_color(rgb: vec3<f32>, guide: vec3<f32>, at: u32) -> vec3<f32> {
    let reference = parameters[at + 1u];
    let widths = parameters[at + 2u];
    let correction = parameters[at + 3u];
    let uniformity = parameters[at + 4u];
    let guided = hue_chroma(guide);
    var hue_weight = 1.;
    if reference.y > 1e-6 {
        hue_weight = falloff(hue_delta(guided.x, reference.x), widths.x, uniformity.w, widths.w)
            * smooth01(guided.y / reference.w);
    }
    let weight = hue_weight * falloff(guided.y - reference.y, widths.y, uniformity.w, widths.w)
        * falloff(luma(guide) - reference.z, widths.z, uniformity.w, widths.w);
    if weight == 0. { return rgb; }
    var hc = hue_chroma(rgb);
    var y = luma(rgb);
    if hc.y > 1e-8 {
        if reference.y > 1e-6 { hc.x += hue_delta(reference.x, hc.x) * uniformity.x / 100.; }
        hc.x += correction.x;
        hc.y += (reference.y - hc.y) * uniformity.y / 100.;
        hc.y *= 1. + correction.y / 100.;
    }
    y += (reference.z - y) * uniformity.z / 100.;
    let unit = hue_rgb(hc.x);
    return rgb + weight * ((vec3(y) + (unit - vec3(luma(unit))) * hc.y) * correction.w - rgb);
}
fn tonal_level(value: f32, at: u32) -> f32 {
    let a = parameters[at];
    let b = parameters[at + 1u];
    if b.y != 0. { return value; }
    let x = (value - a.x) / a.y;
    let direction = select(1., -1., x < 0.);
    return a.z + direction * pow(abs(x), a.w) * b.x;
}
@compute @workgroup_size(256)
fn layers(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= arrayLength(&source) { return; }
    let p = source[i];
    if p.a <= 0. || parameters[0].y == 0. { output[i] = p; return; }
    var rgb = p.rgb;
    if p.a != 1. { rgb /= p.a; }
    var at = 1u;
    for (var layer = 0u; layer < u32(parameters[0].x); layer += 1u) {
        let header = parameters[at];
        at += 1u;
        let guide = rgb;
        var edited = rgb;
        for (var op = 0u; op < u32(header.x); op += 1u) {
            let kind = u32(parameters[at].x);
            let length = u32(parameters[at].y);
            if header.y != 0. { switch kind {
                case 1u: { edited = sample_color(edited, guide, at); }
                case 2u: {
                    let gains = parameters[at + 1u];
                    let v = edited * gains.w * gains.xyz;
                    let y = luma(v);
                    edited = vec3(y) + (v - vec3(y)) * parameters[at + 2u].x;
                }
                case 3u: {
                    // Dynamic vector stores force FXC to unroll the enclosing
                    // variable-length operator/layer loops. Channels are fixed.
                    edited = vec3(
                        tonal_level(tonal_level(edited.x, at + 1u), at + 3u),
                        tonal_level(tonal_level(edited.y, at + 1u), at + 5u),
                        tonal_level(tonal_level(edited.z, at + 1u), at + 7u));
                }
                default: {}
            } }
            at += length;
        }
        var weight = header.y;
        if header.z != 0. { weight *= mask[i]; }
        if weight != 0. { rgb = guide + weight * (edited - guide); }
    }
    output[i] = vec4(rgb * p.a, p.a);
}
