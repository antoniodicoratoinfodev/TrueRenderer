struct Params { source_width:u32, width:u32, height:u32, min_y:u32, rows:u32, opaque:u32, pad0:u32, pad1:u32 }
struct Tap { index:u32, weight:f32 }
@group(0) @binding(0) var<uniform> params:Params;
@group(0) @binding(1) var<storage,read> source:array<vec4<f32>>;
@group(0) @binding(2) var<storage,read_write> horizontal:array<vec4<f32>>;
@group(0) @binding(3) var<storage,read_write> output_pixels:array<vec4<f32>>;
@group(0) @binding(4) var<storage,read> x_ranges:array<vec2<u32>>;
@group(0) @binding(5) var<storage,read> x_taps:array<Tap>;
@group(0) @binding(6) var<storage,read> y_ranges:array<vec2<u32>>;
@group(0) @binding(7) var<storage,read> y_taps:array<Tap>;
@group(0) @binding(8) var display:texture_storage_2d<rgba8unorm,write>;

@compute @workgroup_size(8,8)
fn horizontal_pass(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=params.width || id.y>=params.rows { return; }
    let taps=x_ranges[id.x];
    var sum=vec4<f32>(0.0);
    for(var i=0u;i<taps.y;i+=1u) {
        let tap=x_taps[taps.x+i];
        sum+=source[(id.y+params.min_y)*params.source_width+tap.index]*tap.weight;
    }
    horizontal[id.y*params.width+id.x]=sum;
}
@compute @workgroup_size(8,8)
fn vertical_pass(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=params.width || id.y>=params.height { return; }
    let taps=y_ranges[id.y];
    var sum=vec4<f32>(0.0);
    for(var i=0u;i<taps.y;i+=1u) {
        let tap=y_taps[taps.x+i];
        sum+=horizontal[(tap.index-params.min_y)*params.width+id.x]*tap.weight;
    }
    if params.opaque != 0u { sum.a=1.0; }
    output_pixels[id.y*params.width+id.x]=sum;
}
fn encode(v:f32)->f32 {
    if v<=0.0031308 { return 12.92*v; }
    return 1.055*pow(v,1.0/2.4)-0.055;
}
@compute @workgroup_size(8,8)
fn display_pass(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x>=params.width || id.y>=params.height { return; }
    let p=output_pixels[id.y*params.width+id.x];
    // Analytic conversion/composition matching color::display_pixel(119/255).
    let bg=pow((119.0/255.0+0.055)/1.055,2.4);
    let rgb=p.rgb+vec3<f32>(bg*(1.0-p.a));
    let r=1.660491*rgb.r-0.5876411*rgb.g-0.0728499*rgb.b;
    let g=-0.1245505*rgb.r+1.1328999*rgb.g-0.0083494*rgb.b;
    let b=-0.0181508*rgb.r-0.1005789*rgb.g+1.1187297*rgb.b;
    textureStore(display,vec2<i32>(id.xy),vec4<f32>(clamp(vec3<f32>(encode(r),encode(g),encode(b)),vec3<f32>(0.0),vec3<f32>(1.0)),1.0));
}
