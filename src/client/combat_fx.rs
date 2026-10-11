//! Ship combat, presented: laser beams and where they land, shield bubbles,
//! turrets that track their target, the buttons fitted to ships, alert
//! lighting, and ships blowing up.
//!
//! Everything here is driven by state every peer has (each ship's
//! [`Combatant`], the [`Beams`] the sim traces each tick, the turret and
//! control children every hull builds), so a network client sees what the
//! host sees.
//!
//! Beams are traced in the fixed tick from simulated poses, but ships are
//! drawn elsewhere (a host draws them interpolated a tick behind; a client at
//! its draw clock). So a beam is kept as its two ends in the two ships' own
//! frames and placed through their drawn transforms each frame; otherwise it
//! would visibly come loose from the turret at combat speeds.

use avian3d::prelude::*;
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::image::{Image, ImageSampler};
use bevy::light::NotShadowCaster;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::transform::TransformSystems;

use super::camera::{self, PlayerCamera};
use super::render::Avatar;
use crate::sim::bots::{BodyTuning, Robot};
use crate::sim::vitals::Vitals;
use super::render::ShipLight;
use crate::sim::combat::{Beams, Combatant};
use crate::sim::ship::{Control, ControlAction, HomeShip, ShipHull, Turret};

pub struct CombatFxPlugin;

impl Plugin for CombatFxPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHIELD_SHADER, "shield.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<ShieldMaterial>::default())
            .init_resource::<BeamTracks>()
            .init_resource::<BeamPool>()
            .add_systems(Startup, setup_fx)
            .add_observer(explode)
            .add_observer(robot_destroyed)
            .add_systems(
                Update,
                (
                    fit_turrets,
                    fit_buttons,
                    fit_shields,
                    pulse_buttons,
                    animate_explosions,
                    fade_robot_flashes,
                    animate_sparks,
                    cool_scorches,
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    track_beams,
                    (
                        draw_beams,
                        aim_turrets,
                        draw_shields,
                        alert_lights,
                        hull_hits,
                    ),
                )
                    .chain()
                    // Ships (and the camera) are where they're drawn this frame.
                    .after(camera::update_camera)
                    .before(TransformSystems::Propagate),
            );
    }
}

const SHIELD_SHADER: Handle<Shader> = uuid_handle!("6d1f3c0e-6a43-4c2e-9a9b-3f0b8f61c2d4");

/// Which side fired: ours or theirs. Beams and turret trim are colored by it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Ours,
    Theirs,
}

impl Side {
    fn of(home: bool) -> Self {
        if home { Side::Ours } else { Side::Theirs }
    }

    fn index(self) -> usize {
        self as usize
    }

    /// The beam's hue (linear, unit brightness).
    fn color(self) -> LinearRgba {
        match self {
            Side::Ours => LinearRgba::rgb(0.25, 0.75, 1.0),
            Side::Theirs => LinearRgba::rgb(1.0, 0.28, 0.06),
        }
    }
}

fn scaled(c: LinearRgba, k: f32) -> LinearRgba {
    LinearRgba::rgb(c.red * k, c.green * k, c.blue * k)
}

/// An unlit, additive glow: adds light, never darkens or hides.
fn glow(c: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        base_color: c.into(),
        unlit: true,
        alpha_mode: AlphaMode::Add,
        ..default()
    }
}

#[derive(Resource)]
struct FxAssets {
    /// Radius 1, height 1, along Y.
    cylinder: Handle<Mesh>,
    sphere: Handle<Mesh>,
    cube: Handle<Mesh>,
    ring: Handle<Mesh>,
    beam_core: [Handle<StandardMaterial>; 2],
    beam_glow: [Handle<StandardMaterial>; 2],
    shield_splash: [Handle<StandardMaterial>; 2],
    hull_splash: Handle<StandardMaterial>,
    spark: Handle<StandardMaterial>,
    /// Scorch marks, from white hot to cold.
    scorch: [Handle<StandardMaterial>; 4],
    turret: Handle<StandardMaterial>,
    turret_trim: [Handle<StandardMaterial>; 2],
}

fn setup_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let sides = [Side::Ours, Side::Theirs];
    let mut each = |f: &dyn Fn(Side) -> StandardMaterial| sides.map(|s| materials.add(f(s)));
    let beam_core = each(&|s| StandardMaterial {
        base_color: (scaled(s.color(), 6.0) + LinearRgba::rgb(0.6, 0.6, 0.6)).into(),
        unlit: true,
        ..default()
    });
    let beam_glow = each(&|s| glow(scaled(s.color(), 0.5)));
    let shield_splash = each(&|s| glow(scaled(s.color(), 1.5)));
    let turret_trim = each(&|s| StandardMaterial {
        base_color: s.color().into(),
        emissive: scaled(s.color(), 3.0),
        ..default()
    });
    let hot = |k: f32, c: LinearRgba| StandardMaterial {
        base_color: Color::srgb(0.05, 0.04, 0.04),
        emissive: scaled(c, k),
        perceptual_roughness: 1.0,
        ..default()
    };
    commands.insert_resource(FxAssets {
        cylinder: meshes.add(Cylinder::new(1.0, 1.0)),
        sphere: meshes.add(Sphere::new(1.0).mesh().ico(4).unwrap()),
        cube: meshes.add(Cuboid::from_length(1.0)),
        ring: meshes.add(Torus::new(0.988, 1.0)),
        beam_core,
        beam_glow,
        shield_splash,
        hull_splash: materials.add(glow(LinearRgba::rgb(6.0, 3.5, 1.5))),
        spark: materials.add(glow(LinearRgba::rgb(8.0, 4.0, 1.2))),
        scorch: [
            materials.add(hot(12.0, LinearRgba::rgb(1.0, 0.8, 0.5))),
            materials.add(hot(6.0, LinearRgba::rgb(1.0, 0.35, 0.05))),
            materials.add(hot(1.5, LinearRgba::rgb(0.8, 0.12, 0.02))),
            materials.add(hot(0.0, LinearRgba::BLACK)),
        ],
        turret: materials.add(StandardMaterial {
            base_color: Color::srgb(0.22, 0.23, 0.25),
            metallic: 0.8,
            perceptual_roughness: 0.4,
            ..default()
        }),
        turret_trim,
    });
}

