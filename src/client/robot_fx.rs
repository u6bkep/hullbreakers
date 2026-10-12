//! Robots, presented: what their affixes look like and are doing, the hull
//! turret's searchlight, the warnings before an attack (the marksman's
//! laser, the grappler's draw), venting steam, the sounds robots make
//! through a hull you share with them, and robots coming apart when they're
//! destroyed (a volatile one going up).
//!
//! Driven by what every peer has: each robot's kind, its [`Affixes`] and
//! [`Condition`], and its player state. Nothing here feeds back into the
//! simulation.
//!
//! A robot model carries every affix's parts (`affix_<tag>_...`); the ones
//! it doesn't have are hidden. Its quieter lights (`emissive_secondary`) get
//! materials of its own, so each robot can be lit by what it's doing: a
//! rallied robot's lights pulse gold with its leader's beacon, an
//! overclocked one's run from dull red to white with heat.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use avian3d::prelude::*;
use bevy::audio::{PlaybackMode, Volume};
use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use super::LocalPlayer;
use super::animate;
use super::camera::{CameraFx, PlayerCamera};
use super::combat_fx::{FxAssets, ShieldMaterial, ShieldUniform, glow, random_unit, spawn_spark};
use super::events::EventCursor;
use super::render::Avatar;
use super::sfx::{self, Synth, concat, mix, normalize, wav};
use crate::sim::Layer;
use crate::sim::bots::{Affix, Affixes, BodyTuning, Condition, Robot, Wrecked, blast_of, tuning_of};
use crate::sim::events::PlayerEvent;
use crate::sim::player::{Mode, Player};
use crate::sim::tuning::Tuning;
use crate::sim::weapons::GunKind;

pub struct RobotFxPlugin;

impl Plugin for RobotFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (setup_robot_fx, setup_robot_sounds))
            .add_observer(robot_destroyed)
            .add_observer(undress)
            .add_systems(
                PostUpdate,
                (dress_robots, light_robots, draw_shells, draw_telegraphs)
                    .chain()
                    // Once avatars are posed and rigged for this frame.
                    .after(animate::animate_avatars)
                    .before(TransformSystems::Propagate),
            )
            .add_systems(Update, (vent_steam, drift_puffs, fly_wreckage, animate_blasts, robot_sounds))
            .add_systems(bevy_egui::EguiPrimaryContextPass, label_zoo);
    }
}

// ---------------------------------------------------------------------------
// Colours

/// A shell's tint, and its emitter's.
const SHELL: LinearRgba = LinearRgba::rgb(0.25, 0.8, 1.0);
/// A volatile robot's warning lights.
const HAZARD: LinearRgba = LinearRgba::rgb(1.0, 0.45, 0.04);
/// A leader's beacon, and the lights of robots it rallies.
const GOLD: LinearRgba = LinearRgba::rgb(1.0, 0.68, 0.12);
/// The marksman's aiming laser.
const LASER: LinearRgba = LinearRgba::rgb(1.0, 0.05, 0.02);
/// The turret's searchlight: a warm white.
const SEARCHLIGHT: LinearRgba = LinearRgba::rgb(1.0, 0.92, 0.75);

fn scaled(c: LinearRgba, k: f32) -> LinearRgba {
    LinearRgba::rgb(c.red * k, c.green * k, c.blue * k)
}

fn blend(a: LinearRgba, b: LinearRgba, t: f32) -> LinearRgba {
    let t = t.clamp(0.0, 1.0);
    LinearRgba::rgb(a.red + (b.red - a.red) * t, a.green + (b.green - a.green) * t, a.blue + (b.blue - a.blue) * t)
}

/// How hot an overclocked robot's lights burn at `heat` (0 cold, 1 full):
/// dull red, through orange, to white.
fn heat_glow(heat: f32) -> LinearRgba {
    let heat = heat.clamp(0.0, 1.0);
    if heat < 0.5 {
        blend(LinearRgba::rgb(0.35, 0.03, 0.0), LinearRgba::rgb(2.4, 0.7, 0.08), heat * 2.0)
    } else {
        blend(LinearRgba::rgb(2.4, 0.7, 0.08), LinearRgba::rgb(5.0, 4.0, 2.8), (heat - 0.5) * 2.0)
    }
}

/// An uneven flicker in 0..1, different for each `seed`.
fn flicker(t: f32, seed: f32) -> f32 {
    let a = (t * 13.0 + seed * 7.1).sin() * (t * 7.3 + seed * 3.3).sin();
    let b = (t * 31.0 + seed).sin();
    (0.55 + 0.35 * a + 0.1 * b).clamp(0.0, 1.0)
}

/// The rally pulse, shared by every leader's beacon and the robots it
/// rallies, so they read as one group (0..1).
fn rally_pulse(t: f32) -> f32 {
    0.5 + 0.5 * (t * 4.0).sin()
}

// ---------------------------------------------------------------------------
// Assets

#[derive(Resource)]
struct RobotFxAssets {
    shell: Handle<Mesh>,
    laser: Handle<StandardMaterial>,
    laser_locked: Handle<StandardMaterial>,
    beam_cone: Handle<Mesh>,
    beam_glow: Handle<StandardMaterial>,
    /// Steam, from fresh to thin.
    steam: [Handle<StandardMaterial>; 4],
}

/// The turret's light: its shaft is this long (m) and this wide (half
/// angle, rad).
const BEAM_LENGTH: f32 = 45.0;
const BEAM_ANGLE: f32 = 0.11;

fn setup_robot_fx(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let both_sides = |m: StandardMaterial| StandardMaterial {
        cull_mode: None,
        double_sided: true,
        ..m
    };
    commands.insert_resource(RobotFxAssets {
        shell: meshes.add(Sphere::new(1.0).mesh().uv(32, 16)),
        laser: materials.add(glow(scaled(LASER, 6.0))),
        laser_locked: materials.add(glow(scaled(LASER + LinearRgba::rgb(0.0, 0.25, 0.2), 16.0))),
        beam_cone: meshes.add(Cone::new(BEAM_LENGTH * BEAM_ANGLE.tan(), BEAM_LENGTH).mesh().resolution(16)),
        beam_glow: materials.add(both_sides(glow(scaled(SEARCHLIGHT, 0.035)))),
        steam: [0.5, 0.3, 0.16, 0.06].map(|k| materials.add(glow(LinearRgba::rgb(k, k, k * 1.05)))),
    });
}

