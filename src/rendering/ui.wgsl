@group(0) @binding(0) var<uniform> transform: vec4<f32>;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Input {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
};
struct Output {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex fn vs_main(input: Input, @builtin(vertex_index) index: u32) -> Output {
    var corners = array<vec2<f32>, 6>(vec2(0.,0.), vec2(0.,1.), vec2(1.,0.),
        vec2(1.,0.), vec2(0.,1.), vec2(1.,1.));
    let corner = corners[index];
    let pixel = input.rect.xy + corner * input.rect.zw;
    var output: Output;
    output.position = vec4(pixel * transform.xy + transform.zw, 0., 1.);
    output.uv = mix(input.uv.xy, input.uv.zw, corner);
    output.color = input.color;
    return output;
}

@fragment fn fs_main(input: Output) -> @location(0) vec4<f32> {
    return textureSample(atlas, atlas_sampler, input.uv) * input.color;
}
