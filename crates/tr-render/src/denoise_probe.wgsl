// Experimental spectral block preparation only. Not an editing renderer.
@group(0) @binding(0) var<storage, read> params: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> model: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> filtered: array<vec4<f32>>;
var<workgroup> first: array<vec4<f32>, 256>;
var<workgroup> second: array<vec4<f32>, 256>;
var<workgroup> low: array<vec3<f32>, 256>;
var<workgroup> centre: vec4<f32>;
fn source_pixel(index: u32) -> vec4<f32> { return source[index]; }

// The packed origin's unused z word is zero. Crossing the integer operation
// makes the intermediate f32 rounding observable, including on FMA hardware.
fn rounded3(value: vec3<f32>) -> vec3<f32> {
    let n = u32(params[0].z);
    let zero = bitcast<u32>(params[1u + n * n / 4u].z);
    return bitcast<vec3<f32>>(bitcast<vec3<u32>>(value) ^ vec3(zero));
}
fn rounded1(value: f32) -> f32 { return rounded3(vec3(value)).x; }
fn product_error(a: f32, b: f32, product: f32) -> f32 {
    let ah = bitcast<f32>(bitcast<u32>(a) & 0xfffff000u);
    let bh = bitcast<f32>(bitcast<u32>(b) & 0xfffff000u);
    let al = rounded1(a - ah);
    let bl = rounded1(b - bh);
    let first = rounded1(rounded1(ah * bh) - product);
    let second = rounded1(first + rounded1(ah * bl));
    return rounded1(rounded1(second + rounded1(al * bh)) + rounded1(al * bl));
}
fn divided(a: f32, b: f32) -> f32 {
    let q = rounded1(a / b);
    let product = rounded1(q * b);
    let residual = rounded1(rounded1(a - product) - product_error(q, b, product));
    return rounded1(q + residual / b);
}
fn square_root(value: f32) -> f32 {
    let root = rounded1(sqrt(value));
    if root == 0. { return root; }
    let product = rounded1(root * root);
    let residual = rounded1(rounded1(value - product) - product_error(root, root, product));
    return rounded1(root + residual / (2. * root));
}

fn reflect(v: i32, size: i32) -> i32 {
    if size <= 1 { return 0; }
    let period = 2 * (size - 1);
    let r = ((v % period) + period) % period;
    return min(r, period - r);
}
fn basis(k: u32, j: u32, n: u32) -> f32 {
    let i = k * n + j;
    return params[1u + i / 4u][i % 4u];
}
fn prepare(local: vec3<u32>, group: vec3<u32>) -> vec3<f32> {
    let n = u32(params[0].z);
    let slot = local.y * 16u + local.x;
    let origin = vec2<i32>(params[1u + n * n / 4u + group.x].xy);
    let inside = local.x < n && local.y < n;
    var value = vec4<f32>(0.);
    if inside {
        let x = reflect(origin.x + i32(local.x), i32(params[0].x));
        let y = reflect(origin.y + i32(local.y), i32(params[0].y));
        let p = source_pixel(u32(y) * u32(params[0].x) + u32(x));
        if p.w > 0. {
            let rgb = p.xyz / p.w;
            let terms = rounded3(rgb * vec3(0.2627, 0.6780, 0.0593));
            let luma = rounded3(vec3(terms.x + terms.y)).x + terms.z;
            value = vec4<f32>(luma, rgb.x - luma, rgb.z - luma, 1.);
        }
    }
    first[slot] = value;
    second[slot] = value;
    low[slot] = vec3(0.);
    workgroupBarrier();
    for (var stride = 128u; stride > 0u; stride = stride / 2u) {
        if slot < stride {
            let a = first[slot];
            let b = first[slot + stride];
            let sum = rounded3(a.xyz + b.xyz);
            let virtual_b = rounded3(sum - a.xyz);
            let error = rounded3(a.xyz - rounded3(sum - virtual_b))
                + rounded3(b.xyz - virtual_b);
            low[slot] += low[slot + stride] + error;
            first[slot] = vec4(sum, a.w + b.w);
        }
        workgroupBarrier();
    }
    if slot == 0u { centre = vec4(first[0].xyz + low[0], first[0].w) / f32(n * n); }
    workgroupBarrier();
    first[slot] = vec4(second[slot].xyz - centre.xyz, 0.);
    workgroupBarrier();
    forward_row(local, n);
    forward_column(local, n);
    return first[slot].xyz;
}
@compute @workgroup_size(16, 16)
fn blocks(@builtin(local_invocation_id) local: vec3<u32>,
          @builtin(workgroup_id) group: vec3<u32>) {
    let column = prepare(local, group);
    let slot = local.y * 16u + local.x;
    let valid = centre.w == 1.;
    output[group.x * 257u + slot] = select(vec4<f32>(0.), vec4<f32>(column, 0.), valid);
    if slot == 0u {
        output[group.x * 257u + 256u] = select(vec4<f32>(0.),
            vec4<f32>(centre.x, square_root(abs(centre.x) + 0.003), 1., 0.), valid);
    }
}