// ---------------------------------------------------------------------------
// Dressing: affix parts, lights, and what each affix adds

/// A robot's avatar, dressed for what it is.
#[derive(Component)]
struct RobotLook {
    robot: Entity,
    affixes: Affixes,
    /// Its own power lights (outside its affix parts): their material, and
    /// its glow as built.
    accents: Option<(Handle<StandardMaterial>, LinearRgba)>,
    /// Each of its affixes' lights.
    lights: Vec<(Affix, Handle<StandardMaterial>)>,
    /// Its model's sockets, by name (without `SOCKET_`).
    sockets: HashMap<String, Entity>,
    /// Its shell (Shielded), the marksman's laser (beam, dot and the glint
    /// at its barrel), and a light of its own (a leader's beacon, a
    /// grappler's draw).
    shell: Option<Entity>,
    laser: Option<[Entity; 3]>,
    lamp: Option<Entity>,
    /// Its condition as last drawn, a shell hit's flash (fading) and how
    /// long ago it was, and seconds since its shell broke.
    last: Condition,
    flash: f32,
    since_flash: f32,
    broke: Option<f32>,
    /// Steam owed, in puffs (venting).
    steam: f32,
    /// Its own phase for flickers.
    seed: f32,
}

#[allow(clippy::too_many_arguments)]
fn dress_robots(
    base: Res<Tuning>,
    fx: Res<RobotFxAssets>,
    avatars: Query<(Entity, &Avatar), (With<animate::Rig>, Without<RobotLook>)>,
    robots: Query<(&Robot, Option<&Affixes>, Option<&BodyTuning>, Option<&Condition>)>,
    tree: (Query<&Children>, Query<&ChildOf>, Query<&Name>),
    lit: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
    gallery: Query<(), With<crate::sim::scenarios::gallery::GalleryAnchor>>,
    mut visibility: Query<&mut Visibility>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut shells: ResMut<Assets<ShieldMaterial>>,
    mut commands: Commands,
) {
    let (children, parents, names) = tree;
    for (avatar, &Avatar { player }) in &avatars {
        let Ok((&kind, affixes, own, condition)) = robots.get(player) else { continue };
        let affixes = affixes.copied().unwrap_or_default();
        let tuning = tuning_of(&base, own);
        // Which affix a node is a part of, if any.
        let affix_named = |name: &str| -> Option<Affix> {
            let rest = name.strip_prefix("affix_")?;
            Affix::ALL.into_iter().find(|a| rest.strip_prefix(a.tag()).is_some_and(|r| r.starts_with('_')))
        };
        let affix_above = |mut e: Entity| -> Option<Affix> {
            loop {
                if let Some(affix) = names.get(e).ok().and_then(|n| affix_named(n)) {
                    return Some(affix);
                }
                if e == avatar {
                    return None;
                }
                e = parents.get(e).ok()?.parent();
            }
        };
        // Its affixes' parts shown, the others' hidden; its sockets found.
        let mut sockets = HashMap::new();
        for e in children.iter_descendants(avatar) {
            let Ok(name) = names.get(e) else { continue };
            if let Some(socket) = name.strip_prefix("SOCKET_") {
                sockets.insert(socket.to_string(), e);
            }
            if name.starts_with("affix_")
                && affix_named(name).is_none_or(|a| !affixes.has(a))
                && let Ok(mut v) = visibility.get_mut(e)
            {
                *v = Visibility::Hidden;
            }
        }
        // Its quiet lights, on materials of its own: one for its body, one
        // for each affix's parts.
        let mut accents: Option<(Handle<StandardMaterial>, LinearRgba)> = None;
        let mut lights: Vec<(Affix, Handle<StandardMaterial>)> = Vec::new();
        for e in children.iter_descendants(avatar) {
            let Ok((role, material)) = lit.get(e) else { continue };
            if role.0 != "emissive_secondary" {
                continue;
            }
            let Some(original) = materials.get(&material.0).cloned() else { continue };
            let handle = match affix_above(e) {
                Some(affix) => match lights.iter().find(|(a, _)| *a == affix) {
                    Some((_, h)) => h.clone(),
                    None => {
                        let h = materials.add(original);
                        lights.push((affix, h.clone()));
                        h
                    }
                },
                None => match &accents {
                    Some((h, _)) => h.clone(),
                    None => {
                        let glow = original.emissive;
                        let h = materials.add(original);
                        accents = Some((h.clone(), glow));
                        h
                    }
                },
            };
            commands.entity(e).insert(MeshMaterial3d(handle));
        }
        // A shell round it, if it's shielded.
        let shell = affixes.has(Affix::Shielded).then(|| {
            let r = tuning.capsule_radius * 1.6;
            let size = Vec3::new(r, (tuning.capsule_height * 0.5 + 0.12).max(r), r);
            let material = shells.add(ShieldMaterial {
                shield: ShieldUniform {
                    color: Vec4::new(SHELL.red, SHELL.green, SHELL.blue, 1.0),
                    ..default()
                },
            });
            let bubble = commands
                .spawn((Name::new("Shell"), Mesh3d(fx.shell.clone()), MeshMaterial3d(material), Transform::from_scale(size), NotShadowCaster))
                .id();
            commands.entity(avatar).add_child(bubble);
            bubble
        });
        // The marksman's laser, drawn in the world while it aims.
        let laser = affixes.has(Affix::Marksman).then(|| {
            let part = |commands: &mut Commands, mesh: Handle<Mesh>| {
                commands
                    .spawn((Mesh3d(mesh), MeshMaterial3d(fx.laser.clone()), Transform::default(), Visibility::Hidden, NotShadowCaster))
                    .id()
            };
            // The beam is the unit sphere stretched into a rod (round where
            // it meets the dot); the dot and the glint are the sphere.
            [part(&mut commands, fx.shell.clone()), part(&mut commands, fx.shell.clone()), part(&mut commands, fx.shell.clone())]
        });
        // A light of its own: a leader's beacon; the glow of a grappler's
        // launcher as it draws back to throw.
        let lamp_at = match kind {
            Robot::Grappler => sockets.get("muzzle"),
            _ if affixes.has(Affix::Leader) => sockets.get("beacon"),
            _ => None,
        };
        let lamp = lamp_at.map(|&socket| {
            let lamp = commands
                .spawn((
                    PointLight {
                        color: if kind == Robot::Grappler { Color::srgb(1.0, 0.6, 0.2) } else { GOLD.into() },
                        intensity: 0.0,
                        range: 8.0,
                        radius: 0.05,
                        ..default()
                    },
                    Transform::default(),
                ))
                .id();
            commands.entity(socket).add_child(lamp);
            lamp
        });
        // The turret's searchlight: a spotlight at its eye, and a faint shaft
        // (not in the gallery, where the shafts would cross its other areas).
        if kind == Robot::Turret
            && let Some(&eye) = sockets.get("eye")
        {
            let shaft = gallery.is_empty();
            commands.entity(eye).with_children(|lamp| {
                lamp.spawn((
                    SpotLight {
                        color: SEARCHLIGHT.into(),
                        intensity: SEARCHLIGHT_LUMENS,
                        range: 280.0,
                        radius: 0.15,
                        shadow_maps_enabled: false,
                        outer_angle: BEAM_ANGLE * 1.4,
                        inner_angle: BEAM_ANGLE * 0.6,
                        ..default()
                    },
                    Transform::default(),
                ));
                // The cone's tip at the lamp, opening forward (−Z).
                if shaft {
                    lamp.spawn((
                        Mesh3d(fx.beam_cone.clone()),
                        MeshMaterial3d(fx.beam_glow.clone()),
                        Transform::from_translation(Vec3::NEG_Z * BEAM_LENGTH / 2.0).with_rotation(Quat::from_rotation_x(FRAC_PI_2)),
                        NotShadowCaster,
                    ));
                }
            });
        }
        commands.entity(avatar).insert(RobotLook {
            robot: player,
            affixes,
            accents,
            lights,
            sockets,
            shell,
            laser,
            lamp,
            last: condition.copied().unwrap_or_default(),
            flash: 0.0,
            since_flash: 10.0,
            broke: None,
            steam: 0.0,
            seed: ((player.to_bits() % 997) as f32 * 0.618).fract() * 10.0,
        });
    }
}

