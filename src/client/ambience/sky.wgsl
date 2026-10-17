// The sky, as functions of a direction: shared by the bake (the backdrop
// and the light cubemaps), the stars and the bodies' impostors, so the light
// a place gives is the sky you see in it. See `client::ambience` for the
// data (`SkyUniform`) and `sim::ambience` for what each field means.
//
// The look is stylized: densities are drawn in a few flat tones with soft
// edges, dust is hard-edged dark shapes, cloud edges facing the sun are
// rimmed with light, rocky bodies are faceted like the asteroids and lit in
// bands. Directions are world space, unit length. Radiance is in sky units
// (the caller scales to cd/m² and the view's exposure).

#define_import_path hullbreakers::sky

struct SkySun {
    toward: vec3<f32>,
    // The disc's angular radius (rad).
    radius: f32,
    color: vec3<f32>,
    // How far the corona reaches past the disc, in disc radii.
    corona: f32,
}

struct SkyBand {
    pole: vec3<f32>,
    // Angular half-width (rad).
    width: f32,
    bulge: vec3<f32>,
    dust: f32,
    // Already scaled by its brightness.
    color: vec3<f32>,
    tones: f32,
    bulge_color: vec3<f32>,
    seed: f32,
}

struct SkyNebula {
    direction: vec3<f32>,
    // Angular radius (rad).
    radius: f32,
    inner: vec3<f32>,
    tones: f32,
    outer: vec3<f32>,
    dust: f32,
    // The way it's drawn out (a tangent at its middle).
    axis: vec3<f32>,
    stretch: f32,
    seed: f32,
    rim: f32,
    pad: vec2<f32>,
}

struct SkyBody {
    direction: vec3<f32>,
    // Angular radius of the disc (rad).
    radius: f32,
    color_a: vec3<f32>,
    // 0 rocky, 1 gas giant, 2 ice, 3 star.
    kind: f32,
    color_b: vec3<f32>,
    seed: f32,
    color_c: vec3<f32>,
    // How many of the three colours it has.
    colors: f32,
    axis: vec3<f32>,
    // Rings, in body radii (0: none).
    ring_inner: f32,
    ring_color: vec3<f32>,
    ring_outer: f32,
    atmosphere: vec3<f32>,
    // 1 if it has an atmosphere.
    air: f32,
}

struct Sky {
    sun: SkySun,
    band: SkyBand,
    nebulae: array<SkyNebula, 8>,
    bodies: array<SkyBody, 4>,
    // Nebulae shown, bodies shown, band shown (0/1), sun disc shown (0/1).
    counts: vec4<u32>,
}

// How bright a lit body is (sky units, times its colour) and the sun's disc.
const BODY_LIGHT: f32 = 0.55;
const BODY_NIGHT: f32 = 0.035;
const SUN_DISC: f32 = 40.0;
const SUN_GLOW: f32 = 0.3;
const STAR_DISC: f32 = 14.0;
const BULGE_GAIN: f32 = 1.5;
// The same as `sim::ambience::BACKDROP_CAP`.
const BACKDROP_CAP: f32 = 0.3;
const PI: f32 = 3.14159265;

// ---------------------------------------------------------------------------
// Noise

fn pcg3(v_in: vec3<u32>) -> vec3<u32> {
    var v = v_in * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v = v ^ (v >> vec3(16u));
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return v;
}

// A pseudo-random gradient at an integer lattice point, -1..1 each axis.
fn gradient(i: vec3<f32>) -> vec3<f32> {
    let h = pcg3(bitcast<vec3<u32>>(vec3<i32>(i)));
    return vec3<f32>(h & vec3(0xffffu)) / 32767.5 - 1.0;
}

// Gradient noise, roughly -1..1.
fn noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let n000 = dot(gradient(i), f);
    let n100 = dot(gradient(i + vec3(1.0, 0.0, 0.0)), f - vec3(1.0, 0.0, 0.0));
    let n010 = dot(gradient(i + vec3(0.0, 1.0, 0.0)), f - vec3(0.0, 1.0, 0.0));
    let n110 = dot(gradient(i + vec3(1.0, 1.0, 0.0)), f - vec3(1.0, 1.0, 0.0));
    let n001 = dot(gradient(i + vec3(0.0, 0.0, 1.0)), f - vec3(0.0, 0.0, 1.0));
    let n101 = dot(gradient(i + vec3(1.0, 0.0, 1.0)), f - vec3(1.0, 0.0, 1.0));
    let n011 = dot(gradient(i + vec3(0.0, 1.0, 1.0)), f - vec3(0.0, 1.0, 1.0));
    let n111 = dot(gradient(i + vec3(1.0, 1.0, 1.0)), f - vec3(1.0, 1.0, 1.0));
    let x00 = mix(n000, n100, u.x);
    let x10 = mix(n010, n110, u.x);
    let x01 = mix(n001, n101, u.x);
    let x11 = mix(n011, n111, u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z) * 1.6;
}

