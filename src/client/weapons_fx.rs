//! What shooting looks like: tracers from every gun, sparks where they land,
//! and each family's own: a plasma bolt (a slow glowing ball that bursts),
//! an arc (a jagged bolt to its target and on to the next), a laser (a beam
//! held as long as the trigger is).
//!
//! All of it is drawn from the `Fired` entries in each player's event log
//! (predicted for us, replicated for everyone else). Flown shots are flown
//! here, in straight lines, until they meet something; instant ones are
//! traced here by the same rules the authority uses. It's only for show: the
//! authority decides hits (see `sim::shots`).

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::Rng;

use super::LocalPlayer;
use super::camera::PlayerCamera;
use crate::sim::Layer;
use crate::sim::bots::Robot;
use crate::sim::events::PlayerEvent;
use crate::sim::player::Player;
use crate::sim::shots::{ARC_JUMP, BOLT_LIFE, SHOT_LIFE};
use crate::sim::weapons::{GunKind, Hand, ShotKind};

#[derive(Component)]
pub struct Tracer {
    shooter: Entity,
    velocity: Vec3,
    age: f32,
    /// How far it has come from the hand (m).
    travelled: f32,
    /// A plasma bolt: a ball, not a streak, and a burst where it lands.
    bolt: bool,
}

#[derive(Component)]
pub struct Spark {
    age: f32,
    /// How long it lasts and how far it grows (x its size).
    life: f32,
    grow: f32,
}

/// A beam or an arc's bolt: there for a moment.
#[derive(Component)]
pub struct Zap {
    age: f32,
    life: f32,
}

#[derive(Resource)]
pub struct WeaponFxAssets {
    tracer: Handle<Mesh>,
    spark: Handle<Mesh>,
    bolt: Handle<Mesh>,
    smg: Handle<StandardMaterial>,
    rifle: Handle<StandardMaterial>,
    shotgun: Handle<StandardMaterial>,
    sniper: Handle<StandardMaterial>,
    plasma: Handle<StandardMaterial>,
    arc: Handle<StandardMaterial>,
    laser: Handle<StandardMaterial>,
    /// Robots' tracers: hot red-orange.
    robot: Handle<StandardMaterial>,
    spark_material: Handle<StandardMaterial>,
}

/// A tracer's streak is as long as this much of its flight (s), but never
/// reaches back past the hand.
const STREAK_SECONDS: f32 = 0.04;
const SPARK_SECONDS: f32 = 0.15;
/// Where a tracer drawn from the hand crosses the line of fire (m).
const CONVERGE: f32 = 40.0;
/// A tick of beam lasts this long on screen (the next tick's replaces it).
const BEAM_SECONDS: f32 = 0.05;
const ARC_SECONDS: f32 = 0.12;

pub fn setup_weapon_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut glow = |c: LinearRgba| {
        // Unlit draws the base colour as is: bright enough to bloom.
        materials.add(StandardMaterial {
            base_color: Color::LinearRgba(c),
            unlit: true,
            ..default()
        })
    };
    commands.insert_resource(WeaponFxAssets {
        // A thin streak 1 m long trailing behind its head (along +Z), scaled
        // to length.
        tracer: meshes.add(Cuboid::new(0.05, 0.05, 1.0).mesh().build().translated_by(Vec3::Z * 0.5)),
        spark: meshes.add(Sphere::new(0.12).mesh().ico(1).unwrap()),
        bolt: meshes.add(Sphere::new(0.14).mesh().ico(2).unwrap()),
        smg: glow(LinearRgba::rgb(8.0, 5.0, 1.5)),
        rifle: glow(LinearRgba::rgb(4.0, 7.0, 10.0)),
        shotgun: glow(LinearRgba::rgb(9.0, 5.5, 1.5)),
        sniper: glow(LinearRgba::rgb(10.0, 11.0, 13.0)),
        plasma: glow(LinearRgba::rgb(5.0, 14.0, 3.0)),
        arc: glow(LinearRgba::rgb(4.0, 9.0, 16.0)),
        laser: glow(LinearRgba::rgb(16.0, 1.5, 1.0)),
        robot: glow(LinearRgba::rgb(10.0, 2.5, 1.0)),
        spark_material: glow(LinearRgba::rgb(12.0, 8.0, 3.0)),
    });
}

/// A streak from `from` to `to`, `width` wide.
fn streak(from: Vec3, to: Vec3, width: f32) -> Transform {
    let length = from.distance(to).max(0.01);
    let up = if (to - from).normalize_or_zero().y.abs() > 0.95 { Vec3::X } else { Vec3::Y };
    // The mesh trails behind its head along +Z: its head at `to`, facing
    // on past it, it runs back to `from`.
    Transform::from_translation(to)
        .looking_at(to + (to - from), up)
        .with_scale(Vec3::new(width / 0.05, width / 0.05, length))
}