/// The searchlight's power (lumens).
const SEARCHLIGHT_LUMENS: f32 = 6.0e8;
/// A leader's beacon at its brightest (lumens).
const BEACON_LUMENS: f32 = 4.0e5;

/// Lights each robot by what its affixes are doing.
fn light_robots(
    time: Res<Time>,
    looks: Query<&RobotLook>,
    conditions: Query<&Condition>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut lamps: Query<&mut PointLight>,
) {
    let t = time.elapsed_secs();
    let pulse = rally_pulse(t);
    for look in &looks {
        let c = conditions.get(look.robot).copied().unwrap_or_default();
        let heat = c.heat_fraction();
        let vent_flicker = 0.6 + 0.4 * flicker(t * 2.0, look.seed);
        if let Some((handle, built)) = &look.accents
            && let Some(mut m) = materials.get_mut(handle)
        {
            let mut e = *built;
            if look.affixes.has(Affix::Overclocked) {
                e = blend(e, heat_glow(heat), heat);
                if c.venting {
                    e = scaled(heat_glow(1.0), vent_flicker);
                }
            }
            if c.rallied {
                e = blend(e, scaled(GOLD, 3.5), 0.35 + 0.6 * pulse);
            }
            m.emissive = e;
        }
        for (affix, handle) in &look.lights {
            if matches!(affix, Affix::Armoured | Affix::Marksman) {
                continue;
            }
            let Some(mut m) = materials.get_mut(handle) else { continue };
            m.emissive = match affix {
                Affix::Shielded => {
                    let charge = c.shell_fraction();
                    // Dark when broken; flickering back up as it charges.
                    let k = if charge >= 1.0 { 2.5 } else { (0.3 + 3.0 * charge) * flicker(t * 3.0, look.seed) };
                    scaled(SHELL, k)
                }
                Affix::Overclocked => scaled(heat_glow(heat.max(0.08)), if c.venting { vent_flicker } else { 1.0 }),
                Affix::Volatile => scaled(HAZARD, 1.2 + 4.0 * flicker(t * 1.6, look.seed).powi(3)),
                Affix::Leader => scaled(GOLD, 1.0 + 5.0 * pulse),
                Affix::Armoured | Affix::Marksman => m.emissive,
            };
        }
        if let Some(mut lamp) = look.lamp.and_then(|l| lamps.get_mut(l).ok()) {
            lamp.intensity = if look.affixes.has(Affix::Leader) {
                BEACON_LUMENS * (0.15 + 0.85 * pulse)
            } else {
                // A grappler drawing back to throw.
                BEACON_LUMENS * 2.0 * c.winding_fraction()
            };
        }
    }
}

/// How long a broken shell takes to break up (s).
const SHELL_BREAK_SECONDS: f32 = 0.6;

/// Feeds each shell its charge, its hits and its breaking.
fn draw_shells(
    time: Res<Time>,
    camera: Single<&Transform, With<PlayerCamera>>,
    mut looks: Query<(&mut RobotLook, &Transform)>,
    conditions: Query<&Condition>,
    bubbles: Query<(&MeshMaterial3d<ShieldMaterial>, &Transform), Without<RobotLook>>,
    mut visibility: Query<&mut Visibility>,
    mut shells: ResMut<Assets<ShieldMaterial>>,
) {
    let dt = time.delta_secs();
    for (mut look, avatar) in &mut looks {
        let c = conditions.get(look.robot).copied().unwrap_or_default();
        let look = &mut *look;
        if let Some(bubble) = look.shell {
            if c.shell_hits != look.last.shell_hits {
                look.flash = 1.0;
                look.since_flash = 0.0;
            }
            if c.shell == 0 && look.last.shell > 0 {
                look.broke = Some(0.0);
            }
            if c.shell > 0 {
                look.broke = None;
            }
            look.flash *= (-5.0 * dt).exp();
            look.since_flash += dt;
            if let Some(b) = &mut look.broke {
                *b += dt;
            }
            let broken = look.broke.is_some_and(|b| b > SHELL_BREAK_SECONDS) || (c.shell == 0 && look.broke.is_none());
            if let Ok(mut v) = visibility.get_mut(bubble) {
                v.set_if_neq(if broken { Visibility::Hidden } else { Visibility::Inherited });
            }
            if let Ok((material, local)) = bubbles.get(bubble)
                && let Some(mut m) = shells.get_mut(&material.0)
            {
                let u = &mut m.shield;
                let charge = c.shell_fraction();
                u.color.w = 0.6 + 1.6 * charge;
                // A hit lights the side facing the camera (whoever's
                // shooting, most likely).
                let toward = (camera.translation - avatar.translation).normalize_or(Vec3::Z);
                let radius = local.scale.max_element();
                let inside = camera.translation.distance(avatar.translation) < radius;
                u.hits[0] = (avatar.translation + toward * radius * 0.9).extend(look.flash * 0.5);
                u.ages = Vec4::new(look.since_flash, 0.0, 0.0, 0.0);
                u.params = Vec4::new(
                    time.elapsed_secs_wrapped(),
                    if inside { 1.0 } else { 0.0 },
                    look.broke.map_or(0.0, |b| (b / SHELL_BREAK_SECONDS).min(1.0)),
                    look.flash * 0.8,
                );
            }
        }
        look.last = c;
    }
}

