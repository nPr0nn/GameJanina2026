// Textured 2D quads. Same vertex layout as the shape shader, plus UVs
// (location 2). Samples the bound texture (group 1) and multiplies by the
// per-vertex tint color.

struct Globals {
    // Orthographic projection: (0,0)-(render_w,render_h), top-left origin -> NDC.
    proj: mat4x4<f32>,
    time: f32,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

@group(1) @binding(0)
var tex: texture_2d<f32>;
@group(1) @binding(1)
var samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) uv: vec2<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip = globals.proj * vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // The texture is sRGB, so the sample is already linear; the tint is linear
    // too (Color::to_linear on the CPU). Straightforward modulate.
    return textureSample(tex, samp, in.uv) * in.color;
}
