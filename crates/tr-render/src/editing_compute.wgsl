// Same extended-linear RGB order as tr_core::editing, evaluated before resampling.
struct Parameters { data: array<vec4<f32>, 52> }
@group(0) @binding(0) var<uniform> params: Parameters;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;
fn luma(rgb: vec3<f32>) -> f32 { return rgb.r * 0.2627 + rgb.g * 0.6780 + rgb.b * 0.0593; }
fn square(x: f32) -> f32 { return x * x; }
fn rem(x: f32, n: f32) -> f32 { return x - floor(x / n) * n; }
fn hue(rgb: vec3<f32>, high: f32, chroma: f32) -> f32 {
    if high == rgb.r { return rem((rgb.g - rgb.b) / chroma, 6.) * 60.; }
    if high == rgb.g { return ((rgb.b - rgb.r) / chroma + 2.) * 60.; }
    return ((rgb.r - rgb.g) / chroma + 4.) * 60.;
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
fn curve(x: f32) -> f32 {
    if x <= 0. || x >= 1. { return x; }
    let count = u32(params.data[3].x);
    var right = 1u;
    while right + 1u < count && params.data[8u + right].x < x { right += 1u; }
    let a = params.data[7u + right];
    let b = params.data[8u + right];
    return a.y + (x - a.x) * (b.y - a.y) / (b.x - a.x);
}
fn tone(y: f32, amount: f32, weight: f32) -> f32 { return y + amount * y * (1. - y) * weight; }
fn advanced_color(input: vec3<f32>) -> vec3<f32> {
    var rgb = input;
    let y = luma(rgb);
    let high = max(max(rgb.r, rgb.g), rgb.b);
    let chroma = high - min(min(rgb.r, rgb.g), rgb.b);
    if chroma > 1e-8 {
        let h = hue(rgb, high, chroma);
        let centers = array<f32, 9>(0., 30., 60., 120., 180., 240., 270., 300., 360.);
        var index = 0u;
        while index < 7u && h >= centers[index + 1u] { index += 1u; }
        let t = (h - centers[index]) / (centers[index + 1u] - centers[index]);
        let a = params.data[40u + index];
        let b = params.data[40u + ((index + 1u) % 8u)];
        let band = a + (b - a) * t;
        let unit = hue_rgb(h + band.x * 0.3);
        let c = chroma * (1. + band.y / 100.);
        rgb = (vec3(y) + (unit - vec3(luma(unit))) * c) * exp2(band.z / 100.);
    }
    if params.data[4].y != 0. { rgb = vec3(luma(rgb)); }
    let sy = luma(rgb);
    let t = clamp(sy, 0., 1.);
    let weights = vec3(square(1. - t), 2. * t * (1. - t), t * t);
    for (var i = 0u; i < 3u; i += 1u) {
        let grade = params.data[48u + i];
        if grade.y == 0. { continue; }
        let unit = hue_rgb(grade.x);
        rgb += (unit - vec3(luma(unit))) * grade.y / 100. * weights[i] * abs(sy);
    }
    for (var c = 0u; c < 3u; c += 1u) {
        if rgb[c] >= 0. && rgb[c] <= 1. {
            rgb[c] += params.data[51][c] / 400. * 4. * rgb[c] * (1. - rgb[c]);
        }
    }
    return rgb;
}
@compute @workgroup_size(256)
fn edit(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if i >= arrayLength(&source) { return; }
    let pixel = vec4(source[i].rgb * params.data[5].xyz, source[i].a);
    let alpha = pixel.a;
    if alpha == 0. { output[i] = pixel; return; }
    var rgb = pixel.rgb;
    let p = params.data[0];
    if params.data[3].w != 0. {
        rgb *= p.x;
    } else {
        rgb = rgb / alpha * (vec3(p.x) * p.yzw);
        let old_y = luma(rgb);
        if old_y > 1e-8 && params.data[3].z != 0. {
            var y = old_y;
            if y <= 1. {
                y = tone(y, params.data[1].x, 1.);
                y = tone(y, params.data[1].y, square(1. - y));
                y = tone(y, params.data[1].z, square(y));
                y = tone(y, params.data[1].w, square(square(1. - y)));
                y = tone(y, params.data[2].x, square(square(y)));
            }
            y = 0.18 * pow(y / 0.18, params.data[2].y);
            if params.data[3].x != 0. { y = curve(y); }
            rgb *= y / old_y;
        }
        if params.data[2].z != 1. {
            let y = luma(rgb);
            rgb = vec3(y) + (rgb - vec3(y)) * params.data[2].z;
        }
        if params.data[2].w != 0. && all(rgb >= vec3(0.)) {
            let high = max(max(rgb.r, rgb.g), rgb.b);
            let chroma = high - min(min(rgb.r, rgb.g), rgb.b);
            if high > 1e-8 && chroma > 0. {
                var weight = square(1. - chroma / high);
                if params.data[3].y != 0. {
                    weight *= 1. - 0.75 * max(1. - abs(hue(rgb, high, chroma) - 35.) / 35., 0.);
                }
                let y = luma(rgb);
                rgb = vec3(y) + (rgb - vec3(y)) * (1. + params.data[2].w * weight);
            }
        }
        rgb *= alpha;
    }
    if params.data[4].x != 0. { rgb = advanced_color(rgb / alpha) * alpha; }
    output[i] = vec4(rgb, alpha);
}
