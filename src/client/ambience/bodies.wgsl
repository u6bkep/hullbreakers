// A body (or the sun's disc) at infinity: a quad on the sky round it, shaded
// by casting each pixel's ray at the body (`body_shade`, `sun_shade` in
// `sky.wgsl`), so its edge is sharp at any zoom. Premultiplied over the
// backdrop. See `client::ambience::bodies`.

#import bevy_pbr::mesh_view_bindings::view
#import hullbreakers::sky::{Sky, body_shade, sun_shade}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: Sky;
// x: cd/m² per sky unit; y: which body (0..3), or 4 for the sun.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> params: vec4<f32>;

struct Vertex {
    // The quad's corner, -1..1 in x and y.
    @location(0) corner: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    // Along the pixel's ray (not unit length).
    @location(0) ray: vec3<f32>,
}

const SUN: u32 = 4u;

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    let which = u32(params.y);
    var c: vec3<f32>;
    var reach: f32;
    if which == SUN {
        c = sky.sun.toward;
        // Out to where its wide glow fades (see `sun_shade`).
        reach = sky.sun.radius * (1.0 + 6.0 * (1.5 + 2.0 * sky.sun.corona));
    } else {
        let b = sky.bodies[which];
        c = b.direction;
        if b.kind > 2.5 {
            // A star: its glow (see `body_shade`) reaches several radii.
            reach = b.radius * 5.0;
        } else {
            reach = b.radius * max(1.0, b.ring_outer) * select(1.08, 1.15, b.air > 0.5);
        }
    }
    // A quad on the plane touching the sky at its middle, wide enough to
    // hold all of it.
    reach = min(reach, 1.45);
    let up = select(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), abs(c.y) > 0.95);
    let t1 = normalize(cross(up, c));
    let t2 = cross(c, t1);
    let ray = c + (v.corner.x * t1 + v.corner.y * t2) * tan(reach);
    let clip = view.clip_from_world * vec4(ray, 0.0);
    var out: VertexOutput;
    // On the far plane (reverse Z): behind everything.
    out.clip = vec4(clip.xy, 0.0, clip.w);
    out.ray = ray;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let d = normalize(in.ray);
    // The angle a pixel spans.
    let aa = 2.0 / (view.clip_from_view[1][1] * view.viewport.w);
    let which = u32(params.y);
    var shade: vec4<f32>;
    if which == SUN {
        shade = sun_shade(sky.sun, d, aa);
    } else {
        shade = body_shade(sky.bodies[which], sky.sun, d, aa);
    }
    return vec4(shade.rgb * params.x * view.exposure, shade.a);
}