// ---------------------------------------------------------------------------
// Beams

/// A beam as drawn: its ends in the shooter's and the target's own frames,
/// and how far along it is fading in or out. Outlives the sim's beam
/// briefly, so beams snap off with a tail and shield ripples die down.
struct BeamTrack {
    shooter: Entity,
    target: Entity,
    side: Side,
    from: Vec3,
    to: Vec3,
    shielded: bool,
    /// In the sim's beams this frame.
    live: bool,
    /// The beam itself: 0..1, quick in and out.
    strength: f32,
    /// What it leaves on a shield: slower to die down.
    afterglow: f32,
    /// Seconds since it started.
    age: f32,
    /// Seconds since it last threw sparks or scorched the hull.
    since_spark: f32,
    since_scorch: f32,
}

#[derive(Resource, Default)]
struct BeamTracks(Vec<BeamTrack>);

/// A ship's sim pose and its drawn transform: maps a point in the sim's
/// world to where it is drawn this frame.
struct ShipPose {
    position: Vec3,
    rotation: Quat,
    drawn: Transform,
}

impl ShipPose {
    fn to_local(&self, world: Vec3) -> Vec3 {
        self.rotation.inverse() * (world - self.position)
    }
}

type ShipPoses<'w, 's> = Query<
    'w,
    's,
    (
        &'static Position,
        &'static Rotation,
        &'static Transform,
        Has<HomeShip>,
    ),
    With<Combatant>,
>;

fn ship_pose(ships: &ShipPoses, ship: Entity) -> Option<(ShipPose, bool)> {
    let (position, rotation, drawn, home) = ships.get(ship).ok()?;
    Some((
        ShipPose {
            position: position.0,
            rotation: rotation.0,
            drawn: *drawn,
        },
        home,
    ))
}

/// Follows the sim's beams, in the ships' frames.
fn track_beams(
    time: Res<Time>,
    beams: Res<Beams>,
    ships: ShipPoses,
    mut tracks: ResMut<BeamTracks>,
) {
    let dt = time.delta_secs();
    for track in &mut tracks.0 {
        track.live = false;
    }
    for beam in &beams.0 {
        let (Some((shooter, home)), Some((target, _))) = (
            ship_pose(&ships, beam.shooter),
            ship_pose(&ships, beam.target),
        ) else {
            continue;
        };
        let from = shooter.to_local(beam.from);
        let to = target.to_local(beam.to);
        // The same turret on the same target: the beam it's been firing.
        let existing = tracks.0.iter_mut().find(|t| {
            t.shooter == beam.shooter
                && t.target == beam.target
                && t.from.distance(from) < 0.5
                && (t.live || t.strength > 0.0)
        });
        match existing {
            Some(track) => {
                track.to = to;
                track.shielded = beam.shielded;
                track.live = true;
            }
            None => tracks.0.push(BeamTrack {
                shooter: beam.shooter,
                target: beam.target,
                side: Side::of(home),
                from,
                to,
                shielded: beam.shielded,
                live: true,
                strength: 0.0,
                afterglow: 0.0,
                age: 0.0,
                since_spark: 0.0,
                since_scorch: 0.0,
            }),
        }
    }
    for track in &mut tracks.0 {
        track.age += dt;
        track.since_spark += dt;
        track.since_scorch += dt;
        if track.live {
            track.strength = (track.strength + dt * 12.0).min(1.0);
            track.afterglow = (track.afterglow + dt * 6.0).min(1.0);
        } else {
            track.strength = (track.strength - dt * 7.0).max(0.0);
            track.afterglow = (track.afterglow - dt * 1.2).max(0.0);
        }
    }
    tracks.0.retain(|t| {
        (t.live || t.afterglow > 0.0) && ships.contains(t.shooter) && ships.contains(t.target)
    });
}

/// Height of a turret's barrel pivot above its mount, and the barrel's
/// length: a beam is drawn from the muzzle.
const PIVOT_HEIGHT: f32 = 0.45;
const BARREL_LENGTH: f32 = 1.7;

/// Where a beam is drawn this frame: muzzle and end.
fn beam_ends(track: &BeamTrack, ships: &ShipPoses) -> Option<(Vec3, Vec3)> {
    let (shooter, _) = ship_pose(ships, track.shooter)?;
    let (target, _) = ship_pose(ships, track.target)?;
    let end = target.drawn.transform_point(track.to);
    let up = shooter.drawn.rotation * Vec3::Y * track.from.y.signum();
    let pivot = shooter.drawn.transform_point(track.from) + up * PIVOT_HEIGHT;
    let muzzle = pivot + (end - pivot).normalize_or_zero() * BARREL_LENGTH;
    Some((muzzle, end))
}

/// The pieces one beam is drawn with.
struct BeamSlot {
    core: Entity,
    sheath: Entity,
    muzzle: Entity,
    splash: Entity,
    light: Entity,
}

#[derive(Resource, Default)]
struct BeamPool(Vec<BeamSlot>);