/// While a marksman winds up a shot, its laser runs from its long barrel
/// to where it's aiming (the first thing along its aim, a player
/// included, so teammates see the dot on whoever it's on), and a glint at
/// its barrel swells: a laser aimed at you is seen end on, so the glint is
/// what its target sees. Both turn hot and blink once its aim has locked.
#[allow(clippy::too_many_arguments)]
fn draw_telegraphs(
    time: Res<Time>,
    base: Res<Tuning>,
    fx: Res<RobotFxAssets>,
    spatial: SpatialQuery,
    camera: Single<&Transform, (With<PlayerCamera>, Without<RobotLook>)>,
    looks: Query<(&RobotLook, &Transform, &GlobalTransform)>,
    players: Query<(&Player, Option<&BodyTuning>, Option<&Condition>)>,
    globals: Query<&GlobalTransform, Without<RobotLook>>,
    mut parts: Query<(&mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>), (Without<RobotLook>, Without<PlayerCamera>)>,
) {
    for (look, now, then) in &looks {
        let Some([beam, dot, glint]) = look.laser else { continue };
        let Ok((player, own, condition)) = players.get(look.robot) else { continue };
        let c = condition.copied().unwrap_or_default();
        let on = c.winding > 0;
        for e in [beam, dot, glint] {
            if let Ok((_, mut v, _)) = parts.get_mut(e) {
                v.set_if_neq(if on { Visibility::Visible } else { Visibility::Hidden });
            }
        }
        if !on {
            continue;
        }
        let tuning = tuning_of(&base, own);
        let pitch = match player.mode {
            Mode::Booted { pitch, .. } | Mode::Eva { pitch, .. } => pitch,
        };
        let eye = now.translation + now.rotation * Vec3::Y * (tuning.eye_height - tuning.capsule_height / 2.0);
        let dir = now.rotation * Quat::from_rotation_x(pitch) * Vec3::NEG_Z;
        let reach = Dir3::new(dir)
            .ok()
            .and_then(|d| spatial.cast_ray(eye, d, LASER_RANGE, true, &SpatialQueryFilter::default().with_excluded_entities([look.robot])))
            .map_or(LASER_RANGE, |hit| hit.distance);
        let target = eye + dir * reach;
        // From the barrel as drawn now: its place on the model (as of the
        // last frame's transforms) carried by where the model is now.
        let start = look
            .sockets
            .get("aim")
            .and_then(|s| globals.get(*s).ok())
            .map_or(eye, |s| now.transform_point(then.affine().inverse().transform_point3(s.translation())));
        // At least a pixel or so wide where it's seen.
        let width = 0.006 + 0.0013 * camera.translation.distance(start.lerp(target, 0.5)).min(60.0);
        let blink = c.locked && (time.elapsed_secs() * 14.0).sin() > -0.3;
        let material = if blink { fx.laser_locked.clone() } else { fx.laser.clone() };
        let width = if c.locked { width * 1.8 } else { width };
        if let Ok((mut t, _, mut m)) = parts.get_mut(beam) {
            *t = span(start, target, width);
            m.0 = material.clone();
        }
        if let Ok((mut t, _, mut m)) = parts.get_mut(dot) {
            let size = (0.03 + 0.004 * camera.translation.distance(target)) * if c.locked { 1.8 } else { 1.0 };
            *t = Transform::from_translation(target).with_scale(Vec3::splat(size));
            m.0 = material.clone();
        }
        if let Ok((mut t, _, mut m)) = parts.get_mut(glint) {
            // A few pixels across wherever it's seen from, swelling as the
            // shot comes.
            let wound = c.winding_fraction();
            let size = (0.02 + 0.006 * camera.translation.distance(start)) * (0.4 + 0.8 * wound) * if c.locked { 1.6 } else { 1.0 };
            *t = Transform::from_translation(start).with_scale(Vec3::splat(size));
            m.0 = material;
        }
    }
}

/// How far the marksman's laser reaches (m).
const LASER_RANGE: f32 = 160.0;

/// A unit sphere stretched into a rod from `a` to `b`, `r` thick.
fn span(a: Vec3, b: Vec3, r: f32) -> Transform {
    let d = b - a;
    let length = d.length().max(1e-3);
    Transform::from_translation((a + b) / 2.0)
        .with_rotation(Quat::from_rotation_arc(Vec3::Y, d / length))
        .with_scale(Vec3::new(r, length / 2.0, r))
}

// ---------------------------------------------------------------------------
// Steam

/// A puff of steam: drifts, swells, thins out.
#[derive(Component)]
struct Puff {
    velocity: Vec3,
    age: f32,
    life: f32,
    size: f32,
}

/// Puffs a second from each vent, while venting.
const STEAM_RATE: f32 = 24.0;

