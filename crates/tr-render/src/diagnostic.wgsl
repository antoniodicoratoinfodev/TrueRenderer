// R0 diagnostic only; no Standard/Reference badge is inferred from this test.
@group(0) @binding(0) var<storage, read> input_pixels: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> output_pixels: array<vec4<f32>>;

fn encode(v: f32) -> f32 {
    if v <= 0.0031308 { return v * 12.92; }
    return 1.055 * pow(v, 1.0 / 2.4) - 0.055;
}
@compute @workgroup_size(8,8,1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.y * 64u + id.x;
    if index >= arrayLength(&input_pixels) { return; }
    let pixel = input_pixels[index];
    let background = pow((119.0 / 255.0 + 0.055) / 1.055, 2.4);
    let rgb = pixel.rgb + vec3<f32>(background * (1.0 - pixel.a));
    let output = vec3<f32>(
        dot(rgb, vec3<f32>(1.660491, -0.5876411, -0.0728499)),
        dot(rgb, vec3<f32>(-0.1245505, 1.1328999, -0.0083494)),
        dot(rgb, vec3<f32>(-0.0181508, -0.1005789, 1.1187297))
    );
    output_pixels[index] = vec4<f32>(encode(output.r),encode(output.g),encode(output.b),1.0);
}