fn fbm(p: vec3<f32>, octaves: i32) -> f32 {
    var sum = 0.0;
    var amplitude = 0.5;
    var q = p;
    for (var i = 0; i < octaves; i++) {
        sum += amplitude * noise(q);
        q = q * 2.03 + vec3(17.1, 31.7, 7.3);
        amplitude *= 0.5;
    }
    return sum;
}

// A 0..1 hash of a float.
fn hash1(x: f32) -> f32 {
    return f32(pcg3(vec3<u32>(bitcast<u32>(x), 0x9e37u, 0x79b9u)).x & 0xffffu) / 65535.0;
}

// ---------------------------------------------------------------------------
// Tones: a value drawn in `steps` flat steps, each edge `soft` wide (in
// steps; widened to `aa`, the value's change across a pixel, if wider).

fn tones(x: f32, steps: f32, soft: f32, aa: f32) -> f32 {
    let s = max(x, 0.0) * steps;
    let base = floor(s);
    let f = s - base;
    let w = max(soft, aa * steps);
    return (base + smoothstep(0.5 - w, 0.5 + w, f)) / steps;
}

// ---------------------------------------------------------------------------
// The galactic band

fn band_profile(b: SkyBand, d: vec3<f32>) -> vec2<f32> {
    let lat = asin(clamp(dot(d, b.pole), -1.0, 1.0));
    let in_plane = d - b.pole * dot(d, b.pole);
    let along = acos(clamp(dot(normalize(in_plane + vec3(1e-6)), b.bulge), -1.0, 1.0));
    let bulge = exp(-pow(along / 0.55, 2.0));
    let width = b.width * (1.0 + 1.3 * bulge);
    let wobble = fbm(d * 3.0 + vec3(b.seed), 4);
    let q = lat / (width * (1.0 + 0.4 * wobble));
    let profile = exp(-q * q);
    return vec2(profile, bulge);
}

fn band_dust(b: SkyBand, d: vec3<f32>, profile: f32) -> f32 {
    if b.dust <= 0.0 {
        return 0.0;
    }
    // Filaments: the ridges of a noise, near the band's middle.
    let ridge = 1.0 - abs(fbm(d * 5.5 + vec3(b.seed * 1.3 + 40.0), 4));
    let filaments = smoothstep(0.82, 0.86, ridge);
    // And a lane down the middle, wandering.
    let lat = asin(clamp(dot(d, b.pole), -1.0, 1.0));
    let lane_at = b.width * 0.25 * fbm(d * 2.0 + vec3(b.seed + 9.0), 3);
    let lane = (1.0 - smoothstep(0.45, 0.55, abs(lat - lane_at) / (b.width * 0.22 + 1e-4)))
        * smoothstep(-0.1, 0.25, fbm(d * 4.0 + vec3(b.seed + 21.0), 3));
    return clamp(max(filaments, lane) * smoothstep(0.15, 0.5, profile) * b.dust, 0.0, 1.0);
}

fn band_light(b: SkyBand, d: vec3<f32>, aa: f32) -> vec3<f32> {
    let pb = band_profile(b, d);
    let grain = 0.8 + 0.2 * fbm(d * 9.0 + vec3(b.seed * 1.7), 3);
    let glow = tones(pb.x * grain, b.tones, 0.06, aa);
    let core = tones(pb.y * pb.x * pb.x, b.tones, 0.06, aa);
    var light = b.color * glow + b.bulge_color * BULGE_GAIN * core;
    // A faint haze everywhere, so the band's sky isn't black.
    light += b.color * 0.04;
    return light * (1.0 - 0.9 * band_dust(b, d, pb.x));
}

// ---------------------------------------------------------------------------
// Nebulae