fn vent_steam(
    time: Res<Time>,
    fx: Res<RobotFxAssets>,
    mut looks: Query<&mut RobotLook>,
    conditions: Query<&Condition>,
    globals: Query<&GlobalTransform>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for mut look in &mut looks {
        let venting = conditions.get(look.robot).is_ok_and(|c| c.venting);
        if !venting {
            look.steam = 0.0;
            continue;
        }
        look.steam += STEAM_RATE * dt;
        let vents: Vec<Entity> = ["vent", "vent_l", "vent_r"].iter().filter_map(|v| look.sockets.get(*v).copied()).collect();
        while look.steam >= 1.0 {
            look.steam -= 1.0;
            for vent in &vents {
                let Ok(at) = globals.get(*vent) else { continue };
                let out = at.forward().as_vec3() * (2.0 + 1.5 * rand::random::<f32>()) + random_unit() * 0.5;
                commands.spawn((
                    Puff {
                        velocity: out,
                        age: 0.0,
                        life: 0.6 + 0.6 * rand::random::<f32>(),
                        size: 0.05 + 0.04 * rand::random::<f32>(),
                    },
                    Mesh3d(fx.shell.clone()),
                    MeshMaterial3d(fx.steam[0].clone()),
                    Transform::from_translation(at.translation()).with_scale(Vec3::splat(0.05)),
                    NotShadowCaster,
                ));
            }
        }
    }
}

fn drift_puffs(
    time: Res<Time>,
    fx: Res<RobotFxAssets>,
    mut puffs: Query<(Entity, &mut Puff, &mut Transform, &mut MeshMaterial3d<StandardMaterial>)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut puff, mut t, mut m) in &mut puffs {
        puff.age += dt;
        let k = puff.age / puff.life;
        if k >= 1.0 {
            commands.entity(entity).despawn();
            continue;
        }
        // Slowing as it spreads.
        let v = puff.velocity;
        t.translation += v * dt;
        puff.velocity = v * (-1.5 * dt).exp();
        t.scale = Vec3::splat(puff.size + 0.4 * k.sqrt());
        let stage = ((k * 4.0) as usize).min(3);
        if m.0 != fx.steam[stage] {
            m.0 = fx.steam[stage].clone();
        }
    }
}

// ---------------------------------------------------------------------------
// Destruction

/// A piece of a destroyed robot, tumbling away, bouncing off what it
/// meets, then shrinking out of sight.
#[derive(Component)]
struct Wreck {
    velocity: Vec3,
    spin: Vec3,
    age: f32,
    life: f32,
}

/// How long a piece shrinks for, after its life (s).
const WRECK_SHRINK: f32 = 0.5;

/// The parts a kind of robot comes apart into (each takes its children).
fn pieces(kind: Robot) -> &'static [&'static str] {
    match kind {
        Robot::Gunner => &["head", "upper_arm_l", "upper_arm_r", "thigh_l", "thigh_r", "torso", "pelvis"],
        Robot::Swarmer => &["thruster_fl", "thruster_fr", "thruster_rl", "thruster_rr", "gun", "body"],
        Robot::Grappler => &["head", "upper_arm_l", "upper_arm_r", "winch", "thigh_l", "thigh_r", "torso", "pelvis"],
        Robot::Turret => &["barrel", "head", "base"],
        Robot::Crawler => &["leg_fl", "leg_ml", "leg_rl", "leg_fr", "leg_mr", "leg_rr", "head", "body"],
    }
}

/// A robot destroyed (not tidied away whole by a scenario change) comes
/// apart where it was drawn: its parts fly off, lights out, in a burst of
/// sparks and a flash; a volatile one goes up in a fireball.
#[allow(clippy::too_many_arguments)]
fn robot_destroyed(
    remove: On<Remove, Robot>,
    robots: Query<(&Robot, Has<Wrecked>, Option<&Affixes>, &Player)>,
    avatars: Query<(Entity, &Avatar, &Transform)>,
    tree: (Query<&Children>, Query<&Name>, Query<&GlobalTransform>),
    lamps: (Query<(), Or<(With<PointLight>, With<SpotLight>)>>, Query<&Mesh3d>, Option<Res<RobotFxAssets>>),
    lit: Query<&MeshMaterial3d<StandardMaterial>>,
    velocities: Query<&LinearVelocity>,
    listener: Query<&Player, With<LocalPlayer>>,
    view: Query<&Transform, With<PlayerCamera>>,
    assets: (Option<Res<FxAssets>>, Option<Res<RobotSounds>>),
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut camera: ResMut<CameraFx>,
    mut commands: Commands,
) {
    let (children, names, globals) = tree;
    let (lights, meshes, robot_fx) = lamps;
    let (Ok((&kind, wrecked, affixes, player)), (Some(fx), sounds)) = (robots.get(remove.entity), assets) else {
        return;
    };
    if !wrecked {
        return;
    }
    // Its lights put out: dark copies of its glowing materials, for this
    // wreck alone (they go with its pieces).
    let mut dead: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>> = HashMap::new();
    let Some((avatar, _, transform)) = avatars.iter().find(|(_, a, _)| a.player == remove.entity) else {
        return;
    };
    let at = transform.translation;
    // Its velocity in the world: its own, or what it had walking on its
    // ship.
    let velocity = match player.mode {
        Mode::Eva { velocity, .. } => velocity,
        Mode::Booted { body, velocity, .. } => {
            let ship = velocities.get(body).map_or(Vec3::ZERO, |v| v.0);
            ship + globals.get(body).map_or(velocity, |g| g.rotation() * velocity)
        }
    };
    // It comes apart.
    for name in pieces(kind) {
        let Some(part) = children.iter_descendants(avatar).find(|e| names.get(*e).is_ok_and(|n| n.as_str() == *name)) else {
            continue;
        };
        let Ok(pose) = globals.get(part) else { continue };
        let pose = pose.compute_transform();
        let out = (pose.translation - at).normalize_or(random_unit());
        commands.entity(part).remove::<ChildOf>().insert((
            pose,
            Wreck {
                velocity: velocity + out * (1.5 + 3.0 * rand::random::<f32>()) + random_unit() * 0.8,
                spin: random_unit() * (2.0 + 6.0 * rand::random::<f32>()),
                age: 0.0,
                life: 1.6 + 1.2 * rand::random::<f32>(),
            },
        ));
        // Lights out: its lamps (a beacon, a searchlight and its shaft) go,
        // and its glowing materials go dark.
        for e in std::iter::once(part).chain(children.iter_descendants(part)) {
            let shaft = robot_fx.as_ref().is_some_and(|r| meshes.get(e).is_ok_and(|m| m.0 == r.beam_cone));
            if lights.contains(e) || shaft {
                // A lamp's own lamps (a searchlight's shaft) go with it.
                commands.entity(e).try_despawn();
                continue;
            }
            let Ok(material) = lit.get(e) else { continue };
            let id = material.0.id();
            let dark = match dead.get(&id) {
                Some(h) => h.clone(),
                None => {
                    let Some(m) = materials.get(&material.0) else { continue };
                    if m.emissive == LinearRgba::BLACK {
                        continue;
                    }
                    let mut m = m.clone();
                    m.emissive = LinearRgba::BLACK;
                    m.base_color = m.base_color.with_luminance(m.base_color.luminance() * 0.4);
                    let h = materials.add(m);
                    dead.insert(id, h.clone());
                    h
                }
            };
            commands.entity(e).insert(MeshMaterial3d(dark));
        }
    }
    for _ in 0..30 {
        let v = random_unit() * (3.0 + 12.0 * rand::random::<f32>());
        let life = 0.4 + 0.8 * rand::random::<f32>();
        spawn_spark(&mut commands, &fx, at, velocity + v, 0.08 + 0.12 * rand::random::<f32>(), life);
    }
    commands.spawn((
        Name::new("Robot flash"),
        Blast::flash(at, velocity),
        PointLight {
            color: Color::srgb(1.0, 0.7, 0.4),
            intensity: 0.0,
            range: 25.0,
            ..default()
        },
        Transform::from_translation(at),
    ));
    let shared = listener.single().ok().and_then(|p| p.mode.body()).is_some_and(|b| player.mode.body() == Some(b));
    let distance = view.single().map_or(f32::MAX, |eye| eye.translation.distance(at));
    if affixes.is_some_and(|a| a.has(Affix::Volatile)) {
        let radius = blast_of(kind, at, remove.entity, None).radius;
        spawn_blast(&mut commands, &fx, &mut materials, at, velocity, radius);
        let near = 1.0 - distance / (radius * 2.5);
        if near > 0.0 {
            camera.shake(0.9 * near);
        }
        if let Some(s) = &sounds
            && (shared || distance < radius * 2.0)
        {
            play_at(&mut commands, &s.blast, 1.4, 1.0, at);
        }
    } else if let Some(s) = &sounds
        && shared
    {
        play_at(&mut commands, &s.crash, 1.0, 0.9 + 0.2 * rand::random::<f32>(), at);
    }
}