/// Starts a tracer (or a bolt, beam or arc) for every shot not yet drawn.
#[allow(clippy::too_many_arguments)]
pub fn spawn_tracers(
    mut commands: Commands,
    assets: Res<WeaponFxAssets>,
    spatial: SpatialQuery,
    players: Query<(Entity, &Player, Has<LocalPlayer>, Has<Robot>)>,
    bodies: Query<(Entity, &Position, Has<Robot>), With<Player>>,
    camera: Option<Single<&Transform, With<PlayerCamera>>>,
    mut drawn: Local<HashMap<Entity, u32>>,
) {
    let mut rng = rand::rng();
    for (entity, player, local, robot) in &players {
        let last = drawn.get(&entity).copied();
        let mut newest = last;
        for (at, event) in player.events.iter() {
            if last.is_some_and(|l| at.wrapping_sub(l) as i32 <= 0) {
                continue;
            }
            newest = Some(newest.map_or(at, |n: u32| if at.wrapping_sub(n) as i32 > 0 { at } else { n }));
            let PlayerEvent::Fired {
                hand,
                gun,
                origin,
                velocity,
                kind,
                ..
            } = event
            else {
                continue;
            };
            let Ok(dir) = Dir3::new(velocity) else { continue };
            // Shots leave from the eye, along the crosshair. Drawn from there
            // they'd be dots; instead each starts at the hand it came from
            // (right or left, below the eye) and closes on the line of fire,
            // meeting it CONVERGE metres out.
            let (right, up) = match (local, camera.as_deref()) {
                (true, Some(cam)) => (*cam.right(), *cam.up()),
                _ => {
                    let right = dir.cross(Vec3::Y).try_normalize().unwrap_or(dir.any_orthonormal_vector());
                    (right, right.cross(*dir))
                }
            };
            let side = match hand {
                Hand::Main => 1.0,
                Hand::Off => -1.0,
            };
            let start = origin + *dir * 0.6 + right * side * 0.22 - up * 0.2;
            // What an instant shot meets: the world, or someone.
            let trace = |range: f32| {
                let filter = SpatialQueryFilter::default().with_excluded_entities([entity]);
                match spatial.cast_ray(origin, dir, range, true, &filter) {
                    Some(hit) => (origin + *dir * hit.distance, Some(hit.entity)),
                    None => (origin + *dir * range, None),
                }
            };
            match kind {
                ShotKind::Laser { range } => {
                    let (end, _) = trace(range);
                    commands.spawn((
                        Zap { age: 0.0, life: BEAM_SECONDS },
                        Mesh3d(assets.tracer.clone()),
                        MeshMaterial3d(assets.laser.clone()),
                        streak(start, end, 0.03),
                        bevy::light::NotShadowCaster,
                    ));
                    spark(&mut commands, &assets, end, &assets.laser, SPARK_SECONDS * 0.6, 0.6);
                }
                ShotKind::Arc { range, chain } => {
                    let (end, struck) = trace(range);
                    jagged(&mut commands, &assets, &mut rng, start, end);
                    spark(&mut commands, &assets, end, &assets.arc, SPARK_SECONDS, 2.0);
                    // On to the next targets, as the authority would jump:
                    // the nearest of the other side in reach and in sight.
                    let mut at = end;
                    let mut hit: Vec<Entity> = struck.into_iter().collect();
                    if struck.is_some_and(|s| bodies.contains(s)) {
                        for _ in 0..chain {
                            let world = SpatialQueryFilter::from_mask(Layer::World);
                            let next = bodies
                                .iter()
                                .filter(|(e, _, r)| *r != robot && !hit.contains(e) && *e != entity)
                                .filter(|(_, p, _)| p.0.distance(at) <= ARC_JUMP)
                                .filter(|(_, p, _)| {
                                    let d = p.0 - at;
                                    Dir3::new(d).map_or(true, |dir| spatial.cast_ray(at, dir, d.length(), true, &world).is_none())
                                })
                                .min_by(|a, b| a.1.0.distance(at).total_cmp(&b.1.0.distance(at)));
                            let Some((e, p, _)) = next else { break };
                            jagged(&mut commands, &assets, &mut rng, at, p.0);
                            spark(&mut commands, &assets, p.0, &assets.arc, SPARK_SECONDS, 1.5);
                            hit.push(e);
                            at = p.0;
                        }
                    }
                }
                ShotKind::Bullet { .. } | ShotKind::Plasma { .. } => {
                    let bolt = matches!(kind, ShotKind::Plasma { .. });
                    let meet = origin + *dir * CONVERGE;
                    let speed = velocity.length();
                    // A tracer closes on the line of fire; a bolt does too,
                    // keeping the shooter's velocity that's in its own.
                    let converge = (meet - start).normalize() * speed;
                    let velocity = if bolt { velocity - *dir * speed + converge } else { converge };
                    let (mesh, material, width) = match (bolt, robot, gun) {
                        (true, ..) => (assets.bolt.clone(), assets.plasma.clone(), 1.0),
                        (_, true, _) => (assets.tracer.clone(), assets.robot.clone(), 1.0),
                        (_, _, GunKind::Shotgun) => (assets.tracer.clone(), assets.shotgun.clone(), 0.6),
                        (_, _, GunKind::Sniper) => (assets.tracer.clone(), assets.sniper.clone(), 1.4),
                        (_, _, GunKind::Rifle) => (assets.tracer.clone(), assets.rifle.clone(), 1.0),
                        _ => (assets.tracer.clone(), assets.smg.clone(), 1.0),
                    };
                    let transform = if bolt {
                        Transform::from_translation(start)
                    } else {
                        Transform::from_translation(start).looking_at(meet, up).with_scale(Vec3::new(width, width, 0.01))
                    };
                    commands.spawn((
                        Tracer {
                            shooter: entity,
                            velocity,
                            age: 0.0,
                            travelled: 0.0,
                            bolt,
                        },
                        Mesh3d(mesh),
                        MeshMaterial3d(material),
                        transform,
                        bevy::light::NotShadowCaster,
                    ));
                }
            }
        }
        if let Some(n) = newest {
            drawn.insert(entity, n);
        }
    }
    drawn.retain(|e, _| players.contains(*e));
}

