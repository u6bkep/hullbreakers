//! The sky: stars and a nebula band on a cubemap, which is both what you see
//! behind everything and (filtered on the GPU) the light that fills what the
//! sun doesn't reach. So shadowed sides are lit by the sky you can see:
//! dimly, coloured like the nebula, and metal reflects it.

use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::light::{GeneratedEnvironmentMapLight, LightProbe, Skybox};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

/// Where sunlight comes from (toward the sun). See `render::setup_lighting`.
pub const SUN_DIRECTION: Vec3 = Vec3::new(0.4, 0.5, -0.3);

/// Cubemap face size (pixels). Stars are single pixels, so this sets how
/// fine they are.
const FACE: u32 = 1024;
/// The nebula is smooth; it's computed at this size and upsampled.
const NEBULA_FACE: u32 = 128;
/// Brightness of the visible sky, and of the fill light it gives (cd/m²).
pub const SKY_BRIGHTNESS: f32 = 500.0;
pub const FILL_INTENSITY: f32 = 3500.0;

/// The sky cubemap, shared by every camera, and the dim light that replaces
/// it inside ships (the sky's light would otherwise come through the walls).
#[derive(Resource, Clone)]
pub struct Sky {
    pub sky: Handle<Image>,
    pub interior: Handle<Image>,
}

/// Fill light inside a ship (cd/m²), with an even warm grey surrounding.
const INTERIOR_FILL: f32 = 150.0;
const INTERIOR_COLOR: [u8; 4] = [150, 140, 125, 255];

impl Sky {
    /// What a camera needs to see the sky and be lit by it.
    pub fn camera_components(&self) -> (Skybox, GeneratedEnvironmentMapLight) {
        (
            Skybox {
                image: Some(self.sky.clone()),
                brightness: SKY_BRIGHTNESS,
                ..default()
            },
            GeneratedEnvironmentMapLight {
                environment_map: self.sky.clone(),
                intensity: FILL_INTENSITY,
                ..default()
            },
        )
    }
}

/// The direction through pixel (`x`, `y`) of cubemap `face` (+X, −X, +Y, −Y,
/// +Z, −Z), `size` pixels square.
fn face_direction(face: usize, x: f32, y: f32, size: f32) -> Vec3 {
    let u = 2.0 * (x + 0.5) / size - 1.0;
    let v = 2.0 * (y + 0.5) / size - 1.0;
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
    .normalize()
}

/// The face and pixel coordinates (continuous, 0..size) a direction falls on.
fn direction_face(dir: Vec3, size: f32) -> (usize, f32, f32) {
    let a = dir.abs();
    let (face, u, v, major) = if a.x >= a.y && a.x >= a.z {
        if dir.x > 0.0 {
            (0, -dir.z, -dir.y, a.x)
        } else {
            (1, dir.z, -dir.y, a.x)
        }
    } else if a.y >= a.z {
        if dir.y > 0.0 {
            (2, dir.x, dir.z, a.y)
        } else {
            (3, dir.x, -dir.z, a.y)
        }
    } else if dir.z > 0.0 {
        (4, dir.x, -dir.y, a.z)
    } else {
        (5, -dir.x, -dir.y, a.z)
    };
    let to_pixel = |t: f32| (t / major + 1.0) / 2.0 * size;
    (face, to_pixel(u), to_pixel(v))
}

/// The nebula's colour (linear) in direction `dir`: a broad band across
/// the sky, clumped by noise, teal shading to violet. The band runs through
/// the sun and the point opposite it, so sides facing away from the sun
/// still see some of it.
fn nebula(dir: Vec3, clumps: &Fbm<Perlin>, tint: &Fbm<Perlin>) -> Vec3 {
    let sun = SUN_DIRECTION.normalize();
    let normal = sun.cross(Vec3::new(0.3, 0.2, 1.0)).normalize();
    let off_band = dir.dot(normal);
    let band = (-(off_band / 0.38).powi(2)).exp();
    let p = (dir * 2.2).as_dvec3().to_array();
    let density = ((clumps.get(p) as f32 * 0.5 + 0.55).clamp(0.0, 1.0)).powf(1.6);
    let mix = (tint.get((dir * 1.3).as_dvec3().to_array()) as f32 * 0.8 + 0.5).clamp(0.0, 1.0);
    let teal = Vec3::new(0.10, 0.32, 0.40);
    let violet = Vec3::new(0.30, 0.14, 0.38);
    let haze = Vec3::new(0.004, 0.005, 0.008);
    haze + teal.lerp(violet, mix) * band * density
}