/// Places a unit cylinder from `a` to `b` with radius `r`.
fn span(a: Vec3, b: Vec3, r: f32) -> Transform {
    let d = b - a;
    Transform::from_translation((a + b) / 2.0)
        .with_rotation(Quat::from_rotation_arc(Vec3::Y, d.normalize_or(Vec3::Y)))
        .with_scale(Vec3::new(r, d.length(), r))
}

/// The pieces of drawn beams (mesh parts, or the light).
type BeamParts<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Transform,
        &'static mut Visibility,
        Option<&'static mut MeshMaterial3d<StandardMaterial>>,
        Option<&'static mut PointLight>,
    ),
    Without<Combatant>,
>;

/// Draws each beam: a white-hot core in a colored sheath, a flare at the
/// muzzle, and a splash (and its light) where it stops.
fn draw_beams(
    time: Res<Time>,
    fx: Res<FxAssets>,
    tracks: Res<BeamTracks>,
    ships: ShipPoses,
    mut pool: ResMut<BeamPool>,
    mut parts: BeamParts,
    mut commands: Commands,
) {
    let t = time.elapsed_secs();
    let drawn: Vec<_> = tracks
        .0
        .iter()
        .filter(|tr| tr.strength > 0.0)
        .filter_map(|tr| Some((tr, beam_ends(tr, &ships)?)))
        .collect();
    while pool.0.len() < drawn.len() {
        let mut part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>| {
            commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::default(),
                    Visibility::Hidden,
                    NotShadowCaster,
                ))
                .id()
        };
        let slot = BeamSlot {
            core: part(&fx.cylinder, &fx.beam_core[0]),
            sheath: part(&fx.cylinder, &fx.beam_glow[0]),
            muzzle: part(&fx.sphere, &fx.beam_glow[0]),
            splash: part(&fx.sphere, &fx.shield_splash[0]),
            light: commands
                .spawn((
                    PointLight {
                        range: 25.0,
                        ..default()
                    },
                    Transform::default(),
                    Visibility::Hidden,
                ))
                .id(),
        };
        pool.0.push(slot);
    }
    for (i, slot) in pool.0.iter().enumerate() {
        let Some(&(track, (muzzle, end))) = drawn.get(i) else {
            for e in [slot.core, slot.sheath, slot.muzzle, slot.splash, slot.light] {
                if let Ok((_, mut v, ..)) = parts.get_mut(e) {
                    *v = Visibility::Hidden;
                }
            }
            continue;
        };
        let side = track.side.index();
        let s = track.strength;
        // A beam that crackles: its width flickers every frame.
        let flicker = 1.0 + 0.25 * (t * 97.0 + i as f32).sin() * (t * 61.3).cos();
        let splash = if track.shielded {
            &fx.shield_splash[side]
        } else {
            &fx.hull_splash
        };
        let splash_size = if track.shielded { 1.1 } else { 0.8 } * s * (0.8 + 0.3 * flicker);
        let poses = [
            (
                slot.core,
                span(muzzle, end, 0.07 * s * flicker),
                &fx.beam_core[side],
            ),
            (
                slot.sheath,
                span(muzzle, end, 0.32 * s * flicker),
                &fx.beam_glow[side],
            ),
            (
                slot.muzzle,
                Transform::from_translation(muzzle).with_scale(Vec3::splat(0.35 * s * flicker)),
                &fx.beam_glow[side],
            ),
            (
                slot.splash,
                Transform::from_translation(end).with_scale(Vec3::splat(splash_size)),
                splash,
            ),
        ];
        for (e, pose, material) in poses {
            if let Ok((mut transform, mut v, m, _)) = parts.get_mut(e) {
                *transform = pose;
                *v = Visibility::Visible;
                if let Some(mut m) = m
                    && m.0 != *material
                {
                    m.0 = material.clone();
                }
            }
        }
        if let Ok((mut transform, mut v, _, Some(mut light))) = parts.get_mut(slot.light) {
            // Just off the surface, toward the shooter, so it lights the
            // hull it's burning rather than sitting inside it.
            *transform =
                Transform::from_translation(end + (muzzle - end).normalize_or_zero() * 0.6);
            *v = Visibility::Visible;
            let c = if track.shielded {
                track.side.color()
            } else {
                LinearRgba::rgb(1.0, 0.55, 0.2)
            };
            light.color = c.into();
            light.intensity = 400_000.0 * s * flicker;
        }
    }
}

// ---------------------------------------------------------------------------
// Turrets

/// A turret's moving parts: the swivel (turns about the mount's up) and
/// the barrel's pivot (pitches).
#[derive(Component)]
struct TurretRig {
    swivel: Entity,
    pitch: Entity,
    /// Ship-local up for this mount: dorsal turrets stand, keel turrets hang.
    up: Vec3,
}

