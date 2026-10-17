//! The sky, drawn: whatever [`Ambience`] the authority put the world under
//! (or the reference sky, until one arrives), in four layers, each drawn the
//! way it needs (docs/plan-ambience.md):
//!
//! 1. **The backdrop** (band, dust, nebulae): baked on the GPU into a
//!    cubemap whenever the sky changes ([`bake`]), shown by Bevy's
//!    `Skybox`.
//! 2. **The stars**: small quads at infinity, sized in pixels ([`stars`]).
//! 3. **The bodies and the sun's disc**: impostors at infinity, shaded per
//!    pixel so their edges stay sharp ([`bodies`]).
//! 4. **The light**: the sun's direction, colour and strength; space's
//!    ambient light; and the fill, the cameras' environment light,
//!    filtered from a second baked cubemap that has the bodies in it too,
//!    so a gas giant lights you in its colours. The shaders share one
//!    module (`sky.wgsl`), so the light is the sky you see.
//!
//! Inside a ship the sky's light would come through the walls; a dim
//! uniform light replaces it there ([`interior_light`]), and
//! `devices::interior_ambient` turns space's own down by what it reads in
//! [`SpaceLevels`].

pub mod bake;
pub mod bodies;
pub mod stars;

use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::image::Image;
use bevy::light::{EnvironmentMapLight, GeneratedEnvironmentMapLight, LightProbe, Skybox};
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, ShaderType, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
use bevy::shader::Shader;

use crate::sim::ambience::{Ambience, BodyKind, MAX_BODIES, MAX_NEBULAE};
use bake::SkyBake;
use bodies::{BodyMaterial, SUN_SLOT};
use stars::StarMaterial;

const SKY_SHADER: Handle<Shader> = uuid_handle!("2b7f4c51-9a0e-4d8b-b3a6-5c1e7f2d9a40");
pub(crate) const BAKE_SHADER: Handle<Shader> = uuid_handle!("8e3a1d27-4f6b-4c09-a2d5-71b9e0c4f3a8");
pub(crate) const STAR_SHADER: Handle<Shader> = uuid_handle!("c4d9e2f1-7a3b-4e58-9b0c-2d6f8a1e5b73");
pub(crate) const BODY_SHADER: Handle<Shader> = uuid_handle!("5a0f3e8c-2d71-4b96-8e4a-9c3b7d1f6e02");

/// cd/m² per sky unit: how bright the sky is drawn.
pub const SKY_BRIGHTNESS: f32 = 500.0;

/// Fill light inside a ship (cd/m²), with an even warm grey surrounding.
const INTERIOR_FILL: f32 = 150.0;
const INTERIOR_COLOR: [u8; 4] = [150, 140, 125, 255];

pub struct AmbiencePlugin;

impl Plugin for AmbiencePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SKY_SHADER, "sky.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, BAKE_SHADER, "bake.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, STAR_SHADER, "stars.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, BODY_SHADER, "bodies.wgsl", Shader::from_wgsl);
        app.add_plugins((
            bake::SkyBakePlugin,
            MaterialPlugin::<StarMaterial>::default(),
            MaterialPlugin::<BodyMaterial>::default(),
        ))
        .init_resource::<LayersOff>()
        .add_systems(PreStartup, setup_sky)
        .add_observer(add_sky_to_cameras)
        .add_systems(
            Update,
            (
                follow_ambience,
                (light_the_place, draw_stars, draw_bodies, ask_for_bake).run_if(resource_changed::<ShownSky>),
                refilter_when_baked,
                settle_environment_maps,
            )
                .chain(),
        );
    }
}

/// The sky being drawn, and how many times it's changed.
#[derive(Resource)]
pub struct ShownSky {
    pub sky: Ambience,
    pub generation: u32,
}

/// The sky's cubemaps, shared by every camera, and the dim light that
/// replaces the sky's inside ships.
#[derive(Resource, Clone)]
pub struct SkyMaps {
    pub display: Handle<Image>,
    pub light: Handle<Image>,
    pub interior: Handle<Image>,
}

/// Space's own light, as the sky sets it, for `devices::interior_ambient`
/// to turn down inside dark ships.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct SpaceLevels {
    /// The global ambient light's brightness.
    pub ambient: f32,
    /// The cameras' environment light (cd/m² per sky unit).
    pub fill: f32,
}