/// A robot's avatar gone: what it had drawn in the world goes with it (the
/// marksman's laser isn't parented to it).
fn undress(remove: On<Remove, RobotLook>, looks: Query<&RobotLook>, mut commands: Commands) {
    let Ok(look) = looks.get(remove.entity) else { return };
    for e in look.laser.into_iter().flatten() {
        commands.entity(e).try_despawn();
    }
}

fn fly_wreckage(time: Res<Time>, spatial: SpatialQuery, mut wrecks: Query<(Entity, &mut Wreck, &mut Transform)>, mut commands: Commands) {
    let dt = time.delta_secs();
    let filter = SpatialQueryFilter::from_mask(Layer::World);
    for (entity, mut wreck, mut t) in &mut wrecks {
        wreck.age += dt;
        if wreck.age > wreck.life + WRECK_SHRINK {
            commands.entity(entity).despawn();
            continue;
        }
        let step = wreck.velocity * dt;
        match Dir3::new(step).ok().and_then(|d| spatial.cast_ray(t.translation, d, step.length() + 0.05, true, &filter)) {
            // It bounces off, losing most of its speed.
            Some(hit) => {
                let n = hit.normal;
                wreck.velocity = (wreck.velocity - 2.0 * wreck.velocity.dot(n) * n) * 0.35;
                wreck.spin *= 0.5;
            }
            None => t.translation += step,
        }
        let spin = wreck.spin;
        t.rotate(Quat::from_scaled_axis(spin * dt));
        if wreck.age > wreck.life {
            let k = 1.0 - (wreck.age - wreck.life) / WRECK_SHRINK;
            t.scale = Vec3::splat(k.max(0.0));
        }
    }
}

/// A robot's end: a flash; for a volatile one, a fireball of puffs and a
/// shock ring out to the blast's reach, drifting on with its last
/// velocity.
#[derive(Component)]
struct Blast {
    age: f32,
    origin: Vec3,
    velocity: Vec3,
    /// How far the blast reaches (m); 0 for a plain flash.
    radius: f32,
    /// Its flash, ring and puffs (each: which way it billows, how big it
    /// gets).
    flash: Option<Entity>,
    ring: Option<Entity>,
    puffs: Vec<(Entity, Vec3, f32)>,
}

impl Blast {
    fn flash(origin: Vec3, velocity: Vec3) -> Blast {
        Blast {
            age: 0.0,
            origin,
            velocity,
            radius: 0.0,
            flash: None,
            ring: None,
            puffs: Vec::new(),
        }
    }

    fn seconds(&self) -> f32 {
        if self.radius > 0.0 { BLAST_SECONDS } else { FLASH_SECONDS }
    }
}

const BLAST_SECONDS: f32 = 1.3;
const FLASH_SECONDS: f32 = 0.3;

fn spawn_blast(commands: &mut Commands, fx: &FxAssets, materials: &mut Assets<StandardMaterial>, at: Vec3, velocity: Vec3, radius: f32) {
    let mut part = |mesh: &Handle<Mesh>, rotation: Quat| {
        commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(materials.add(glow(LinearRgba::BLACK))),
                Transform::from_translation(at).with_rotation(rotation).with_scale(Vec3::ZERO),
                NotShadowCaster,
            ))
            .id()
    };
    let flash = part(&fx.sphere, Quat::IDENTITY);
    let ring = part(&fx.ring, Quat::from_rotation_arc(Vec3::Y, random_unit()));
    let puffs = (0..12)
        .map(|_| (part(&fx.sphere, Quat::IDENTITY), random_unit() * radius * 0.3, radius * (0.07 + 0.08 * rand::random::<f32>())))
        .collect();
    commands.spawn((
        Name::new("Robot blast"),
        Blast {
            age: 0.0,
            origin: at,
            velocity,
            radius,
            flash: Some(flash),
            ring: Some(ring),
            puffs,
        },
        PointLight {
            color: Color::srgb(1.0, 0.6, 0.25),
            intensity: 0.0,
            range: radius * 6.0,
            ..default()
        },
        Transform::from_translation(at),
    ));
    for _ in 0..50 {
        let v = random_unit() * (4.0 + 20.0 * rand::random::<f32>().powi(2));
        spawn_spark(commands, fx, at + v * 0.03, velocity + v, 0.06 + 0.18 * rand::random::<f32>().powi(2), 0.5 + 1.2 * rand::random::<f32>());
    }
}

