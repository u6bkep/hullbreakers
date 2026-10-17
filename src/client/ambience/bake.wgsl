// Bakes one face of a sky cubemap: the backdrop (band and nebulae), and for
// the light cubemap the bodies too (they light what's near them; the sun
// doesn't, it's the directional light). Drawn as a fullscreen triangle over
// the face. See `client::ambience::bake`.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import hullbreakers::sky::{Sky, backdrop, bodies}

struct Face {
    // Which face: +X, -X, +Y, -Y, +Z, -Z.
    face: u32,
    // 1 for the light cubemap (bodies included).
    light: u32,
    // The angle a texel spans (rad).
    texel: f32,
    pad: f32,
}

@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var<uniform> face: Face;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let a = in.uv.x * 2.0 - 1.0;
    let b = in.uv.y * 2.0 - 1.0;
    var c: vec3<f32>;
    switch face.face {
        case 0u: { c = vec3(1.0, -b, -a); }
        case 1u: { c = vec3(-1.0, -b, a); }
        case 2u: { c = vec3(a, 1.0, b); }
        case 3u: { c = vec3(a, -1.0, -b); }
        case 4u: { c = vec3(a, -b, 1.0); }
        default: { c = vec3(-a, -b, -1.0); }
    }
    // Bevy's cubemaps are left-handed: a texel's world direction has its z
    // turned round (see its skybox and environment-map shaders).
    let d = normalize(vec3(c.x, c.y, -c.z));
    var light = backdrop(sky, d, 0.0);
    if face.light == 1u {
        let over = bodies(sky, d, face.texel);
        light = over.rgb + light * (1.0 - over.a);
    }
    return vec4(light, 1.0);
}