/// Layers this peer leaves out whatever the sky says (profiling switches).
#[derive(Resource, Default, Clone, Copy, PartialEq)]
pub struct LayersOff {
    pub stars: bool,
    pub bodies: bool,
}

/// The lights the sun is: the world's and the hands' (see
/// `render::setup_lighting`).
#[derive(Component)]
pub struct SunLight;

/// A companion star's light (unshadowed), there while the sky has it.
#[derive(Component)]
struct CompanionLight;

/// The sky's own entities: the stars' mesh, and an impostor per body slot
/// and the sun's.
#[derive(Resource, Default)]
struct SkyEntities {
    stars: Option<(Entity, Handle<StarMaterial>)>,
    /// What the stars were grown from, to grow them again only when it
    /// changes.
    grown_from: Option<(crate::sim::ambience::Stars, Option<crate::sim::ambience::Band>)>,
    bodies: [Option<(Entity, Handle<BodyMaterial>)>; MAX_BODIES + 1],
}

fn setup_sky(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let maps = SkyMaps {
        display: images.add(bake::display_cubemap()),
        light: images.add(bake::light_cubemap()),
        interior: images.add(uniform_cubemap(32, INTERIOR_COLOR)),
    };
    let sky = Ambience::reference();
    commands.insert_resource(SpaceLevels {
        ambient: sky.light.ambient_brightness,
        fill: sky.light.fill,
    });
    commands.insert_resource(SkyBake {
        display: maps.display.clone(),
        light: maps.light.clone(),
        sky: sky_uniform(&sky),
        generation: 0,
        baked: default(),
    });
    commands.insert_resource(ShownSky { sky, generation: 1 });
    commands.insert_resource(maps);
    commands.init_resource::<SkyEntities>();
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
pub fn interior_light(maps: &SkyMaps, center: Vec3, size: Vec3) -> impl Bundle {
    (
        LightProbe {
            falloff: Vec3::splat(0.02),
        },
        GeneratedEnvironmentMapLight {
            environment_map: maps.interior.clone(),
            intensity: INTERIOR_FILL,
            ..default()
        },
        Transform::from_translation(center).with_scale(size),
    )
}

/// Gives every 3D camera the sky: the view-model camera only its light,
/// since it draws over the world, sky and all.
pub fn add_sky_to_cameras(
    add: On<Add, Camera3d>,
    maps: Option<Res<SkyMaps>>,
    levels: Option<Res<SpaceLevels>>,
    view_model: Query<(), With<super::camera::ViewModelCamera>>,
    mut commands: Commands,
) {
    let (Some(maps), Some(levels)) = (maps, levels) else { return };
    let light = GeneratedEnvironmentMapLight {
        environment_map: maps.light.clone(),
        intensity: levels.fill,
        ..default()
    };
    if view_model.contains(add.entity) {
        commands.entity(add.entity).insert(light);
    } else {
        commands.entity(add.entity).insert((
            Skybox {
                image: Some(maps.display.clone()),
                brightness: SKY_BRIGHTNESS,
                ..default()
            },
            light,
        ));
    }
}

/// Follows the authority's sky (there's one at a time, cleared with its
/// scenario's world), with this peer's own leavings-out.
fn follow_ambience(skies: Query<Ref<Ambience>>, off: Res<LayersOff>, mut shown: ResMut<ShownSky>) {
    let changed = skies.iter().any(|s| s.is_changed());
    if !changed && !off.is_changed() {
        return;
    }
    // One at a time, but a scenario's switch can show the old and the new
    // together for a frame: the new one is the one just changed.
    let Some(sky) = skies.iter().filter(|s| s.is_changed()).last().or_else(|| skies.iter().last()) else { return };
    let mut sky: Ambience = (*sky).clone();
    sky.show.stars &= !off.stars;
    sky.show.bodies &= !off.bodies;
    if sky != shown.sky {
        shown.sky = sky;
        shown.generation += 1;
    }
}

/// The sun's light, space's ambient light and the fill, from the sky.
fn light_the_place(
    shown: Res<ShownSky>,
    mut suns: Query<(&mut DirectionalLight, &mut Transform), With<SunLight>>,
    companions: Query<Entity, With<CompanionLight>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut levels: ResMut<SpaceLevels>,
    mut commands: Commands,
) {
    let sky = &shown.sky;
    let color = |c: crate::sim::ambience::Rgb| Color::srgb_u8(c.0[0], c.0[1], c.0[2]);
    let facing = |toward: Vec3| {
        let up = if toward.y.abs() > 0.98 { Vec3::Z } else { Vec3::Y };
        Transform::default().looking_to(-toward, up)
    };
    for (mut light, mut transform) in &mut suns {
        light.illuminance = sky.sun.illuminance;
        light.color = color(sky.sun.color);
        *transform = facing(sky.sun_toward());
    }
    for companion in &companions {
        commands.entity(companion).despawn();
    }
    if sky.show.bodies {
        for body in sky.bodies.iter().filter(|b| b.kind == BodyKind::Star && b.light > 0.0) {
            commands.spawn((
                Name::new("Companion star"),
                CompanionLight,
                DirectionalLight {
                    illuminance: body.light,
                    color: color(body.colors[0]),
                    shadow_maps_enabled: false,
                    ..default()
                },
                facing(body.direction.normalize_or(Vec3::Y)),
            ));
        }
    }
    ambient.color = color(sky.light.ambient);
    let wanted = SpaceLevels {
        ambient: sky.light.ambient_brightness,
        fill: sky.light.fill,
    };
    if *levels != wanted {
        *levels = wanted;
    }
}

/// The stars, grown again when what they're grown from changes; the rest
/// of the sky (what hides them) goes to their material.
fn draw_stars(
    shown: Res<ShownSky>,
    mut entities: ResMut<SkyEntities>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StarMaterial>>,
    mut commands: Commands,
) {
    let sky = &shown.sky;
    let entities = &mut *entities;
    if !sky.show.stars {
        if let Some((entity, _)) = entities.stars.take() {
            commands.entity(entity).despawn();
        }
        entities.grown_from = None;
        return;
    }
    let uniform = sky_uniform(sky);
    let from = (sky.stars.clone(), sky.band.clone());
    if entities.grown_from.as_ref() != Some(&from) || entities.stars.is_none() {
        if let Some((entity, _)) = entities.stars.take() {
            commands.entity(entity).despawn();
        }
        let catalogue = stars::catalogue(&sky.stars, sky.band.as_ref());
        let material = materials.add(StarMaterial {
            sky: uniform,
            params: Vec4::new(SKY_BRIGHTNESS, 0.0, 0.0, 0.0),
        });
        let entity = commands
            .spawn((
                Name::new("Stars"),
                Mesh3d(meshes.add(stars::mesh(&catalogue))),
                MeshMaterial3d(material.clone()),
                Transform::default(),
                NoFrustumCulling,
                RenderLayers::layer(0),
                bevy::light::NotShadowCaster,
            ))
            .id();
        entities.stars = Some((entity, material));
        entities.grown_from = Some(from);
    } else if let Some((_, material)) = &entities.stars
        && let Some(mut material) = materials.get_mut(material)
    {
        material.sky = uniform;
    }
}

/// An impostor for each body shown, and the sun's disc.
fn draw_bodies(
    shown: Res<ShownSky>,
    mut entities: ResMut<SkyEntities>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<BodyMaterial>>,
    mut quad: Local<Option<Handle<Mesh>>>,
    mut commands: Commands,
) {
    let sky = &shown.sky;
    let uniform = sky_uniform(sky);
    let quad = quad.get_or_insert_with(|| meshes.add(Rectangle::new(2.0, 2.0))).clone();
    for slot in 0..=MAX_BODIES {
        let wanted = if slot == SUN_SLOT {
            sky.show.sun
        } else {
            sky.show.bodies && slot < sky.bodies.len()
        };
        match (&entities.bodies[slot], wanted) {
            (Some((_, material)), true) => {
                if let Some(mut material) = materials.get_mut(material) {
                    material.sky = uniform;
                }
            }
            (None, true) => {
                let material = materials.add(BodyMaterial {
                    sky: uniform,
                    params: Vec4::new(SKY_BRIGHTNESS, slot as f32, 0.0, 0.0),
                });
                let entity = commands
                    .spawn((
                        Name::new(if slot == SUN_SLOT { "Sun's disc".to_string() } else { format!("Sky body {slot}") }),
                        Mesh3d(quad.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::default(),
                        NoFrustumCulling,
                        RenderLayers::layer(0),
                        bevy::light::NotShadowCaster,
                    ))
                    .id();
                entities.bodies[slot] = Some((entity, material));
            }
            (Some((entity, _)), false) => {
                commands.entity(*entity).despawn();
                entities.bodies[slot] = None;
            }
            (None, false) => {}
        }
    }
}

/// Asks the render world to bake the sky's cubemaps again.
fn ask_for_bake(shown: Res<ShownSky>, mut bake: ResMut<SkyBake>) {
    bake.sky = sky_uniform(&shown.sky);
    bake.generation = shown.generation;
}

/// Frames a generated environment map is left to filter before it's kept
/// as it is.
const FILTER_FRAMES: u32 = 30;

/// How many frames a generated environment map has had to filter.
#[derive(Component)]
pub struct Filtering(u32);

/// Once a new sky's light cubemap is baked, every camera's environment
/// light is filtered from it again (for a while: see
/// [`settle_environment_maps`]).
fn refilter_when_baked(
    bake: Res<SkyBake>,
    maps: Res<SkyMaps>,
    levels: Res<SpaceLevels>,
    cameras: Query<(Entity, Option<&EnvironmentMapLight>), (With<Camera3d>, Without<LightProbe>)>,
    mut filtered: Local<u32>,
    mut commands: Commands,
) {
    let baked = bake.baked();
    if baked == *filtered || baked != bake.generation {
        return;
    }
    *filtered = baked;
    for (camera, map) in &cameras {
        commands.entity(camera).insert((
            GeneratedEnvironmentMapLight {
                environment_map: maps.light.clone(),
                intensity: map.map_or(levels.fill, |m| m.intensity),
                ..default()
            },
            Filtering(0),
        ));
    }
}

/// Bevy re-filters a generated environment map every frame (about 4 ms of
/// GPU for the sky and a ship's cabin together, measured). A sky only
/// changes when it's baked again, so once one has been filtered, its
/// generator is removed and the light it made stays (until the next bake:
/// [`refilter_when_baked`]).
pub fn settle_environment_maps(
    mut commands: Commands,
    mut maps: Query<(Entity, Option<&mut Filtering>), (With<GeneratedEnvironmentMapLight>, With<EnvironmentMapLight>)>,
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

// ---------------------------------------------------------------------------
// The sky as the shaders take it (`sky.wgsl`'s `Sky`).

#[derive(Clone, Copy, Default, ShaderType)]
pub struct SunUniform {
    toward: Vec3,
    radius: f32,
    color: Vec3,
    corona: f32,
}

#[derive(Clone, Copy, Default, ShaderType)]
pub struct BandUniform {
    pole: Vec3,
    width: f32,
    bulge: Vec3,
    dust: f32,
    color: Vec3,
    tones: f32,
    bulge_color: Vec3,
    seed: f32,
}

#[derive(Clone, Copy, Default, ShaderType)]
pub struct NebulaUniform {
    direction: Vec3,
    radius: f32,
    inner: Vec3,
    tones: f32,
    outer: Vec3,
    dust: f32,
    axis: Vec3,
    stretch: f32,
    seed: f32,
    rim: f32,
    pad: Vec2,
}

#[derive(Clone, Copy, Default, ShaderType)]
pub struct BodyUniform {
    direction: Vec3,
    radius: f32,
    color_a: Vec3,
    kind: f32,
    color_b: Vec3,
    seed: f32,
    color_c: Vec3,
    colors: f32,
    axis: Vec3,
    ring_inner: f32,
    ring_color: Vec3,
    ring_outer: f32,
    atmosphere: Vec3,
    air: f32,
}

#[derive(Clone, Copy, Default, ShaderType)]
pub struct SkyUniform {
    sun: SunUniform,
    band: BandUniform,
    nebulae: [NebulaUniform; MAX_NEBULAE],
    bodies: [BodyUniform; MAX_BODIES],
    /// Nebulae shown, bodies shown, band shown (0/1), the sun's disc shown
    /// (0/1).
    counts: UVec4,
}

/// A seed as a noise offset: big enough to move well away, small enough to
/// keep a float's precision.
fn seed_offset(seed: u32) -> f32 {
    (seed % 997) as f32 * 1.618
}

/// A tangent at `toward`, turned `degrees` round it.
fn tangent(toward: Vec3, degrees: f32) -> Vec3 {
    let up = if toward.y.abs() > 0.95 { Vec3::X } else { Vec3::Y };
    let t1 = up.cross(toward).normalize();
    let t2 = toward.cross(t1);
    let a = degrees.to_radians();
    t1 * a.cos() + t2 * a.sin()
}

/// The sky, packed for the shaders: unit directions, angles in radians,
/// colours linear and times their brightness, only the layers shown.
pub fn sky_uniform(sky: &Ambience) -> SkyUniform {
    let half_rad = |degrees: f32| (degrees / 2.0).to_radians();
    let mut u = SkyUniform {
        sun: SunUniform {
            toward: sky.sun_toward(),
            radius: half_rad(sky.sun.size),
            color: sky.sun.color.linear(),
            corona: sky.sun.corona,
        },
        ..default()
    };
    let mut band_shown = 0;
    if let Some(band) = sky.band.as_ref().filter(|_| sky.show.band) {
        let pole = band.pole.normalize_or(Vec3::Y);
        u.band = BandUniform {
            pole,
            width: half_rad(band.width),
            bulge: (band.bulge - pole * band.bulge.dot(pole)).normalize_or(pole.any_orthonormal_vector()),
            dust: band.dust,
            color: band.color.linear() * band.brightness,
            tones: band.tones.clamp(1, 8) as f32,
            bulge_color: band.bulge_color.linear() * band.brightness,
            seed: seed_offset(band.seed),
        };
        band_shown = 1;
    }
    let mut nebulae = 0;
    if sky.show.nebulae {
        for (slot, n) in u.nebulae.iter_mut().zip(&sky.nebulae) {
            let direction = n.direction.normalize_or(Vec3::Z);
            *slot = NebulaUniform {
                direction,
                radius: half_rad(n.size),
                inner: n.inner.linear() * n.brightness,
                tones: n.tones.clamp(1, 8) as f32,
                outer: n.outer.linear() * n.brightness,
                dust: n.dust,
                axis: tangent(direction, n.turn),
                stretch: n.stretch.max(1.0),
                seed: seed_offset(n.seed),
                rim: n.rim,
                pad: Vec2::ZERO,
            };
            nebulae += 1;
        }
    }
    let mut bodies = 0;
    if sky.show.bodies {
        for (slot, b) in u.bodies.iter_mut().zip(&sky.bodies) {
            let colour = |i: usize| b.colors.get(i).or(b.colors.first()).map_or(Vec3::splat(0.5), |c| c.linear());
            let (ring_inner, ring_outer, ring_color) = b.rings.as_ref().map_or((0.0, 0.0, Vec3::ZERO), |r| (r.inner, r.outer, r.color.linear()));
            *slot = BodyUniform {
                direction: b.direction.normalize_or(Vec3::X),
                radius: half_rad(b.size),
                color_a: colour(0),
                kind: match b.kind {
                    BodyKind::Rocky => 0.0,
                    BodyKind::GasGiant => 1.0,
                    BodyKind::Ice => 2.0,
                    BodyKind::Star => 3.0,
                },
                color_b: colour(1),
                seed: seed_offset(b.seed),
                color_c: colour(2),
                colors: b.colors.len().clamp(1, 3) as f32,
                axis: b.axis.normalize_or(Vec3::Y),
                ring_inner,
                ring_color,
                ring_outer,
                atmosphere: b.atmosphere.map_or(Vec3::ZERO, |c| c.linear()),
                air: if b.atmosphere.is_some() { 1.0 } else { 0.0 },
            };
            bodies += 1;
        }
    }
    u.counts = UVec4::new(nebulae, bodies, band_shown, sky.show.sun as u32);
    u
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_layers_are_left_out_of_the_shaders_sky() {
        let mut sky = Ambience::named("boarding").unwrap();
        let all = sky_uniform(&sky);
        assert_eq!(all.counts, UVec4::new(0, 2, 1, 1));
        sky.show.band = false;
        sky.show.bodies = false;
        sky.show.sun = false;
        assert_eq!(sky_uniform(&sky).counts, UVec4::new(0, 0, 0, 0));
        // Colours go in linear and times their brightness.
        let band = Ambience::named("boarding").unwrap().band.unwrap();
        assert!((all.band.color - band.color.linear() * band.brightness).length() < 1e-6);
        assert!((all.sun.toward.length() - 1.0).abs() < 1e-6);
    }
}
