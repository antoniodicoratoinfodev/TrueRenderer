@compute @workgroup_size(8, 8)
fn prefix(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(output);
    if any(id.xy >= size) { return; }
    var pixel = spectral_source_pixel(source, id.y * size.x + id.x, size.x, params.data[6].z != 0.);
    pixel = vec4(pixel.rgb * params.data[5].xyz, pixel.a);
    if pixel.a > 0. && params.data[6].x != 0. {
        pixel = vec4(pixel.rgb * optical[id.y * size.x + id.x], pixel.a);
    }
    textureStore(output, vec2<i32>(id.xy), edit_prefix(pixel));
}