// Where `d` is in the nebula's frame: its distance from the middle in
// radii (stretched), and the noise coordinates there.
fn nebula_density(n: SkyNebula, d: vec3<f32>) -> f32 {
    let c = n.direction;
    let cosang = clamp(dot(d, c), -1.0, 1.0);
    let ang = acos(cosang);
    let t = normalize(d - c * cosang + vec3(1e-6));
    let side = cross(c, n.axis);
    let x = dot(t, n.axis) * ang;
    let y = dot(t, side) * ang * n.stretch;
    let r = length(vec2(x, y)) / n.radius;
    // Features scale with its size: a few big lobes, not a fractal spray.
    let scale = 1.3 / max(n.radius, 0.12);
    let p = d * scale + vec3(n.seed);
    let warp = vec3(fbm(p + vec3(3.1, 0.0, 0.0), 2), fbm(p + vec3(0.0, 5.7, 0.0), 2), fbm(p + vec3(0.0, 0.0, 9.3), 2));
    let shape = fbm(p + warp * 0.8, 4);
    return clamp((1.0 - r) * 1.3 + shape * 1.5 - 0.15, 0.0, 1.0);
}

fn nebula_light(n: SkyNebula, sun: vec3<f32>, d: vec3<f32>, aa: f32) -> vec3<f32> {
    if dot(d, n.direction) < cos(min(n.radius * 1.6, PI)) {
        return vec3(0.0);
    }
    let density = nebula_density(n, d);
    if density <= 0.0 {
        return vec3(0.0);
    }
    let tone = tones(density, n.tones, 0.05, aa);
    var light = mix(n.outer, n.inner, tone) * tone;
    // The edges facing the sun, lit: where the cloud thins toward it.
    if n.rim > 0.0 {
        let toward = sun - d * dot(sun, d);
        let step_to = normalize(d + normalize(toward + vec3(1e-6)) * n.radius * 0.06);
        let thinning = density - nebula_density(n, step_to);
        let rim = smoothstep(0.08, 0.14, thinning) * step(0.02, tone);
        light += n.inner * n.rim * rim;
    }
    // Dust: hard dark shapes across it.
    if n.dust > 0.0 {
        let scale = 2.2 / max(n.radius, 0.12);
        let dust = smoothstep(0.18, 0.24, fbm(d * scale + vec3(n.seed + 51.0), 3)) * n.dust;
        light *= 1.0 - 0.85 * dust;
    }
    return light;
}

// How much of what's behind it the dust at `d` hides (0..1), band and
// nebulae together: stars dim behind it.
fn sky_dust(sky: Sky, d: vec3<f32>) -> f32 {
    var hidden = 0.0;
    if sky.counts.z > 0u {
        let pb = band_profile(sky.band, d);
        hidden = max(hidden, band_dust(sky.band, d, pb.x));
    }
    for (var i = 0u; i < sky.counts.x; i++) {
        let n = sky.nebulae[i];
        if n.dust <= 0.0 || dot(d, n.direction) < cos(min(n.radius * 1.6, PI)) {
            continue;
        }
        if nebula_density(n, d) <= 0.0 {
            continue;
        }
        let scale = 2.2 / max(n.radius, 0.12);
        hidden = max(hidden, smoothstep(0.18, 0.24, fbm(d * scale + vec3(n.seed + 51.0), 3)) * n.dust);
    }
    return hidden;
}

// The backdrop: band and nebulae (`aa`: how much a value can change
// across a pixel, for the tones' edges).
fn backdrop(sky: Sky, d: vec3<f32>, aa: f32) -> vec3<f32> {
    var light = vec3(0.0);
    if sky.counts.z > 0u {
        light += band_light(sky.band, d, aa);
    }
    for (var i = 0u; i < sky.counts.x; i++) {
        light += nebula_light(sky.nebulae[i], sky.sun.toward, d, aa);
    }
    // Where features overlap they add up: held to the cap a single one is
    // validated against (`sim::ambience::BACKDROP_CAP`).
    let luminance = dot(light, vec3(0.2126, 0.7152, 0.0722));
    if luminance > BACKDROP_CAP {
        light *= BACKDROP_CAP / luminance;
    }
    return light;
}

// ---------------------------------------------------------------------------
// Bodies