/// Builds the sky cubemap.
pub fn sky_image() -> Image {
    let clumps = Fbm::<Perlin>::new(11).set_octaves(5);
    let tint = Fbm::<Perlin>::new(12).set_octaves(2);
    // The nebula, coarse.
    let n = NEBULA_FACE as usize;
    let coarse: Vec<Vec3> = (0..6 * n * n)
        .map(|i| {
            let (face, rest) = (i / (n * n), i % (n * n));
            let dir = face_direction(face, (rest % n) as f32, (rest / n) as f32, n as f32);
            nebula(dir, &clumps, &tint)
        })
        .collect();
    let sample = |face: usize, x: f32, y: f32| {
        // Bilinear within a face; clamped at its edges.
        let fx = (x - 0.5).clamp(0.0, (n - 1) as f32);
        let fy = (y - 0.5).clamp(0.0, (n - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(n - 1), (y0 + 1).min(n - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let at = |x: usize, y: usize| coarse[face * n * n + y * n + x];
        at(x0, y0)
            .lerp(at(x1, y0), tx)
            .lerp(at(x0, y1).lerp(at(x1, y1), tx), ty)
    };
    let size = FACE as usize;
    let scale = NEBULA_FACE as f32 / FACE as f32;
    let mut sky = vec![Vec3::ZERO; 6 * size * size];
    let mut rng = ChaCha8Rng::seed_from_u64(7);
    for (i, px) in sky.iter_mut().enumerate() {
        let (face, rest) = (i / (size * size), i % (size * size));
        let (x, y) = ((rest % size) as f32 + 0.5, (rest / size) as f32 + 0.5);
        // A little noise against banding in 8 bits.
        let dither = (rng.random::<f32>() - 0.5) / 255.0;
        *px = sample(face, x * scale, y * scale) + Vec3::splat(dither);
    }
    // Stars, uniform over the sphere, a few of them coloured.
    for _ in 0..9_000 {
        let z: f32 = 2.0 * rng.random::<f32>() - 1.0;
        let a: f32 = std::f32::consts::TAU * rng.random::<f32>();
        let r = (1.0 - z * z).sqrt();
        let dir = Vec3::new(r * a.cos(), z, r * a.sin());
        let (face, x, y) = direction_face(dir, FACE as f32);
        let (x, y) = ((x as usize).min(size - 1), (y as usize).min(size - 1));
        let brightness = 0.15 + 0.85 * rng.random::<f32>().powi(5);
        let tint = match rng.random_range(0..5) {
            0 => Vec3::new(0.75, 0.82, 1.0),
            1 => Vec3::new(1.0, 0.85, 0.65),
            _ => Vec3::ONE,
        };
        let px = &mut sky[face * size * size + y * size + x];
        *px = px.max(tint * brightness);
    }
    let mut data = Vec::with_capacity(sky.len() * 4);
    for px in sky {
        let srgb = Color::linear_rgb(px.x, px.y, px.z).to_srgba();
        data.extend(
            [srgb.red, srgb.green, srgb.blue].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
        );
        data.push(255);
    }
    let mut image = Image::new(
        Extent3d {
            width: FACE,
            height: FACE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

/// A cubemap of one colour, `size` pixels square.
fn uniform_cubemap(size: u32, color: [u8; 4]) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        &color,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image
}

/// The light inside a ship: a probe filling its cabin (center and size in
/// ship coordinates), which replaces the sky's light there.
pub fn interior_light(sky: &Sky, center: Vec3, size: Vec3) -> impl Bundle {
    (
        LightProbe {
            falloff: Vec3::splat(0.02),
        },
        GeneratedEnvironmentMapLight {
            environment_map: sky.interior.clone(),
            intensity: INTERIOR_FILL,
            ..default()
        },
        Transform::from_translation(center).with_scale(size),
    )
}

pub fn setup_sky(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.insert_resource(Sky {
        sky: images.add(sky_image()),
        interior: images.add(uniform_cubemap(32, INTERIOR_COLOR)),
    });
}

/// Gives every 3D camera the sky.
pub fn add_sky_to_cameras(add: On<Add, Camera3d>, sky: Option<Res<Sky>>, mut commands: Commands) {
    if let Some(sky) = sky {
        commands.entity(add.entity).insert(sky.camera_components());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_lookup_inverts_face_directions() {
        let size = 64.0;
        for face in 0..6 {
            for (x, y) in [(0.0, 0.0), (10.0, 50.0), (63.0, 31.0)] {
                let dir = face_direction(face, x, y, size);
                let (f, px, py) = direction_face(dir, size);
                assert_eq!(f, face);
                assert!(
                    (px - (x + 0.5)).abs() < 1e-3 && (py - (y + 0.5)).abs() < 1e-3,
                    "{face} {x} {y}: {px} {py}"
                );
            }
        }
    }
}

/// Frames a generated environment map is left to filter before it's kept
/// as it is.
const FILTER_FRAMES: u32 = 30;

/// How many frames a generated environment map has had to filter.
#[derive(Component)]
pub struct Filtering(u32);

/// Bevy re-filters a generated environment map every frame (about 4 ms of
/// GPU for the sky and a ship's cabin together, measured). Ours never
/// change, so once one has been filtered, its generator is removed and the
/// light it made stays.
pub fn settle_environment_maps(
    mut commands: Commands,
    mut maps: Query<
        (Entity, Option<&mut Filtering>),
        (With<GeneratedEnvironmentMapLight>, With<bevy::light::EnvironmentMapLight>),
    >,
) {
    for (entity, filtering) in &mut maps {
        match filtering {
            None => {
                commands.entity(entity).insert(Filtering(0));
            }
            Some(mut f) if f.0 < FILTER_FRAMES => f.0 += 1,
            Some(_) => {
                commands.entity(entity).remove::<(GeneratedEnvironmentMapLight, Filtering)>();
            }
        }
    }
}
