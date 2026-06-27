// Blits the render texture onto the swapchain. The aspect-fit viewport is set
// on the CPU (set_viewport) so this just draws a fullscreen triangle and
// samples the render texture across [0,1] UVs.

@group(0) @binding(0)
var tex: texture_2d<f32>;
@group(0) @binding(1)
var samp: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VsOut {
    // Oversized triangle covering the viewport. UVs span [0,1].
    var out: VsOut;
    let uv = vec2<f32>(f32((idx << 1u) & 2u), f32(idx & 2u));
    out.uv = uv;
    out.clip = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    // Flip Y: UV origin is top-left, NDC +Y is up.
    out.clip.y = -out.clip.y;
    return out;
}

// Encode linear -> sRGB. The render texture is sampled as linear (it is an sRGB
// texture, so the hardware decodes on read); the swapchain is linear (non-sRGB),
// so we apply the sRGB transfer function here. Doing it manually keeps colors
// identical across native, WebGPU and WebGL2 regardless of swapchain support.
fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lower = c * 12.92;
    let higher = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(higher, lower, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv);
    return vec4<f32>(linear_to_srgb(c.rgb), c.a);
}