fn fit_turrets(
    fx: Option<Res<FxAssets>>,
    turrets: Query<(Entity, &Transform, &ChildOf), Added<Turret>>,
    homes: Query<(), With<HomeShip>>,
    mut commands: Commands,
) {
    let Some(fx) = fx else { return };
    for (turret, mount, ship) in &turrets {
        let side = Side::of(homes.contains(ship.parent())).index();
        let up = Vec3::Y * mount.translation.y.signum();
        let part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, t: Transform| {
            (Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), t)
        };
        // Barrel along -Z from the pivot, with a glowing band at the muzzle.
        let pitch = commands
            .spawn((
                Transform::from_xyz(0.0, PIVOT_HEIGHT, 0.0),
                Visibility::default(),
            ))
            .with_children(|p| {
                let along_z = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                p.spawn(part(
                    &fx.cylinder,
                    &fx.turret,
                    Transform::from_xyz(0.0, 0.0, -BARREL_LENGTH / 2.0)
                        .with_rotation(along_z)
                        .with_scale(Vec3::new(0.11, BARREL_LENGTH, 0.11)),
                ));
                p.spawn(part(
                    &fx.cylinder,
                    &fx.turret_trim[side],
                    Transform::from_xyz(0.0, 0.0, -BARREL_LENGTH + 0.12)
                        .with_rotation(along_z)
                        .with_scale(Vec3::new(0.15, 0.12, 0.15)),
                ));
                // The breech, so the barrel doesn't float.
                p.spawn(part(
                    &fx.cube,
                    &fx.turret,
                    Transform::from_scale(Vec3::new(0.42, 0.36, 0.6)),
                ));
            })
            .id();
        let swivel = commands
            .spawn((Transform::default(), Visibility::default()))
            .with_children(|s| {
                s.spawn(part(
                    &fx.cylinder,
                    &fx.turret,
                    Transform::from_xyz(0.0, 0.1, 0.0).with_scale(Vec3::new(0.48, 0.2, 0.48)),
                ));
                // Cheeks either side of the barrel.
                for x in [-0.3, 0.3] {
                    s.spawn(part(
                        &fx.cube,
                        &fx.turret,
                        Transform::from_xyz(x, 0.38, 0.05).with_scale(Vec3::new(0.12, 0.45, 0.55)),
                    ));
                }
            })
            .add_child(pitch)
            .id();
        commands
            .entity(turret)
            .insert((TurretRig { swivel, pitch, up }, Visibility::default()))
            .add_child(swivel);
    }
}

/// Turns each turret toward its ship's target, as drawn: the barrel lines up
/// with the beam it fires.
fn aim_turrets(
    rigs: Query<(&TurretRig, &Transform, &ChildOf)>,
    combatants: Query<(&Combatant, &Transform)>,
    mut parts: Query<&mut Transform, (Without<TurretRig>, Without<Combatant>)>,
) {
    for (rig, mount, ship) in &rigs {
        let Ok((combatant, ship_transform)) = combatants.get(ship.parent()) else {
            continue;
        };
        let Some(Ok((_, target))) = combatant.target.map(|t| combatants.get(t)) else {
            continue;
        };
        // The sim aims at the middle of the target's hull.
        let aim = target.transform_point(Vec3::new(0.0, 1.5, 0.0));
        let local = ship_transform.rotation.inverse() * (aim - ship_transform.translation);
        let d = local - mount.translation - rig.up * PIVOT_HEIGHT;
        let flat = d.reject_from(rig.up);
        if flat.length_squared() < 1e-6 {
            continue;
        }
        let swivel = Transform::default().looking_to(flat, rig.up).rotation;
        let in_swivel = swivel.inverse() * d;
        let pitch = in_swivel.y.atan2(-in_swivel.z);
        if let Ok(mut t) = parts.get_mut(rig.swivel) {
            t.rotation = swivel;
        }
        if let Ok(mut t) = parts.get_mut(rig.pitch) {
            t.rotation = Quat::from_rotation_x(pitch);
        }
    }
}

// ---------------------------------------------------------------------------
// Shields

#[derive(Clone, Copy, Default, ShaderType)]
struct ShieldUniform {
    color: Vec4,
    hits: [Vec4; 4],
    ages: Vec4,
    params: Vec4,
}

/// The shield bubble's look (see `shield.wgsl`).
#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct ShieldMaterial {
    #[uniform(0)]
    shield: ShieldUniform,
}

impl Material for ShieldMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHIELD_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// Both faces: from outside, the far side of the bubble shows through;
    /// from inside, only the far side exists.
    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// A ship's shield, drawn. It stays a while after the shield fails, to
/// break up.
#[derive(Component)]
struct ShieldBubble {
    ship: Entity,
    /// Seconds since the shield failed.
    failed: Option<f32>,
}

/// Marks a ship whose shield has been given a bubble (and so isn't given
/// another when it fails).
#[derive(Component)]
struct ShieldFitted;

const SHIELD_FAIL_SECONDS: f32 = 0.9;

fn fit_shields(
    ships: Query<(Entity, &Combatant, &ShipHull, Has<HomeShip>), Without<ShieldFitted>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ShieldMaterial>>,
    mut mesh: Local<Option<Handle<Mesh>>>,
    mut commands: Commands,
) {
    for (ship, combatant, hull, home) in &ships {
        if !combatant.shield {
            continue;
        }
        let mesh = mesh
            .get_or_insert_with(|| meshes.add(Sphere::new(1.0).mesh().uv(96, 48)))
            .clone();
        let tint = Side::of(home).color();
        let material = materials.add(ShieldMaterial {
            shield: ShieldUniform {
                color: Vec4::new(tint.red, tint.green, tint.blue, 0.35),
                ..default()
            },
        });
        let bubble = commands
            .spawn((
                Name::new("Shield"),
                ShieldBubble { ship, failed: None },
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(hull.0.shield().0).with_scale(hull.0.shield().1),
                NotShadowCaster,
            ))
            .id();
        commands.entity(ship).insert(ShieldFitted).add_child(bubble);
    }
}

