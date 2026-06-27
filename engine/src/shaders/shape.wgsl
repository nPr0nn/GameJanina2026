// Colored 2D vertices projected from virtual-canvas pixel space into NDC.

struct Globals {
    // Orthographic projection: (0,0)-(render_w,render_h), top-left origin -> NDC.
    proj: mat4x4<f32>,
    // Seconds since startup (for animated custom shaders; unused here).
    time: f32,
};

@group(0) @binding(0)
var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> VsOut {
    var out: VsOut;
    out.clip = globals.proj * vec4<f32>(position, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