// The nearest of a few dozen points spread over the sphere (turned by
// `seed`): a facet, and its middle.
fn facet(n: vec3<f32>, seed: f32, count: f32) -> vec4<f32> {
    var best = -2.0;
    var best_at = vec3(0.0, 1.0, 0.0);
    var best_i = 0.0;
    let golden = 2.39996323;
    let turn = seed * 0.618;
    for (var i = 0.0; i < count; i += 1.0) {
        let y = 1.0 - 2.0 * (i + 0.5) / count;
        let r = sqrt(max(0.0, 1.0 - y * y));
        let a = golden * i + turn;
        // Jittered, so the facets aren't a regular lattice.
        let j = vec3(hash1(i + seed) - 0.5, hash1(i * 1.7 + seed) - 0.5, hash1(i * 2.3 + seed) - 0.5) * (1.6 / sqrt(count));
        let p = normalize(vec3(cos(a) * r, y, sin(a) * r) + j);
        let dp = dot(n, p);
        if dp > best {
            best = dp;
            best_at = p;
            best_i = i;
        }
    }
    return vec4(best_at, best_i);
}

// Lit in bands: full, half, night.
fn banded_light(lambert: f32, aa: f32) -> f32 {
    let w = max(0.03, aa);
    let lit = smoothstep(0.32 - w, 0.32 + w, lambert);
    let mid = smoothstep(-0.02 - w, -0.02 + w, lambert);
    return mix(BODY_NIGHT, mix(0.5, 1.0, lit), mid);
}

fn body_palette(b: SkyBody, k: f32) -> vec3<f32> {
    if b.colors < 1.5 {
        return b.color_a;
    }
    if b.colors < 2.5 {
        return select(b.color_a, b.color_b, k > 0.5);
    }
    if k < 0.34 {
        return b.color_a;
    }
    return select(b.color_b, b.color_c, k > 0.67);
}

// A body seen along `d`: premultiplied colour (sky units, lit by `sun`)
// and coverage. `aa`: the angle a pixel spans (rad).
fn body_shade(b: SkyBody, sun: SkySun, d: vec3<f32>, aa: f32) -> vec4<f32> {
    let c = b.direction;
    let rr = sin(b.radius);
    let tc = dot(d, c);
    let h2 = max(0.0, 1.0 - tc * tc);
    let edge = (rr - sqrt(h2)) / max(aa, 1e-5);
    let covered = clamp(edge + 0.5, 0.0, 1.0) * step(0.0, tc);
    var out = vec4(0.0);
    var planet_t = 1e9;
    if b.kind > 2.5 {
        // A companion star: a bright disc and a soft glow round it.
        let ang = acos(clamp(tc, -1.0, 1.0));
        let x = ang / b.radius;
        let glow = exp(-max(x - 1.0, 0.0) * 1.6) * 0.5 * step(1.0, x);
        let disc = b.color_a * STAR_DISC * (1.0 - 0.3 * x * x);
        return vec4(disc * covered + b.color_a * glow * 2.0 * (1.0 - covered), covered);
    }
    if covered > 0.0 {
        let t = tc - sqrt(max(rr * rr - h2, 0.0));
        planet_t = t;
        let n = normalize(d * t - c);
        var colour = b.color_a;
        var normal = n;
        if b.kind < 0.5 || (b.kind > 1.5 && b.kind < 2.5) {
            // Rock or ice: flat facets, each its own shade of the palette,
            // more of them on a bigger body (ice has fewer, larger).
            let degrees = b.radius * 57.3;
            let count = clamp(40.0 + degrees * 9.0, 40.0, 240.0) * select(1.0, 0.6, b.kind > 1.5);
            let f = facet(n, b.seed, count);
            normal = f.xyz;
            // Colour by regions bigger than a facet (continents, ice
            // fields), each facet a little lighter or darker.
            let k = clamp(fbm(f.xyz * 1.6 + vec3(b.seed), 3) * 1.4 + 0.5, 0.0, 1.0);
            colour = body_palette(b, k) * (0.88 + 0.24 * hash1(f.w * 7.7 + b.seed));
        } else {
            // A gas giant: bands by latitude, wavering.
            let lat = dot(n, b.axis);
            let waver = 0.08 * fbm(n * 4.0 + vec3(b.seed), 3);
            let band = fract((lat + waver) * 3.2 + 0.5);
            let k = floor(band * 3.0) / 3.0 + 0.1;
            colour = body_palette(b, k);
        }
        let lambert = dot(normal, sun.toward);
        var lit = banded_light(lambert, aa * 6.0);
        if b.kind > 1.5 && b.kind < 2.5 {
            // Ice: a sheen on the facets facing the sun's reflection.
            let r = reflect(-sun.toward, normal);
            lit += 0.6 * smoothstep(0.9, 0.93, dot(r, -d)) * step(0.0, lambert);
        }
        var shade = colour * sun.color * BODY_LIGHT * lit;
        // Air: its edge glows, brighter on the sunward side.
        if b.air > 0.5 {
            let limb = pow(1.0 - clamp(dot(n, -d), 0.0, 1.0), 3.0);
            shade += b.atmosphere * BODY_LIGHT * limb * (0.15 + 0.85 * smoothstep(-0.2, 0.4, lambert));
        }
        out = vec4(shade * covered, covered);
    }
    if b.air > 0.5 && covered < 1.0 {
        // The air's halo past the disc.
        let ang = acos(clamp(tc, -1.0, 1.0));
        let x = (ang - b.radius) / (b.radius * 0.1);
        let halo = clamp(1.0 - x, 0.0, 1.0);
        let side = normalize(d - c * tc + vec3(1e-6));
        let sunward = smoothstep(-0.3, 0.6, dot(side, sun.toward));
        let glow = b.atmosphere * BODY_LIGHT * halo * halo * (0.1 + 0.9 * sunward) * 0.7;
        out = vec4(out.rgb + glow * (1.0 - covered), out.a);
    }
    if b.ring_outer > 0.0 {
        // Rings in the equator's plane, behind or in front of the disc.
        let denom = dot(d, b.axis);
        if abs(denom) > 1e-4 {
            let t = dot(c, b.axis) / denom;
            let p = d * t;
            let dist = length(p - c) / rr;
            if t > 0.0 && dist > b.ring_inner && dist < b.ring_outer && (t < planet_t || covered < 1.0) {
                let u = (dist - b.ring_inner) / (b.ring_outer - b.ring_inner);
                let stripe = 0.55 + 0.45 * step(0.35, hash1(floor(u * 18.0) + b.seed));
                let gap = smoothstep(0.0, 0.04, abs(u - 0.62)) * smoothstep(0.0, 0.05, u) * (1.0 - smoothstep(0.95, 1.0, u));
                // In the planet's shadow?
                let to_c = c - p;
                let along = dot(to_c, sun.toward);
                let off = length(to_c - sun.toward * along);
                let shadowed = select(1.0, 0.15, along > 0.0 && off < rr);
                let alpha = 0.8 * stripe * gap;
                let ring = b.ring_color * sun.color * BODY_LIGHT * 0.9 * shadowed;
                let behind = t > planet_t;
                let a = select(alpha, alpha * (1.0 - covered), behind);
                // Over (or under) the disc.
                out = vec4(ring * a + out.rgb * (1.0 - a), a + out.a * (1.0 - a));
            }
        }
    }
    return out;
}