/// Feeds each bubble its hits, whether the camera is inside it, and how far
/// its failure has got.
fn draw_shields(
    time: Res<Time>,
    tracks: Res<BeamTracks>,
    camera: Single<&Transform, With<PlayerCamera>>,
    ships: Query<(&Combatant, &Transform, &ShipHull)>,
    mut bubbles: Query<(Entity, &mut ShieldBubble, &MeshMaterial3d<ShieldMaterial>)>,
    mut materials: ResMut<Assets<ShieldMaterial>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut bubble, material) in &mut bubbles {
        let Ok((combatant, ship, hull)) = ships.get(bubble.ship) else {
            continue;
        };
        if !combatant.shield {
            let failed = bubble.failed.get_or_insert(0.0);
            *failed += dt;
            if *failed > SHIELD_FAIL_SECONDS {
                commands.entity(entity).despawn();
                continue;
            }
        }
        let Some(mut material) = materials.get_mut(&material.0) else {
            continue;
        };
        let u = &mut material.shield;
        let local = ship.rotation.inverse() * (camera.translation - ship.translation);
        let (shield_center, shield_radii) = hull.0.shield();
        let inside = ((local - shield_center) / shield_radii).length() < 1.0;
        let mut hits: Vec<&BeamTrack> = tracks
            .0
            .iter()
            .filter(|t| t.target == bubble.ship && t.shielded && t.afterglow > 0.0)
            .collect();
        hits.sort_by(|a, b| b.afterglow.total_cmp(&a.afterglow));
        u.hits = [Vec4::ZERO; 4];
        u.ages = Vec4::ZERO;
        let mut flare: f32 = 0.0;
        for (i, hit) in hits.iter().take(4).enumerate() {
            u.hits[i] = ship.transform_point(hit.to).extend(hit.afterglow);
            u.ages[i] = hit.age;
            // A jolt when a beam first lands, then a steady shimmer.
            flare += hit.afterglow * (0.12 + 0.8 * (-hit.age * 4.0).exp());
        }
        u.params = Vec4::new(
            time.elapsed_secs_wrapped(),
            if inside { 1.0 } else { 0.0 },
            bubble.failed.map_or(0.0, |f| f / SHIELD_FAIL_SECONDS),
            flare.min(1.5),
        );
    }
}

// ---------------------------------------------------------------------------
// Hull hits: sparks and scorch marks

/// A spark (or a bit of burning wreckage): flies, shrinks, goes out.
#[derive(Component)]
pub(crate) struct Spark {
    velocity: Vec3,
    life: f32,
    age: f32,
    size: f32,
}

/// A burn left on a hull by a beam, cooling from white hot to black.
#[derive(Component)]
pub(crate) struct Scorch {
    age: f32,
    stage: usize,
}

fn random_unit() -> Vec3 {
    loop {
        let v = Vec3::new(
            rand::random::<f32>() * 2.0 - 1.0,
            rand::random::<f32>() * 2.0 - 1.0,
            rand::random::<f32>() * 2.0 - 1.0,
        );
        if let Some(v) = v.try_normalize()
            && v.length_squared() <= 1.0
        {
            return v;
        }
    }
}

fn spawn_spark(
    commands: &mut Commands,
    fx: &FxAssets,
    at: Vec3,
    velocity: Vec3,
    size: f32,
    life: f32,
) {
    commands.spawn((
        Spark {
            velocity,
            life,
            age: 0.0,
            size,
        },
        Mesh3d(fx.cube.clone()),
        MeshMaterial3d(fx.spark.clone()),
        Transform::from_translation(at).with_scale(Vec3::splat(size)),
        NotShadowCaster,
    ));
}

/// A beam burning bare hull throws sparks and leaves scorch marks.
fn hull_hits(
    fx: Res<FxAssets>,
    mut tracks: ResMut<BeamTracks>,
    ships: Query<(&Transform, Option<&LinearVelocity>), With<Combatant>>,
    mut commands: Commands,
) {
    for track in &mut tracks.0 {
        if !track.live || track.shielded {
            continue;
        }
        let (Ok((target, velocity)), Ok((shooter, _))) =
            (ships.get(track.target), ships.get(track.shooter))
        else {
            continue;
        };
        let at = target.transform_point(track.to);
        let back = (shooter.translation - at).normalize_or_zero();
        let carried = velocity.map_or(Vec3::ZERO, |v| v.0);
        if track.since_spark > 0.03 {
            track.since_spark = 0.0;
            for _ in 0..3 {
                let v = (back + random_unit() * 0.9).normalize_or_zero()
                    * (4.0 + 10.0 * rand::random::<f32>());
                spawn_spark(
                    &mut commands,
                    &fx,
                    at,
                    carried + v,
                    0.07,
                    0.4 + 0.4 * rand::random::<f32>(),
                );
            }
        }
        if track.since_scorch > 0.25 {
            track.since_scorch = 0.0;
            let scorch = commands
                .spawn((
                    Scorch { age: 0.0, stage: 0 },
                    Mesh3d(fx.sphere.clone()),
                    MeshMaterial3d(fx.scorch[0].clone()),
                    Transform::from_translation(track.to)
                        .with_scale(Vec3::splat(0.22 + 0.15 * rand::random::<f32>())),
                    NotShadowCaster,
                ))
                .id();
            commands.entity(track.target).add_child(scorch);
        }
    }
}

