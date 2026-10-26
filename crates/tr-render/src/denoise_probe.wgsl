// Experimental spectral block preparation only. Not an editing renderer.
@group(0) @binding(0) var<storage, read> params: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> source: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read> model: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> filtered: array<vec4<f32>>;
var<workgroup> first: array<vec4<f32>, 256>;
var<workgroup> second: array<vec4<f32>, 256>;
var<workgroup> centre: vec4<f32>;
fn source_pixel(index: u32) -> vec4<f32> { return source[index]; }

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
            let luma = 0.2627 * rgb.x + 0.6780 * rgb.y + 0.0593 * rgb.z;
            value = vec4<f32>(luma, rgb.x - luma, rgb.z - luma, 1.);
        }
    }
    first[slot] = value;
    second[slot] = value;
    workgroupBarrier();
    for (var stride = 128u; stride > 0u; stride = stride / 2u) {
        if slot < stride { first[slot] += first[slot + stride]; }
        workgroupBarrier();
    }
    if slot == 0u { centre = first[0] / f32(n * n); }
    workgroupBarrier();
    var row = vec3<f32>(0.);
    if inside {
        for (var j = 0u; j < n; j += 1u) {
            row += (second[local.y * 16u + j].xyz - centre.xyz) * basis(local.x, j, n);
        }
    }
    first[slot] = vec4<f32>(row, 0.);
    workgroupBarrier();
    var column = vec3<f32>(0.);
    if inside {
        for (var j = 0u; j < n; j += 1u) {
            column += first[j * 16u + local.x].xyz * basis(local.y, j, n);
        }
    }
    return column;
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
            vec4<f32>(centre.x, sqrt(abs(centre.x) + 0.003), 1., 0.), valid);
    }
}

fn removed(ratio: f32) -> f32 {
    let t = clamp((ratio - 0.75) / 0.75, 0., 1.);
    return 1. - t * t * (3. - 2. * t);
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
    let blend = clamp((level - a) / max(b - a, 1e-8), 0., 1.);
    let sa = model[1u + count + lo * n * n + index].xyz;
    let sb = model[1u + count + hi * n * n + index].xyz;
    let sigma = (sa * (1. - blend) + sb * blend) * level * (1. + model[0].w);
    var result = value;
    result.x *= select(removed(abs(value.x) / max(sigma.x * model[0].y * 3., 1e-12)), 0., model[0].y == 0.);
    let strength = model[0].z * 3.25;
    let ratio = sqrt((value.y * value.y + value.z * value.z) /
        max((sigma.y * sigma.y + sigma.z * sigma.z) * strength * strength, 1e-24));
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
        residual = shrink(value, local.y * n + local.x, n, sqrt(abs(centre.x) + 0.003));
    }
    // prepare() finishes by reading first[]; all invocations must finish those
    // reads before the inverse transform overwrites the shared coefficients.
    workgroupBarrier();
    first[slot] = vec4<f32>(residual, 0.);
    workgroupBarrier();
    var row = vec3<f32>(0.);
    if inside {
        for (var k = 0u; k < n; k += 1u) {
            row += first[local.y * 16u + k].xyz * basis(k, local.x, n);
        }
    }
    second[slot] = vec4<f32>(row, 0.);
    workgroupBarrier();
    var column = vec3<f32>(0.);
    if inside {
        for (var k = 0u; k < n; k += 1u) {
            column += second[k * 16u + local.x].xyz * basis(k, local.y, n);
        }
    }
    if inside {
        output[block * n * n + local.y * n + local.x] = select(vec4<f32>(0.), vec4<f32>(column, 1.), centre.w == 1.);
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
        p.x -= (d.x + d.y) * p.w;
        p.y -= (d.x - (0.2627 * d.y + 0.0593 * d.z) / 0.6780) * p.w;
        p.z -= (d.x + d.z) * p.w;
    }
    filtered[u32(params[end + 2u].x) + id.x] = p;
}