// The sun's disc and corona along `d`: premultiplied colour and coverage.
fn sun_shade(sun: SkySun, d: vec3<f32>, aa: f32) -> vec4<f32> {
    let ang = acos(clamp(dot(d, sun.toward), -1.0, 1.0));
    let x = ang / sun.radius;
    let disc = clamp((1.0 - x) * sun.radius / max(aa, 1e-5) + 0.5, 0.0, 1.0);
    var light = sun.color * SUN_DISC * disc;
    if sun.corona > 0.0 {
        let c = clamp((x - 1.0) / sun.corona, 0.0, 1.0);
        // Rings of corona, stepped.
        let glow = tones(pow(1.0 - c, 2.0), 3.0, 0.08, 0.0) * step(1.0, x);
        light += sun.color * 1.6 * glow * (1.0 - disc);
    }
    // A wide soft glow, out to several times the corona.
    // (Faded out before its quad's widest reach, 1.45 rad: a big sun's
    // glow would otherwise end in a square.)
    let wide = exp(-max(x - 1.0, 0.0) / (1.5 + 2.0 * sun.corona)) * step(1.0, x) * (1.0 - smoothstep(1.0, 1.4, ang));
    light += sun.color * SUN_GLOW * wide * (1.0 - disc);
    return vec4(light, disc);
}

// Every body along `d`, over each other (for the light cubemap).
fn bodies(sky: Sky, d: vec3<f32>, aa: f32) -> vec4<f32> {
    var out = vec4(0.0);
    for (var i = 0u; i < sky.counts.y; i++) {
        let b = body_shade(sky.bodies[i], sky.sun, d, aa);
        out = vec4(b.rgb + out.rgb * (1.0 - b.a), b.a + out.a * (1.0 - b.a));
    }
    return out;
}
