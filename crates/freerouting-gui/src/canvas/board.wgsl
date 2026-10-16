// Filled triangles of the board. One draw is one chunk of one (slot, kind);
// the fill colour comes from a dynamic-offset uniform, premultiplied and in
// gamma space, exactly as egui's own vertex colours are.

struct Camera {
    // The camera centre, in millimetres relative to the board origin.
    offset: vec2<f32>,
    // Points per millimetre.
    zoom: f32,
    pad0: f32,
    // The canvas centre in screen points (y down), and half the screen size
    // in points.
    center: vec2<f32>,
    half_screen: vec2<f32>,
    // The camera's orientation: a board offset (dx, dy) lands
    // zoom * (basis.x * dx + basis.y * dy, basis.z * dx + basis.w * dy) points
    // from the centre (screen y down). The entries are 0, 1 or -1.
    basis: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

struct Fill {
    color: vec4<f32>,
};

@group(1) @binding(0) var<uniform> fill: Fill;

// A board point in screen points.
fn to_screen(p: vec2<f32>) -> vec2<f32> {
    let d = p - camera.offset;
    let rotated = vec2<f32>(
        camera.basis.x * d.x + camera.basis.y * d.y,
        camera.basis.z * d.x + camera.basis.w * d.y,
    );
    return camera.center + rotated * camera.zoom;
}

// Screen points to clip space, as egui's shader does.
fn clip_from_screen(s: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(s.x / camera.half_screen.x - 1.0, 1.0 - s.y / camera.half_screen.y, 0.0, 1.0);
}

@vertex
fn vs_main(@location(0) pos: vec2<f32>) -> @builtin(position) vec4<f32> {
    return clip_from_screen(to_screen(pos));
}

// 0-1 linear from 0-1 sRGB gamma (the function egui's shader uses).
fn linear_from_gamma_rgb(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let lower = srgb / vec3<f32>(12.92);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

// For an sRGB render target, which converts to gamma on write.
@fragment
fn fs_linear() -> @location(0) vec4<f32> {
    return vec4<f32>(linear_from_gamma_rgb(fill.color.rgb), fill.color.a);
}

// For a target that stores gamma values as they are.
@fragment
fn fs_gamma() -> @location(0) vec4<f32> {
    return fill.color;
}