fn animate_blasts(
    time: Res<Time>,
    mut blasts: Query<(Entity, &mut Blast, &mut PointLight, &mut Transform)>,
    mut parts: Query<(&mut Transform, &MeshMaterial3d<StandardMaterial>), Without<Blast>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut blast, mut light, mut center) in &mut blasts {
        blast.age += dt;
        let t = blast.age / blast.seconds();
        if t >= 1.0 {
            commands.entity(entity).despawn();
            // Some of these hang from the blast, and went with it.
            for e in [blast.flash, blast.ring].into_iter().flatten().chain(blast.puffs.iter().map(|p| p.0)) {
                commands.entity(e).try_despawn();
            }
            continue;
        }
        let at = blast.origin + blast.velocity * blast.age;
        center.translation = at;
        if blast.radius <= 0.0 {
            light.intensity = 2_000_000.0 * (1.0 - t);
            continue;
        }
        let r = blast.radius;
        let swell = 1.0 - (1.0 - t).powi(3);
        let heat = (1.0 - t).powi(2);
        let flash = (1.0 - blast.age / 0.15).max(0.0);
        let ring = (blast.age / 0.45).min(1.0);
        let mut looks: Vec<(Entity, Vec3, f32, LinearRgba)> = Vec::new();
        if let Some(e) = blast.flash {
            looks.push((e, at, r * (0.12 + 0.2 * (1.0 - flash)), scaled(LinearRgba::rgb(20.0, 17.0, 12.0), flash)));
        }
        if let Some(e) = blast.ring {
            looks.push((e, at, r * 1.2 * (1.0 - (1.0 - ring).powi(2)), scaled(LinearRgba::rgb(1.0, 0.75, 0.45), 3.0 * (1.0 - ring))));
        }
        let fire = LinearRgba::rgb(1.0, 0.2 + 0.6 * heat, 0.04 + 0.3 * heat);
        for &(puff, out, size) in &blast.puffs {
            looks.push((puff, at + out * swell, size * (0.3 + 0.7 * swell), scaled(fire, 3.0 * heat)));
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
        light.intensity = 6.0e7 * (0.7 * flash + 0.3 * heat);
    }
}

// ---------------------------------------------------------------------------
// The gallery's robots