fn animate_sparks(
    time: Res<Time>,
    mut sparks: Query<(Entity, &mut Spark, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut spark, mut transform) in &mut sparks {
        spark.age += dt;
        if spark.age >= spark.life {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += spark.velocity * dt;
        transform.scale = Vec3::splat(spark.size * (1.0 - spark.age / spark.life));
    }
}

/// Seconds a scorch spends at each stage of cooling before the next.
const SCORCH_STAGES: [f32; 3] = [0.3, 1.0, 2.5];

fn cool_scorches(
    time: Res<Time>,
    fx: Res<FxAssets>,
    mut scorches: Query<(&mut Scorch, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    for (mut scorch, mut material) in &mut scorches {
        scorch.age += time.delta_secs();
        let stage = SCORCH_STAGES
            .iter()
            .scan(0.0, |t, d| {
                *t += d;
                Some(*t)
            })
            .filter(|end| scorch.age > *end)
            .count();
        if stage != scorch.stage {
            scorch.stage = stage;
            material.0 = fx.scorch[stage].clone();
        }
    }
}

// ---------------------------------------------------------------------------
// Alert lighting

const ALARM_RED: Color = Color::srgb(1.0, 0.08, 0.04);

/// Our ship's lights pulse red while it's under fire, harder when it's
/// badly hurt. A carrier whose shield is down is counting down to blowing
/// up: its lights go to a red strobe that sweeps along the hull.
fn alert_lights(
    time: Res<Time>,
    beams: Res<Beams>,
    ships: Query<(&Combatant, Has<HomeShip>)>,
    mut lights: Query<(&ShipLight, &mut PointLight, &Transform, &ChildOf)>,
) {
    let t = time.elapsed_secs();
    for (normal, mut light, transform, ship) in &mut lights {
        let Ok((combatant, home)) = ships.get(ship.parent()) else {
            continue;
        };
        let (color, intensity) = if home {
            let hit = beams.0.iter().any(|b| b.target == ship.parent());
            let hurt = combatant.integrity < 0.35;
            let (level, hz) = match (hurt, hit) {
                (true, _) => (1.0, 1.6),
                (false, true) => (0.55, 0.7),
                (false, false) => (0.0, 0.0),
            };
            let pulse = 0.5 + 0.5 * (t * hz * std::f32::consts::TAU).sin();
            let k = level * pulse;
            (
                normal.color.mix(&ALARM_RED, k),
                normal.intensity * (1.0 - 0.45 * level * (1.0 - pulse)),
            )
        } else if !combatant.shield {
            let phase = (t * 1.8 - transform.translation.z / 30.0).rem_euclid(1.0);
            let on = phase < 0.3;
            (ALARM_RED, if on { 220_000.0 } else { 6_000.0 })
        } else {
            (normal.color, normal.intensity)
        };
        light.color = color;
        light.intensity = intensity;
    }
}

// ---------------------------------------------------------------------------
// Buttons

/// A button's face: background and text, as pixels.
#[derive(Clone, Copy)]
enum ButtonStyle {
    /// Pick a scenario.
    Go,
    /// Yellow and black: you were warned.
    Hazard,
    /// Big, red, and on the enemy's bridge.
    Danger,
}

impl ButtonStyle {
    fn of(action: ControlAction) -> Self {
        match action {
            ControlAction::Scenario(_) | ControlAction::Armory => ButtonStyle::Go,
            ControlAction::Lurch => ButtonStyle::Hazard,
            ControlAction::DisableShields => ButtonStyle::Danger,
        }
    }

    /// Body color, and how much it glows.
    fn body(self) -> (Color, f32) {
        match self {
            ButtonStyle::Go => (Color::srgb(0.15, 0.75, 0.3), 0.35),
            ButtonStyle::Hazard => (Color::srgb(0.95, 0.75, 0.05), 0.3),
            ButtonStyle::Danger => (Color::srgb(0.9, 0.06, 0.04), 0.8),
        }
    }
}

/// The danger button breathes, to be found in a dark bridge.
#[derive(Component)]
struct PulsingButton(Handle<StandardMaterial>);

fn fit_buttons(
    controls: Query<(Entity, &Control, &Transform), Added<Control>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    for (entity, control, transform) in &controls {
        let style = ButtonStyle::of(control.action);
        let half = control.half_size;
        // The face is across the button's thinnest axis, toward the
        // cabin: inward from a wall, up from a console top.
        let thin = half.min_position();
        let normal = match thin {
            1 => Vec3::Y,
            axis => {
                let mut n = Vec3::ZERO;
                n[axis] = -transform.translation[axis].signum();
                n
            }
        };
        let up = if thin == 1 { Vec3::NEG_Z } else { Vec3::Y };
        let right = up.cross(normal);
        let face = Vec2::new(
            (half * right).abs().max_element(),
            (half * up).abs().max_element(),
        ) * 2.0;
        let (color, glow) = style.body();
        let body = materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * glow,
            perceptual_roughness: 0.5,
            ..default()
        });
        let image = images.add(face_image(control.action.label(), face, style));
        let face_material = materials.add(StandardMaterial {
            base_color_texture: Some(image.clone()),
            emissive_texture: Some(image),
            emissive: LinearRgba::WHITE * glow,
            perceptual_roughness: 0.5,
            ..default()
        });
        commands
            .entity(entity)
            .insert(Visibility::default())
            .with_children(|b| {
                b.spawn((
                    Mesh3d(meshes.add(Cuboid::from_size(half * 2.0))),
                    MeshMaterial3d(body.clone()),
                ));
                b.spawn((
                    Mesh3d(meshes.add(Rectangle::from_size(face))),
                    MeshMaterial3d(face_material.clone()),
                    Transform::from_translation(normal * (half.dot(normal.abs()) + 0.002))
                        .with_rotation(Quat::from_mat3(&Mat3::from_cols(right, up, normal))),
                ));
            });
        if matches!(style, ButtonStyle::Danger) {
            commands.entity(entity).insert(PulsingButton(face_material));
        }
    }
}

