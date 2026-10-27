// Global frequency and brightness-conditioned models. Every selected sample,
// quantile index, band boundary and cap follows the canonical CPU estimator.
@group(0) @binding(0) var<storage, read> params: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> samples: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> order: array<u32>;
@group(0) @binding(3) var<storage, read_write> raw: array<f32>;
@group(0) @binding(4) var<storage, read_write> model: array<vec4<f32>>;
var<workgroup> keys: array<vec2<u32>, 1024>;
var<workgroup> values: array<f32, 1024>;
var<workgroup> selected_count: u32;
const SENTINEL: f32 = 3.402823466e38;

fn settings() -> vec4<f32> {
    let n = u32(params[0].z);
    return params[1u + n * n / 4u + u32(params[0].w)];
}
fn sort_values(local: u32, length: u32) {
    // Keep every barrier independent of storage-buffer values. FXC cannot
    // prove that a count produced by an earlier dispatch is workgroup-uniform.
    // Only the compare/swap work depends on length; the sorting network and
    // arithmetic for active elements remain identical.
    for (var k = 2u; k <= 1024u; k *= 2u) {
        for (var j = k / 2u; j > 0u; j /= 2u) {
            for (var i = local; i < length; i += 256u) {
                let other = i ^ j;
                if k <= length && other > i {
                    let a = values[i];
                    let b = values[other];
                    if (a > b) == ((i & k) == 0u) {
                        values[i] = b;
                        values[other] = a;
                    }
                }
            }
            workgroupBarrier();
        }
    }
}
fn high_frequency(i: u32, n: u32) -> bool {
    let frequency = i / n + i % n;
    return frequency >= n / 2u && frequency <= n + n / 2u;
}

@compute @workgroup_size(256)
fn sort_samples(@builtin(local_invocation_index) local: u32) {
    let count = u32(params[0].w);
    for (var i = local; i < 1024u; i += 256u) {
        var level = SENTINEL;
        if i < count && samples[i * 257u + 256u].z == 1. {
            level = samples[i * 257u + 256u].y;
        }
        keys[i] = vec2<u32>(bitcast<u32>(level), i);
    }
    workgroupBarrier();
    for (var k = 2u; k <= 1024u; k *= 2u) {
        for (var j = k / 2u; j > 0u; j /= 2u) {
            for (var i = local; i < 1024u; i += 256u) {
                let other = i ^ j;
                if other > i {
                    let a = keys[i];
                    let b = keys[other];
                    let greater = a.x > b.x || (a.x == b.x && a.y > b.y);
                    if greater == ((i & k) == 0u) { keys[i] = b; keys[other] = a; }
                }
            }
            workgroupBarrier();
        }
    }
    for (var i = local; i < 1024u; i += 256u) { order[2u + i] = keys[i].y; }
    if local == 0u {
        var valid = 0u;
        while valid < count && keys[valid].x != bitcast<u32>(SENTINEL) { valid += 1u; }
        var bands = 0u;
        if valid > 0u { bands = clamp(valid / 64u, 1u, 4u); }
        order[0] = valid;
        order[1] = bands;
        model[0] = vec4<f32>(f32(bands), settings().yzw);
        for (var band = 0u; band < bands; band += 1u) {
            let start = band * valid / bands;
            let end = (band + 1u) * valid / bands;
            model[1u + band] = vec4<f32>(bitcast<f32>(keys[start + (end - start) / 2u].x), 0., 0., 0.);
        }
    }
}

