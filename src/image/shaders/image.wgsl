// Image rendering shader for terminal inline images
// Supports alpha blending and basic transformations

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) tex_coords: vec2<f32>,
}

@group(0) @binding(0) var image_texture: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    
    // Transform from [0,1] to [-1,1] clip space
    let clip_x = input.position.x * 2.0 - 1.0;
    let clip_y = -(input.position.y * 2.0 - 1.0); // Flip Y for correct orientation
    
    output.position = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    output.tex_coords = input.tex_coords;
    
    return output;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(image_texture, image_sampler, input.tex_coords);
    return color;
}
