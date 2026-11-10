struct Camera {
    view_projection: mat4x4<f32>,
    time: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var celestial: texture_2d<f32>;
@group(1) @binding(1) var celestial_sampler: sampler;

struct VertexInput { @location(0) position: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32> };
struct VertexOutput { @builtin(position) clip_position: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) color: vec4<f32> };

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = camera.view_projection * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    output.color = input.color;
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(celestial, celestial_sampler, input.uv);
    // With Java's additive blend, pure black opaque texels leave the sky
    // unchanged. Discard keeps them from becoming a visible black backdrop.
    if (texel.r <= 0.001 && texel.g <= 0.001 && texel.b <= 0.001) {
        discard;
    }
    return texel * input.color;
}