@compute @workgroup_size(256)
fn quantiles(@builtin(local_invocation_index) local: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let n = u32(params[0].z);
    let band = group.z;
    let enabled_band = band <= order[1];
    var start = 0u;
    var end = select(0u, order[0], enabled_band);
    if band > 0u && enabled_band {
        start = (band - 1u) * order[0] / max(order[1], 1u);
        end = band * order[0] / max(order[1], 1u);
    }
    let count = end - start;
    var length = 1u;
    while length < count { length *= 2u; }
    for (var i = local; i < length; i += 256u) {
        var value = SENTINEL;
        if i < count {
            let sample_id = order[2u + start + i];
            let coefficient = samples[sample_id * 257u + (group.x / n) * 16u + group.x % n][group.y];
            value = abs(coefficient) / samples[sample_id * 257u + 256u].y;
        }
        values[i] = value;
    }
    workgroupBarrier();
    sort_values(local, length);
    if local == 0u && enabled_band {
        var value = 0.;
        if count > 0u { value = values[u32(f32(count - 1u) * 0.3)] / 0.38532; }
        raw[(band * n * n + group.x) * 3u + group.y] = value;
    }
}

@compute @workgroup_size(256)
fn cap_global(@builtin(local_invocation_index) local: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let n = u32(params[0].z);
    if local < n * n {
        values[local] = select(SENTINEL, raw[local * 3u + group.x], high_frequency(local, n));
    }
    if local == 0u {
        selected_count = 0u;
        for (var i = 0u; i < n * n; i += 1u) { if high_frequency(i, n) { selected_count += 1u; } }
    }
    workgroupBarrier();
    sort_values(local, n * n);
    let cap = 8. * values[(selected_count - 1u) / 2u];
    if local < n * n {
        let opposite = (local % n) * n + local / n;
        var value = min((raw[local * 3u + group.x] + raw[opposite * 3u + group.x]) * 0.5, cap);
        if local == 0u { value = 0.; }
        raw[(5u * n * n + local) * 3u + group.x] = value;
    }
}

@compute @workgroup_size(256)
fn band_statistics(@builtin(local_invocation_index) local: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let enabled_band = group.x < order[1];
    let n = u32(params[0].z);
    let global = 5u * n * n * 3u;
    let stats = 6u * n * n * 3u + (group.x * 3u + group.y) * 2u;
    if local < n * n {
        let g = raw[global + local * 3u + group.y];
        var value = SENTINEL;
        if high_frequency(local, n) && g > 1e-8 { value = raw[((group.x + 1u) * n * n + local) * 3u + group.y] / g; }
        values[local] = value;
    }
    if local == 0u {
        selected_count = 0u;
        for (var i = 0u; i < n * n; i += 1u) {
            if high_frequency(i, n) && raw[global + i * 3u + group.y] > 1e-8 { selected_count += 1u; }
        }
    }
    workgroupBarrier();
    sort_values(local, n * n);
    if local == 0u && enabled_band {
        var gain = 1.;
        if selected_count > 0u { gain = clamp(values[(selected_count - 1u) / 2u], 0.25, 4.); }
        raw[stats] = gain;
    }
    workgroupBarrier();
    if local < n * n { values[local] = select(SENTINEL, raw[global + local * 3u + group.y], high_frequency(local, n)); }
    if local == 0u {
        selected_count = 0u;
        for (var i = 0u; i < n * n; i += 1u) { if high_frequency(i, n) { selected_count += 1u; } }
    }
    workgroupBarrier();
    sort_values(local, n * n);
    if local == 0u && enabled_band { raw[stats + 1u] = select(8., 1.5, settings().x != 0.) * values[(selected_count - 1u) / 2u]; }
}

@compute @workgroup_size(256)
fn finish_model(@builtin(global_invocation_id) id: vec3<u32>) {
    let n = u32(params[0].z);
    let band = id.x / (n * n);
    let frequency = id.x % (n * n);
    if band >= order[1] { return; }
    var sigma = vec4<f32>(0.);
    if frequency != 0u {
        for (var c = 0u; c < 3u; c += 1u) {
            let global = raw[(5u * n * n + frequency) * 3u + c];
            let stats = 6u * n * n * 3u + (band * 3u + c) * 2u;
            sigma[c] = min(global, raw[stats + 1u]) * raw[stats];
        }
    }
    model[1u + order[1] + id.x] = sigma;
}