/// A flash where something landed.
fn spark(commands: &mut Commands, assets: &WeaponFxAssets, at: Vec3, material: &Handle<StandardMaterial>, life: f32, grow: f32) {
    commands.spawn((
        Spark { age: 0.0, life, grow },
        Mesh3d(assets.spark.clone()),
        MeshMaterial3d(material.clone()),
        Transform::from_translation(at),
        bevy::light::NotShadowCaster,
    ));
}

/// An arc's bolt from `from` to `to`: a few segments kinked at random.
fn jagged(commands: &mut Commands, assets: &WeaponFxAssets, rng: &mut impl Rng, from: Vec3, to: Vec3) {
    let d = to - from;
    let along = d.normalize_or(Vec3::Z);
    let side = along.any_orthonormal_vector();
    let other = along.cross(side);
    let kinks = 5;
    let jitter = (d.length() * 0.06).clamp(0.05, 0.6);
    let mut last = from;
    for k in 1..=kinks {
        let along = from + d * (k as f32 / kinks as f32);
        let point = if k == kinks {
            to
        } else {
            along + side * rng.random_range(-jitter..jitter) + other * rng.random_range(-jitter..jitter)
        };
        commands.spawn((
            Zap { age: 0.0, life: ARC_SECONDS },
            Mesh3d(assets.tracer.clone()),
            MeshMaterial3d(assets.arc.clone()),
            streak(last, point, 0.025),
            bevy::light::NotShadowCaster,
        ));
        last = point;
    }
}

/// Flies tracers and bolts; one that meets something stops there in a
/// spark (a bolt in a burst).
pub fn fly_tracers(
    mut commands: Commands,
    time: Res<Time>,
    assets: Res<WeaponFxAssets>,
    spatial: SpatialQuery,
    mut tracers: Query<(Entity, &mut Tracer, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (entity, mut tracer, mut transform) in &mut tracers {
        tracer.age += dt;
        let step = tracer.velocity * dt;
        let Ok(dir) = Dir3::new(step) else {
            commands.entity(entity).despawn();
            continue;
        };
        let filter = SpatialQueryFilter::from_mask(Layer::World).with_excluded_entities([tracer.shooter]);
        if let Some(hit) = spatial.cast_ray(transform.translation, dir, step.length(), true, &filter) {
            let point = transform.translation + dir * hit.distance;
            commands.entity(entity).despawn();
            if tracer.bolt {
                spark(&mut commands, &assets, point, &assets.plasma, 0.35, 14.0);
            } else {
                spark(&mut commands, &assets, point, &assets.spark_material, SPARK_SECONDS, 1.5);
            }
            continue;
        }
        if tracer.age > if tracer.bolt { BOLT_LIFE } else { SHOT_LIFE } {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += step;
        tracer.travelled += step.length();
        if !tracer.bolt {
            transform.scale.z = (tracer.velocity.length() * STREAK_SECONDS).min(tracer.travelled).max(0.01);
        }
    }
}

pub fn fade_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut sparks: Query<(Entity, &mut Spark, &mut Transform)>,
    mut zaps: Query<(Entity, &mut Zap)>,
) {
    for (entity, mut spark, mut transform) in &mut sparks {
        spark.age += time.delta_secs();
        if spark.age > spark.life {
            commands.entity(entity).despawn();
        } else {
            transform.scale = Vec3::splat(1.0 + spark.grow * spark.age / spark.life);
        }
    }
    for (entity, mut zap) in &mut zaps {
        zap.age += time.delta_secs();
        if zap.age > zap.life {
            commands.entity(entity).despawn();
        }
    }
}