fn pulse_buttons(
    time: Res<Time>,
    buttons: Query<&PulsingButton>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let k = 0.5 + 0.5 * (time.elapsed_secs() * 3.0).sin();
    for button in &buttons {
        if let Some(mut m) = materials.get_mut(&button.0) {
            m.emissive = LinearRgba::WHITE * (0.5 + 1.5 * k);
        }
    }
}

/// Texels per font pixel.
const TEXEL: usize = 4;

/// Draws a button face of `size` meters: background, border, and `label`
/// in capitals, wrapped to read as large as it fits.
fn face_image(label: &str, size: Vec2, style: ButtonStyle) -> Image {
    let text = label.to_uppercase();
    let lines = wrap(&text, size);
    let cols = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1);
    // Text block in font pixels: 6 per glyph (5 + gap), 9 per line.
    let block = Vec2::new((cols * 6 + 1) as f32, (lines.len() * 9 + 1) as f32);
    let border = 3.0;
    let per_meter = ((block + Vec2::splat(2.0 * border + 2.0)) / size).max_element();
    let (w, h) = (
        (size.x * per_meter).ceil() as usize * TEXEL,
        (size.y * per_meter).ceil() as usize * TEXEL,
    );
    let mut px = vec![[0u8; 4]; w * h];
    let (bg, fg, edge): ([u8; 4], [u8; 4], [u8; 4]) = match style {
        ButtonStyle::Go => ([30, 170, 70, 255], [8, 20, 10, 255], [12, 70, 30, 255]),
        ButtonStyle::Hazard => ([245, 195, 10, 255], [10, 10, 10, 255], [10, 10, 10, 255]),
        ButtonStyle::Danger => ([220, 20, 15, 255], [255, 235, 225, 255], [90, 5, 5, 255]),
    };
    let b = border as usize * TEXEL;
    for y in 0..h {
        for x in 0..w {
            let in_border = x < b || y < b || x >= w - b || y >= h - b;
            px[y * w + x] = match (in_border, style) {
                // Hazard stripes round the edge.
                (true, ButtonStyle::Hazard) if ((x + y) / (2 * TEXEL)).is_multiple_of(2) => bg,
                (true, _) => edge,
                (false, _) => bg,
            };
        }
    }
    let x0 = (w / TEXEL - block.x as usize) / 2;
    let y0 = (h / TEXEL - block.y as usize) / 2;
    for (row, line) in lines.iter().enumerate() {
        let n = line.chars().count();
        let lx = x0 + 1 + (cols - n) * 3;
        for (i, c) in line.chars().enumerate() {
            for (gy, bits) in glyph(c).iter().enumerate() {
                for gx in 0..5 {
                    if bits & (0x10 >> gx) == 0 {
                        continue;
                    }
                    let (fx, fy) = (lx + i * 6 + gx, y0 + 1 + row * 9 + gy);
                    for ty in 0..TEXEL {
                        for tx in 0..TEXEL {
                            px[(fy * TEXEL + ty) * w + fx * TEXEL + tx] = fg;
                        }
                    }
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px.concat(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// Splits `text` into lines so it reads largest on a face of `size`.
fn wrap(text: &str, size: Vec2) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut best = (0.0, vec![text.to_string()]);
    for width in 1..=text.len().max(1) {
        let mut lines: Vec<String> = Vec::new();
        for word in &words {
            match lines.last_mut() {
                Some(line) if line.len() + 1 + word.len() <= width => {
                    line.push(' ');
                    line.push_str(word);
                }
                _ => lines.push(word.to_string()),
            }
        }
        let cols = lines.iter().map(|l| l.len()).max().unwrap_or(1);
        // Glyph scale this layout allows (meters per font pixel).
        let scale = (size.x / (cols * 6 + 8) as f32).min(size.y / (lines.len() * 9 + 8) as f32);
        if scale > best.0 {
            best = (scale, lines);
        }
    }
    best.1
}

/// A 5x7 glyph, one row per byte (bit 4 is the left column). Unknown
/// characters are blank.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        _ => [0; 7],
    }
}

// ---------------------------------------------------------------------------
// Explosions

/// A ship blowing up: a white flash, a fireball of overlapping puffs, a
/// shock ring, and a light, all drifting on with the ship's last velocity.
#[derive(Component)]
struct Explosion {
    age: f32,
    origin: Vec3,
    velocity: Vec3,
    flash: Entity,
    ring: Entity,
    /// Each puff: its entity, which way it billows, and how big it gets.
    puffs: Vec<(Entity, Vec3, f32)>,
}

const EXPLOSION_SECONDS: f32 = 2.4;
const FLASH_SECONDS: f32 = 0.3;
const RING_SECONDS: f32 = 1.2;
const RING_RADIUS: f32 = 90.0;

/// A ship that loses its [`Combatant`] worn down to nothing was destroyed
/// (not tidied away by a scenario change): blow it up where it was drawn.
fn explode(
    remove: On<Remove, Combatant>,
    ships: Query<(&Combatant, &Transform, Option<&LinearVelocity>)>,
    fx: Option<Res<FxAssets>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let (Ok((combatant, transform, velocity)), Some(fx)) = (ships.get(remove.entity), fx) else {
        return;
    };
    // A client may hear of the last bit of wear a little late.
    if combatant.integrity > 0.1 {
        return;
    }
    let at = transform.translation;
    let velocity = velocity.map_or(Vec3::ZERO, |v| v.0);
    let mut part = |mesh: &Handle<Mesh>, rotation: Quat| {
        commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add(glow(LinearRgba::BLACK))),
                Transform::from_translation(at)
                    .with_rotation(rotation)
                    .with_scale(Vec3::ZERO),
                NotShadowCaster,
            ))
            .id()
    };
    let flash = part(&fx.sphere, Quat::IDENTITY);
    // The shock ring spreads in the ship's own plane.
    let ring = part(&fx.ring, transform.rotation);
    // Puffs along the hull (it's long), billowing out.
    let puffs = (0..8)
        .map(|_| {
            let along = transform.rotation * Vec3::Z * (rand::random::<f32>() - 0.5) * 24.0;
            let out = along + random_unit() * 6.0;
            (
                part(&fx.sphere, Quat::IDENTITY),
                out,
                7.0 + 8.0 * rand::random::<f32>(),
            )
        })
        .collect();
    commands.spawn((
        Name::new("Explosion"),
        Explosion {
            age: 0.0,
            origin: at,
            velocity,
            flash,
            ring,
            puffs,
        },
        PointLight {
            color: Color::srgb(1.0, 0.6, 0.3),
            intensity: 0.0,
            range: 400.0,
            ..default()
        },
        Transform::from_translation(at),
    ));
    // Burning bits flung out of it.
    for _ in 0..90 {
        let v = random_unit() * (8.0 + 40.0 * rand::random::<f32>().powi(2));
        let size = 0.15 + 0.5 * rand::random::<f32>().powi(3);
        let life = 1.0 + 2.5 * rand::random::<f32>();
        spawn_spark(&mut commands, &fx, at + v * 0.05, velocity + v, size, life);
    }
}

