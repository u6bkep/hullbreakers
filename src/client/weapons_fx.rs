//! What shooting looks like: tracers from every gun, sparks where they land.
//!
//! Tracers are drawn from the `Fired` entries in each player's event log
//! (predicted for us, replicated for everyone else) and flown here, in
//! straight lines, until they meet something. Where they stop is only for
//! show; the authority decides hits (see `sim::shots`).

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::prelude::*;

use super::LocalPlayer;
use super::camera::PlayerCamera;
use crate::sim::Layer;
use crate::sim::events::PlayerEvent;
use crate::sim::player::Player;
use crate::sim::shots::SHOT_LIFE;
use crate::sim::weapons::{GunKind, Hand};

#[derive(Component)]
pub struct Tracer {
    shooter: Entity,
    velocity: Vec3,
    age: f32,
    /// How far it has come from the hand (m).
    travelled: f32,
}

#[derive(Component)]
pub struct Spark {
    age: f32,
}

#[derive(Resource)]
pub struct WeaponFxAssets {
    tracer: Handle<Mesh>,
    spark: Handle<Mesh>,
    smg: Handle<StandardMaterial>,
    rifle: Handle<StandardMaterial>,
    spark_material: Handle<StandardMaterial>,
}

/// A tracer's streak is as long as this much of its flight (s), but never
/// reaches back past the hand.
const STREAK_SECONDS: f32 = 0.04;
const SPARK_SECONDS: f32 = 0.15;
/// Where a tracer drawn from the hand crosses the line of fire (m).
const CONVERGE: f32 = 40.0;

pub fn setup_weapon_fx(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |materials: &mut Assets<StandardMaterial>, c: LinearRgba| {
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
        smg: glow(&mut materials, LinearRgba::rgb(8.0, 5.0, 1.5)),
        rifle: glow(&mut materials, LinearRgba::rgb(4.0, 7.0, 10.0)),
        spark_material: glow(&mut materials, LinearRgba::rgb(12.0, 8.0, 3.0)),
    });
}

/// Starts a tracer for every shot not yet drawn.
pub fn spawn_tracers(
    mut commands: Commands,
    assets: Res<WeaponFxAssets>,
    players: Query<(Entity, &Player, Has<LocalPlayer>)>,
    camera: Option<Single<&Transform, With<PlayerCamera>>>,
    mut drawn: Local<HashMap<Entity, u32>>,
) {
    for (entity, player, local) in &players {
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
            let meet = origin + *dir * CONVERGE;
            let speed = velocity.length();
            commands.spawn((
                Tracer {
                    shooter: entity,
                    velocity: (meet - start).normalize() * speed,
                    age: 0.0,
                    travelled: 0.0,
                },
                Mesh3d(assets.tracer.clone()),
                MeshMaterial3d(match gun {
                    GunKind::Smg => assets.smg.clone(),
                    GunKind::Rifle => assets.rifle.clone(),
                }),
                Transform::from_translation(start)
                    .looking_at(meet, up)
                    .with_scale(Vec3::new(1.0, 1.0, 0.01)),
                bevy::light::NotShadowCaster,
            ));
        }
        if let Some(n) = newest {
            drawn.insert(entity, n);
        }
    }
    drawn.retain(|e, _| players.contains(*e));
}

/// Flies tracers; one that meets something stops there in a spark.
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
            commands.spawn((
                Spark { age: 0.0 },
                Mesh3d(assets.spark.clone()),
                MeshMaterial3d(assets.spark_material.clone()),
                Transform::from_translation(point),
                bevy::light::NotShadowCaster,
            ));
            continue;
        }
        if tracer.age > SHOT_LIFE {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += step;
        tracer.travelled += step.length();
        transform.scale.z = (tracer.velocity.length() * STREAK_SECONDS).min(tracer.travelled).max(0.01);
    }
}

pub fn fade_sparks(mut commands: Commands, time: Res<Time>, mut sparks: Query<(Entity, &mut Spark, &mut Transform)>) {
    for (entity, mut spark, mut transform) in &mut sparks {
        spark.age += time.delta_secs();
        if spark.age > SPARK_SECONDS {
            commands.entity(entity).despawn();
        } else {
            transform.scale = Vec3::splat(1.0 + 1.5 * spark.age / SPARK_SECONDS);
        }
    }
}
