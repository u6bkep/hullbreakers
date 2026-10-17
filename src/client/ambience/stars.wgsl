// The stars: one small quad each, at infinity, a fixed number of pixels
// across whatever the field of view, so they stay sharp at any zoom. Each
// is a round core with a soft halo; the brightest have a four-point glint.
// Drawn additively over the backdrop, in sky units (so they bloom). Stars
// behind a body or the sun aren't drawn, and dust dims those behind it.
// See `client::ambience::stars`.

#import bevy_pbr::mesh_view_bindings::view
#import hullbreakers::sky::{Sky, sky_dust}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> sky: Sky;
// x: cd/m² per sky unit.
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> params: vec4<f32>;

struct Vertex {
    // Toward the star.
    @location(0) direction: vec3<f32>,
    // Which corner of its quad, -1..1.
    @location(1) corner: vec2<f32>,
    // Its core's radius and its glint's reach (px; 0: none).
    @location(2) size: vec2<f32>,
    // Its colour, times its brightness (sky units).
    @location(3) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    // From the star's middle (px).
    @location(0) offset: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) size: vec2<f32>,
}

@vertex
fn vertex(v: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let d = normalize(v.direction);
    var clip = view.clip_from_world * vec4(d, 0.0);
    var shown = 1.0;
    for (var i = 0u; i < sky.counts.y; i++) {
        let b = sky.bodies[i];
        if dot(d, b.direction) > cos(b.radius * select(1.0, 1.1, b.air > 0.5)) {
            shown = 0.0;
        }
    }
    if sky.counts.w > 0u && dot(d, sky.sun.toward) > cos(sky.sun.radius * (1.0 + sky.sun.corona)) {
        shown = 0.0;
    }
    if shown > 0.0 {
        shown = 1.0 - 0.92 * sky_dust(sky, d);
    }
    if clip.w <= 0.0 || shown < 0.01 {
        // Off screen: nothing to draw.
        out.clip = vec4(2.0, 2.0, 0.5, 1.0);
        return out;
    }
    let extent = max(v.size.x * 2.5, v.size.y) + 1.5;
    let px = v.corner * extent;
    // On the far plane (reverse Z): behind everything.
    out.clip = vec4(clip.xy + px * 2.0 / view.viewport.zw * clip.w, 0.0, clip.w);
    out.offset = px;
    out.color = v.color.rgb * shown;
    out.size = v.size;
    return out;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let r = length(in.offset);
    let radius = in.size.x;
    let core = clamp(radius + 0.5 - r, 0.0, 1.0);
    let halo = exp(-r * r / (radius * radius * 3.0)) * 0.15;
    var glint = 0.0;
    if in.size.y > 0.0 {
        let ax = abs(in.offset.x);
        let ay = abs(in.offset.y);
        let arm = in.size.y;
        glint = (exp(-ay / 0.45) * clamp(1.0 - ax / arm, 0.0, 1.0) + exp(-ax / 0.45) * clamp(1.0 - ay / arm, 0.0, 1.0)) * 0.45;
    }
    let light = in.color * (core + halo + glint) * params.x * view.exposure;
    return vec4(light, 0.0);
}