/// A robot destroyed (not tidied away whole by a scenario change): a burst
/// of sparks and a brief flash where it was drawn.
fn robot_destroyed(
    remove: On<Remove, Robot>,
    robots: Query<(&Vitals, Option<&BodyTuning>)>,
    avatars: Query<(&Avatar, &Transform)>,
    fx: Option<Res<FxAssets>>,
    mut commands: Commands,
) {
    let (Ok((vitals, own)), Some(fx)) = (robots.get(remove.entity), fx) else {
        return;
    };
    if own.is_some_and(|t| vitals.health >= t.0.player_health) {
        return;
    }
    let Some((_, transform)) = avatars.iter().find(|(a, _)| a.player == remove.entity) else {
        return;
    };
    let at = transform.translation;
    for _ in 0..30 {
        let v = random_unit() * (3.0 + 12.0 * rand::random::<f32>());
        let life = 0.4 + 0.8 * rand::random::<f32>();
        spawn_spark(&mut commands, &fx, at, v, 0.08 + 0.12 * rand::random::<f32>(), life);
    }
    commands.spawn((
        Name::new("Robot flash"),
        RobotFlash(0.0),
        PointLight {
            color: Color::srgb(1.0, 0.7, 0.4),
            intensity: 2_000_000.0,
            range: 25.0,
            ..default()
        },
        Transform::from_translation(at),
    ));
}

#[derive(Component)]
struct RobotFlash(f32);

fn fade_robot_flashes(
    time: Res<Time>,
    mut commands: Commands,
    mut flashes: Query<(Entity, &mut RobotFlash, &mut PointLight)>,
) {
    for (entity, mut flash, mut light) in &mut flashes {
        flash.0 += time.delta_secs();
        if flash.0 > 0.3 {
            commands.entity(entity).despawn();
        } else {
            light.intensity = 2_000_000.0 * (1.0 - flash.0 / 0.3);
        }
    }
}

fn animate_explosions(
    time: Res<Time>,
    mut explosions: Query<(Entity, &mut Explosion, &mut PointLight, &mut Transform)>,
    mut parts: Query<(&mut Transform, &MeshMaterial3d<StandardMaterial>), Without<Explosion>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut boom, mut light, mut center) in &mut explosions {
        boom.age += dt;
        let t = boom.age / EXPLOSION_SECONDS;
        if t >= 1.0 {
            commands.entity(entity).despawn();
            for e in [boom.flash, boom.ring]
                .into_iter()
                .chain(boom.puffs.iter().map(|p| p.0))
            {
                commands.entity(e).despawn();
            }
            continue;
        }
        let at = boom.origin + boom.velocity * boom.age;
        center.translation = at;
        let swell = 1.0 - (1.0 - t).powi(4);
        let heat = (1.0 - t).powi(3);
        let flash = (1.0 - boom.age / FLASH_SECONDS).max(0.0);
        let ring = (boom.age / RING_SECONDS).min(1.0);
        let mut looks = vec![
            (
                boom.flash,
                at,
                8.0 + 10.0 * (1.0 - flash),
                LinearRgba::rgb(20.0, 18.0, 14.0) * flash,
            ),
            (
                boom.ring,
                at,
                RING_RADIUS * (1.0 - (1.0 - ring).powi(2)),
                scaled(LinearRgba::rgb(1.0, 0.7, 0.45), 2.5 * (1.0 - ring)),
            ),
        ];
        // Puffs cool from yellow-white through orange to a dull red; where
        // they overlap, the fireball is brightest.
        let fire = LinearRgba::rgb(1.0, 0.2 + 0.6 * heat, 0.04 + 0.3 * heat);
        for &(puff, out, size) in &boom.puffs {
            looks.push((
                puff,
                at + out * swell,
                size * (0.3 + 0.7 * swell),
                scaled(fire, 1.4 * heat),
            ));
        }
        for (e, position, size, color) in looks {
            if let Ok((mut transform, material)) = parts.get_mut(e) {
                transform.translation = position;
                transform.scale = Vec3::splat(size);
                if let Some(mut m) = materials.get_mut(&material.0) {
                    m.base_color = color.into();
                }
            }
        }
        light.intensity = 3.0e9 * (0.6 * flash + 0.4 * heat);
    }
}