fn removed(ratio: f32) -> f32 {
    let t = clamp(divided(rounded1(ratio - 0.75), 0.75), 0., 1.);
    return 1. - rounded1(rounded1(t * t) * (3. - 2. * t));
}
fn shrink(value: vec3<f32>, index: u32, n: u32, level: f32) -> vec3<f32> {
    let count = u32(model[0].x);
    if count == 0u || index == 0u { return vec3<f32>(0.); }
    var hi = 0u;
    while hi < count && model[1u + hi].x < level { hi += 1u; }
    hi = min(hi, count - 1u);
    let lo = u32(max(i32(hi) - 1, 0));
    let a = model[1u + lo].x;
    let b = model[1u + hi].x;
    let blend = clamp(divided(rounded1(level - a), max(b - a, 1e-8)), 0., 1.);
    let sa = model[1u + count + lo * n * n + index].xyz;
    let sb = model[1u + count + hi * n * n + index].xyz;
    let mixed = rounded3(rounded3(sa * (1. - blend)) + rounded3(sb * blend));
    let sigma = rounded3(rounded3(mixed * level) * (1. + model[0].w));
    var result = value;
    result.x *= select(removed(divided(abs(value.x), max(rounded1(sigma.x * model[0].y) * 3., 1e-12))), 0., model[0].y == 0.);
    let strength = model[0].z * 3.25;
    let power = rounded1(rounded1(value.y * value.y) + rounded1(value.z * value.z));
    let noise = rounded1(rounded1(sigma.y * sigma.y) + rounded1(sigma.z * sigma.z));
    let ratio = square_root(divided(power,
        max(rounded1(rounded1(noise * strength) * strength), 1e-24)));
    result.y *= select(removed(ratio), 0., model[0].z == 0.);
    result.z *= select(removed(ratio), 0., model[0].z == 0.);
    return result;
}

// One group per canonical patch. Store the removed signal, never normalized RGB.
@compute @workgroup_size(16, 16)
fn reconstruct(@builtin(local_invocation_id) local: vec3<u32>,
               @builtin(workgroup_id) group: vec3<u32>) {
    let block = group.y * 256u + group.x;
    if block >= u32(params[0].w) { return; }
    let value = prepare(local, vec3<u32>(block, 0u, 0u));
    let n = u32(params[0].z);
    let slot = local.y * 16u + local.x;
    let inside = local.x < n && local.y < n;
    var residual = vec3<f32>(0.);
    if inside && centre.w == 1. {
        residual = shrink(value, local.y * n + local.x, n, square_root(abs(centre.x) + 0.003));
    }
    // prepare() finishes by reading first[]; all invocations must finish those
    // reads before the inverse transform overwrites the shared coefficients.
    workgroupBarrier();
    first[slot] = vec4<f32>(residual, 0.);
    workgroupBarrier();
    inverse_row(local, n);
    inverse_column(local, n);
    if inside {
        output[block * n * n + local.y * n + local.x] = select(vec4<f32>(0.), vec4<f32>(first[slot].xyz, 1.), centre.w == 1.);
    }
}

fn floor_div(v: i32, d: i32) -> i32 {
    return v / d - select(0, 1, v % d < 0);
}
fn parity(v: i32) -> i32 { return ((v % 2) + 2) % 2; }

// Deterministic gather in the CPU's patch-row/patch-column order. No floating
// atomics and no changes to the staggered, eight-estimate canonical lattice.
@compute @workgroup_size(256)
fn gather(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = i32(params[0].z);
    let end = 1u + u32(n * n) / 4u + u32(params[0].w);
    let rect = vec4<i32>(params[end]);
    if id.x >= u32(rect.z * rect.w) { return; }
    let grid = vec4<i32>(params[end + 1u]);
    let x = rect.x + i32(id.x % u32(rect.z));
    let y = rect.y + i32(id.x / u32(rect.z));
    let step = max(n / 4, 1);
    let first_x = (floor_div(x - n, step) + 1) * step;
    let first_y = (floor_div(y - n, step) + 1) * step;
    var sum = vec4<f32>(0.);
    for (var py = first_y; py <= y; py += step) {
        let row_start = grid.x + parity(grid.x / step + py / step) * step;
        let start = first_x + parity(first_x / step + py / step) * step;
        for (var px = start; px <= x; px += 2 * step) {
            let block = (py - grid.y) / step * grid.z + (px - row_start) / (2 * step);
            sum += output[u32(block * n * n + (y - py) * n + x - px)];
        }
    }
    var p = source_pixel(u32(y) * u32(params[0].x) + u32(x));
    if sum.w > 0. {
        let d = sum.xyz / sum.w;
        let terms = rounded3(vec3(0.2627 * d.y, 0.0593 * d.z, 0.));
        let green = d.x - divided(rounded1(terms.x + terms.y), 0.6780);
        let delta = rounded3(rounded3(vec3(d.x + d.y, green, d.x + d.z)) * p.w);
        p = vec4(p.xyz - delta, p.w);
    }
    filtered[u32(params[end + 2u].x) + id.x] = p;
}
