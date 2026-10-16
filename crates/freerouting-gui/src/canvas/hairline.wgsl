// Hairlines: one instance per segment, expanded to a quad in screen space.
// The quad is at least one point wide at every zoom (the instance's width is
// in millimetres and is used when it is wider), and reaches half a width
// past both ends so that joints of a polyline leave no gaps.

struct Camera {
    offset: vec2<f32>,
    zoom: f32,
    pad0: f32,
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

struct Segment {
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    @location(2) width: f32,
};

fn to_screen(p: vec2<f32>) -> vec2<f32> {
    let d = p - camera.offset;
    let rotated = vec2<f32>(
        camera.basis.x * d.x + camera.basis.y * d.y,
        camera.basis.z * d.x + camera.basis.w * d.y,
    );
    return camera.center + rotated * camera.zoom;
}

fn clip_from_screen(s: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(s.x / camera.half_screen.x - 1.0, 1.0 - s.y / camera.half_screen.y, 0.0, 1.0);
}

@vertex
fn vs_main(@builtin(vertex_index) corner: u32, seg: Segment) -> @builtin(position) vec4<f32> {
    let pa = to_screen(seg.a);
    let pb = to_screen(seg.b);
    let d = pb - pa;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if len > 1e-6 {
        dir = d / len;
    }
    let normal = vec2<f32>(-dir.y, dir.x);
    let half_width = 0.5 * max(seg.width * camera.zoom, 1.0);
    // Corners as a triangle strip: start left, start right, end left, end right.
    let at_end = corner >= 2u;
    let side = select(-1.0, 1.0, (corner & 1u) == 1u);
    let base = select(pa, pb, at_end);
    let cap = select(-half_width, half_width, at_end);
    return clip_from_screen(base + dir * cap + normal * (side * half_width));
}

fn linear_from_gamma_rgb(srgb: vec3<f32>) -> vec3<f32> {
    let cutoff = srgb < vec3<f32>(0.04045);
    let lower = srgb / vec3<f32>(12.92);
    let higher = pow((srgb + vec3<f32>(0.055)) / vec3<f32>(1.055), vec3<f32>(2.4));
    return select(higher, lower, cutoff);
}

@fragment
fn fs_linear() -> @location(0) vec4<f32> {
    return vec4<f32>(linear_from_gamma_rgb(fill.color.rgb), fill.color.a);
}

@fragment
fn fs_gamma() -> @location(0) vec4<f32> {
    return fill.color;
}
