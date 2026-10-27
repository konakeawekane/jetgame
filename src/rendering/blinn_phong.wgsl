struct Scene {
    view_projection: mat4x4<f32>,
    camera_position: vec4<f32>,
    light_direction: vec4<f32>,
    light_color: vec4<f32>,
    color_ambient: vec4<f32>,
    specular_shininess: vec4<f32>,
};
@group(0) @binding(0) var<uniform> scene: Scene;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) normal_0: vec4<f32>,
    @location(7) normal_1: vec4<f32>,
    @location(8) normal_2: vec4<f32>,
};
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex fn vs_main(input: VertexInput) -> VertexOutput {
    let model = mat4x4<f32>(input.model_0, input.model_1, input.model_2, input.model_3);
    let normal_matrix = mat3x3<f32>(input.normal_0.xyz, input.normal_1.xyz, input.normal_2.xyz);
    let world = model * vec4<f32>(input.position, 1.0);
    var output: VertexOutput;
    output.clip_position = scene.view_projection * world;
    output.world_position = world.xyz;
    output.normal = normal_matrix * input.normal;
    return output;
}

fn safe_normalize(value: vec3<f32>) -> vec3<f32> {
    return value * inverseSqrt(max(dot(value, value), 0.00000001));
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let normal = safe_normalize(input.normal);
    let light = scene.light_direction.xyz;
    let view = safe_normalize(scene.camera_position.xyz - input.world_position);
    let halfway = safe_normalize(light + view);
    let diffuse = max(dot(normal, light), 0.0);
    var specular = 0.0;
    if diffuse > 0.0 {
        specular = scene.specular_shininess.x *
            pow(max(dot(normal, halfway), 0.0), scene.specular_shininess.y);
    }
    let base = scene.color_ambient.rgb;
    let color = base * scene.color_ambient.a +
        scene.light_color.rgb * (base * diffuse + vec3<f32>(specular));
    // Lighting is linear; the sRGB color target performs display encoding.
    return vec4<f32>(color, 1.0);
}
