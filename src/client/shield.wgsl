// A ship's shield bubble, drawn additively on both faces of an ellipsoid.
//
// At rest it is only a faint rim (brighter where the view grazes it, like a
// soap bubble), so a crew inside it still sees out. Beam hits light it up:
// a hot spot at each hit, rings running out from it across a lattice, and
// the whole bubble flaring. When the shield fails it flashes and breaks up.

#import bevy_pbr::{forward_io::VertexOutput, mesh_view_bindings::view}

struct Shield {
    // rgb: tint, a: resting rim strength.
    color: vec4<f32>,
    // Up to four hits: world position, and strength (0 = none).
    hits: array<vec4<f32>, 4>,
    // Seconds since each hit began.
    ages: vec4<f32>,
    // x: time (s), y: 1 when the camera is inside, z: collapse (0..1), w: flare.
    params: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> shield: Shield;

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let v = normalize(view.world_position - p);
    let n = normalize(in.world_normal);
    let facing = abs(dot(n, v));
    let time = shield.params.x;
    let inside = shield.params.y;
    let collapse = shield.params.z;
    let flare = shield.params.w;

#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2(0.0);
#endif
    // A faint hex-ish lattice: two families of lines on the sphere's UVs.
    let cells = uv * vec2(64.0, 32.0);
    let lines = abs(fract(cells + vec2(0.5 * floor(cells.y), 0.0)) - 0.5);
    let lattice = 1.0 - smoothstep(0.0, 0.05, min(lines.x, lines.y));

    // Rim: grazing views look through more of the shell. From inside there
    // is no rim to see, and a resting shield shows nothing at all, so it
    // never fogs the crew's view; only hits light it up.
    let rim = pow(1.0 - facing, 3.0);
    var glow = shield.color.a * (0.03 + rim) * (1.0 - inside);

    var hit = 0.0;
    for (var i = 0; i < 4; i++) {
        let h = shield.hits[i];
        if h.w <= 0.0 {
            continue;
        }
        let d = distance(p, h.xyz);
        let age = shield.ages[i];
        // A hot spot that shimmers.
        let spot = exp(-d * d / 3.0) * (0.8 + 0.2 * sin(time * 40.0 + d * 3.0));
        // The first impact's shock ring, then a ring every 0.7 s, lighting
        // up the lattice as they pass.
        let first = exp(-pow((d - age * 14.0) / 1.2, 2.0)) * max(1.0 - age * 0.6, 0.0) * 1.2;
        let phase = fract(age / 0.7);
        let ring = exp(-pow((d - phase * 8.0) / 0.7, 2.0)) * (1.0 - phase) * 0.6;
        hit += h.w * (spot * 2.5 + (first + ring) * (0.15 + 0.85 * lattice));
    }
    // The whole bubble flares (at the rim, from outside) when struck.
    glow += hit + flare * (0.02 + 0.4 * rim) * (1.0 - 0.85 * inside);

    // Failing: a white flash that breaks up into cells and fades.
    if collapse > 0.0 {
        let cell = hash(floor(cells * 0.5));
        let alive = step(collapse, cell * 0.9 + 0.05);
        glow = (glow + (1.0 - collapse) * 2.0 * (0.2 + lattice)) * alive;
    }

    // Additive: alpha 0 leaves what's behind untouched.
    return vec4(shield.color.rgb * glow, 0.0);
}