/// In the gallery's table of robots, a label under each: its kind's name
/// under the plain one that starts each row, its affix's under the others.
fn label_zoo(
    mut contexts: bevy_egui::EguiContexts,
    gallery: Query<(), With<crate::sim::scenarios::gallery::GalleryAnchor>>,
    camera: crate::client::hud::CameraView,
    looks: Query<(&RobotLook, &GlobalTransform)>,
    robots: Query<(&Robot, Option<&Affixes>)>,
) -> Result {
    if gallery.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let (camera, eye) = *camera;
    let view = crate::client::hud::Projector::new(camera, eye, ctx);
    let painter = ctx.layer_painter(bevy_egui::egui::LayerId::new(bevy_egui::egui::Order::Background, bevy_egui::egui::Id::new("zoo")));
    for (look, at) in &looks {
        let Ok((kind, affixes)) = robots.get(look.robot) else { continue };
        let (label, size) = match affixes.and_then(|a| a.iter().next()) {
            Some(affix) => (affix.name(), 14.0),
            None => (kind.name(), 17.0),
        };
        let below = at.translation() - Vec3::Y * 1.2;
        let distance = view.eye.translation.distance(below);
        // Only while you face the table, so its labels stay out of the view
        // of the gallery's other areas.
        if Vec3::from(view.eye.forward()).angle_between(below - view.eye.translation) > super::gallery::FACING || distance > 30.0 {
            continue;
        }
        let Some(p) = view.to_screen(below) else { continue };
        painter.text(
            p,
            bevy_egui::egui::Align2::CENTER_TOP,
            label,
            bevy_egui::egui::FontId::proportional(size),
            bevy_egui::egui::Color32::from_rgb(255, 214, 150),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Sound

/// Robots' own sounds, heard (like the crew's) only through the body you
/// share with them.
#[derive(Resource)]
pub struct RobotSounds {
    rifle: Handle<AudioSource>,
    sprayer: Handle<AudioSource>,
    marksman: Handle<AudioSource>,
    cannon: Handle<AudioSource>,
    /// The marksman's charge, rising as it aims; the click of its lock.
    charge: Handle<AudioSource>,
    lock: Handle<AudioSource>,
    /// The grappler's winch ratcheting back, and its harpoon going.
    ratchet: Handle<AudioSource>,
    harpoon: Handle<AudioSource>,
    /// A shell giving out; an overheated robot venting.
    shell_break: Handle<AudioSource>,
    vent: Handle<AudioSource>,
    /// A robot coming apart; a volatile one going up.
    crash: Handle<AudioSource>,
    blast: Handle<AudioSource>,
    /// A crawler's magnetic feet clacking down, one by one, and the clunk
    /// of it taking a new surface.
    clack: Handle<AudioSource>,
    clamp: Handle<AudioSource>,
}

fn setup_robot_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut synth = Synth {
        rng: ChaCha8Rng::seed_from_u64(17),
    };
    let mut add = |samples: Vec<f32>| {
        sources.add(AudioSource {
            bytes: wav(&normalize(samples, 0.9)).into(),
        })
    };
    let ratchet = (0..7).fold(Vec::new(), |acc, i| {
        let click = mix(synth.clank(0.055, 950.0 + 40.0 * i as f32, 90.0, 0.5), synth.thud(0.055, 80.0, 1500.0, 0.3));
        concat(acc, click)
    });
    commands.insert_resource(RobotSounds {
        rifle: add(mix(synth.thud(0.12, 50.0, 800.0, 1.0), synth.clank(0.1, 520.0, 45.0, 0.4))),
        sprayer: add(mix(synth.thud(0.07, 70.0, 1100.0, 0.9), synth.clank(0.05, 800.0, 70.0, 0.3))),
        marksman: add(mix(synth.thud(0.22, 28.0, 1300.0, 1.0), synth.sweep(0.3, 3200.0, 400.0, 0.5, 0.45))),
        cannon: add(mix(synth.thud(0.4, 12.0, 280.0, 1.0), synth.clank(0.35, 150.0, 14.0, 0.5))),
        charge: add(synth.sweep(1.0, 260.0, 2600.0, 0.08, 0.5)),
        lock: add(concat(synth.beep(0.05, 2400.0, 0.4), synth.beep(0.05, 2400.0, 0.4))),
        ratchet: add(ratchet),
        harpoon: add(mix(synth.thud(0.28, 18.0, 500.0, 1.0), synth.sweep(0.35, 1900.0, 260.0, 0.5, 0.5))),
        shell_break: add(mix(synth.crackle(0.5, 0.012, 0.995), synth.sweep(0.45, 2600.0, 140.0, 0.3, 0.6))),
        vent: add(synth.thud(1.4, 2.2, 5000.0, 0.8)),
        crash: add(mix(synth.clank(0.7, 120.0, 6.0, 0.8), synth.thud(0.45, 9.0, 420.0, 1.0))),
        blast: add(synth.explosion(1.8, 2600.0, 1.3)),
        clack: add(mix(synth.clank(0.05, 1650.0, 95.0, 0.6), synth.thud(0.04, 120.0, 2500.0, 0.4))),
        clamp: add(mix(synth.clank(0.25, 330.0, 22.0, 0.8), synth.thud(0.2, 25.0, 700.0, 0.7))),
    });
}

fn play_at(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, speed: f32, position: Vec3) {
    play_far(commands, sound, volume, speed, position, sfx::STRUCTURE_SCALE);
}

/// As [`play_at`], carrying as far as `scale` lets it (smaller: further).
fn play_far(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, speed: f32, position: Vec3, scale: f32) {
    commands.spawn((
        AudioPlayer(sound.clone()),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(volume),
            speed,
            spatial: true,
            spatial_scale: Some(bevy::audio::SpatialScale::new(scale)),
            ..default()
        },
        Transform::from_translation(position),
    ));
}

/// What presentation keeps for each robot's sounds.
#[derive(Component, Default)]
struct RobotHearing {
    events: EventCursor,
    last: Condition,
    /// Distance walked since a crawler's last footfall (m).
    stride: f32,
}

/// A crawler's footfalls come this often (m walked), and its clatter
/// carries this much further through a hull than other sounds do.
const CLACK_EVERY: f32 = 0.13;
const CLATTER_REACH: f32 = 2.5;

/// Robots' gunfire, and what their affixes and attacks sound like, heard
/// where they are when they're on the body you're standing on.
fn robot_sounds(
    time: Res<Time>,
    sounds: Res<RobotSounds>,
    listener: Query<&Player, With<LocalPlayer>>,
    mut robots: Query<(Entity, &Robot, &Player, Option<&Condition>, Option<&Affixes>, Option<&mut RobotHearing>)>,
    avatars: Query<(&Avatar, &GlobalTransform)>,
    mut commands: Commands,
) {
    let shared = listener.single().ok().and_then(|p| p.mode.body());
    for (entity, kind, player, condition, affixes, hearing) in &mut robots {
        let Some(mut hearing) = hearing else {
            commands.entity(entity).insert(RobotHearing::default());
            continue;
        };
        let hearing = &mut *hearing;
        // Read the log whether or not it's heard, so nothing plays late.
        let events = hearing.events.take_new(&player.events);
        let c = condition.copied().unwrap_or_default();
        let last = std::mem::replace(&mut hearing.last, c);
        if shared.is_none() || player.mode.body() != shared {
            continue;
        }
        let Some(at) = avatars.iter().find(|(a, _)| a.player == entity).map(|(_, t)| t.translation()) else {
            continue;
        };
        let pitch = || 0.93 + 0.14 * rand::random::<f32>();
        for &event in &events {
            match event {
                PlayerEvent::Fired { gun, .. } => {
                    let (sound, volume) = match gun {
                        GunKind::SwarmerSprayer => (&sounds.sprayer, 0.5),
                        GunKind::MarksmanRifle => (&sounds.marksman, 1.0),
                        GunKind::TurretCannon => (&sounds.cannon, 1.0),
                        _ => (&sounds.rifle, 0.7),
                    };
                    play_at(&mut commands, sound, volume, pitch(), at);
                }
                PlayerEvent::GrappleAttached { .. } if *kind == Robot::Grappler => {
                    play_at(&mut commands, &sounds.harpoon, 1.0, pitch(), at);
                }
                _ => {}
            }
        }
        let marksman = affixes.is_some_and(|a| a.has(Affix::Marksman));
        if c.winding > 0 && last.winding == 0 {
            if marksman {
                play_at(&mut commands, &sounds.charge, 0.7, 1.0, at);
            } else if *kind == Robot::Grappler {
                play_at(&mut commands, &sounds.ratchet, 0.8, pitch(), at);
            }
        }
        if c.locked && !last.locked {
            play_at(&mut commands, &sounds.lock, 0.8, 1.0, at);
        }
        if c.shell == 0 && last.shell > 0 {
            play_at(&mut commands, &sounds.shell_break, 0.9, pitch(), at);
        }
        if c.venting && !last.venting {
            play_at(&mut commands, &sounds.vent, 0.9, pitch(), at);
        }
        // A crawler is heard well before it's seen: its feet clatter
        // through the hull, and it clunks onto each new surface.
        if *kind == Robot::Crawler
            && let Mode::Booted { velocity, .. } = player.mode
        {
            let reach = sfx::STRUCTURE_SCALE / CLATTER_REACH;
            hearing.stride += velocity.length() * time.delta_secs();
            if hearing.stride > CLACK_EVERY {
                hearing.stride = 0.0;
                play_far(&mut commands, &sounds.clack, 0.55, pitch(), at, reach);
            }
            if events.iter().any(|e| matches!(e, PlayerEvent::SurfaceChanged { .. } | PlayerEvent::Landed { .. })) {
                play_far(&mut commands, &sounds.clamp, 0.9, pitch(), at, reach);
            }
        }
    }
}
