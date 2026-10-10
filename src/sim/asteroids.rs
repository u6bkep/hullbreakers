//! Procedural asteroid field streaming past the ship.

use avian3d::prelude::*;
use bevy::prelude::*;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::SimMaterial;
use super::bodies::{PreviousVelocity, dynamic_mass};
use super::tuning::Tuning;

pub const FIELD_LENGTH: f32 = 800.0;
pub const FIELD_WIDTH: f32 = 400.0;
pub const ASTEROID_COUNT: usize = 300;
/// Rocks bigger than this keep out of the ship's flight corridor.
const CORRIDOR_MIN_RADIUS: f32 = 3.0;
const CORRIDOR_RADIUS: f32 = 30.0;
pub const ROCK_DENSITY: f32 = 2000.0;

/// The field streams along +Z (the ship faces -Z).
pub const FIELD_DIRECTION: Vec3 = Vec3::Z;

/// A rock. Its shape is generated from `seed` wherever the component appears
/// (see [`shape_asteroid`]), so replicating this is replicating the rock.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asteroid {
    pub radius: f32,
    pub seed: u64,
}

/// Recycling must not teleport a rock the player is attached to.
#[derive(Component)]
pub struct Pinned;

/// Field generation state: the random stream and the next rock's number.
#[derive(Resource)]
pub struct FieldRng {
    rng: ChaCha8Rng,
    next: u32,
}

/// The field velocity actually applied to rocks, so a change to
/// `Tuning::field_speed` can be applied as a delta.
#[derive(Resource)]
pub struct AppliedFieldSpeed(pub f32);

/// Gives a new [`Asteroid`] its collider.
pub fn shape_asteroid(add: On<Add, Asteroid>, rocks: Query<&Asteroid>, mut commands: Commands) {
    let rock = rocks.get(add.entity).expect("just added");
    let collider = asteroid_collider(rock.seed, rock.radius);
    commands.entity(add.entity).insert((
        dynamic_mass(&collider, ROCK_DENSITY),
        collider,
        SimMaterial::Rock,
    ));
}

/// Builds a lumpy rock as a compound of 1–3 noisy convex lobes.
pub fn asteroid_collider(seed: u64, radius: f32) -> Collider {
    let rng = &mut ChaCha8Rng::seed_from_u64(seed);
    let lobe_count = if radius < 1.0 {
        1
    } else {
        rng.random_range(1..=3)
    };
    let mut lobes = Vec::new();
    for lobe in 0..lobe_count {
        let lobe_radius = if lobe == 0 {
            radius
        } else {
            radius * rng.random_range(0.5..0.8)
        };
        let offset = if lobe == 0 {
            Vec3::ZERO
        } else {
            random_unit(rng) * radius * rng.random_range(0.5..0.8)
        };
        let squash = Vec3::new(1.0, rng.random_range(0.65..1.0), rng.random_range(0.6..1.0));
        let fbm = Fbm::<Perlin>::new(rng.random())
            .set_octaves(4)
            .set_frequency(1.3);
        let sphere = Sphere::new(1.0)
            .mesh()
            .ico(3)
            .expect("ico subdivisions in range");
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            sphere.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            unreachable!("ico sphere has float3 positions");
        };
        let points: Vec<Vec3> = positions
            .iter()
            .map(|p| {
                let dir = Vec3::from(*p);
                let n = fbm.get([dir.x as f64, dir.y as f64, dir.z as f64]) as f32;
                dir * squash * lobe_radius * (1.0 + 0.35 * n)
            })
            .collect();
        let hull = Collider::convex_hull(points).expect("noisy sphere has a hull");
        lobes.push((offset, Quat::IDENTITY, hull));
    }
    if lobes.len() == 1 {
        lobes.pop().unwrap().2
    } else {
        Collider::compound(lobes)
    }
}

fn random_unit(rng: &mut impl Rng) -> Vec3 {
    loop {
        let v = Vec3::new(
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
        );
        let len2 = v.length_squared();
        if len2 > 1e-3 && len2 <= 1.0 {
            return v / len2.sqrt();
        }
    }
}

/// Many small rocks, few big ones.
fn random_radius(rng: &mut impl Rng) -> f32 {
    let (min, max) = (0.35f32, 30.0f32);
    min * (max / min).powf(rng.random::<f32>().powi(3))
}

/// A position in the field box (relative to `center`). With `corridor`, big
/// rocks keep out of the ship's flight line. `z_range` selects the slab.
fn random_field_position(
    rng: &mut impl Rng,
    center: Vec3,
    radius: f32,
    corridor: bool,
    z_range: (f32, f32),
) -> Vec3 {
    loop {
        let x = rng.random_range(-FIELD_WIDTH / 2.0..FIELD_WIDTH / 2.0);
        let y = rng.random_range(-FIELD_WIDTH / 2.0..FIELD_WIDTH / 2.0);
        let lateral = Vec2::new(x, y).length();
        if corridor && radius > CORRIDOR_MIN_RADIUS && lateral < CORRIDOR_RADIUS + radius {
            continue;
        }
        let z = rng.random_range(z_range.0..z_range.1);
        return center + Vec3::new(x, y, z);
    }
}

fn drift(rng: &mut impl Rng, field_speed: f32) -> (Vec3, Vec3) {
    let linear = FIELD_DIRECTION * field_speed + random_unit(rng) * rng.random_range(0.0..0.6);
    let angular = random_unit(rng) * rng.random_range(0.0..0.3);
    (linear, angular)
}

pub fn spawn_field(mut commands: Commands, tuning: Res<Tuning>) {
    let mut field = FieldRng {
        rng: ChaCha8Rng::seed_from_u64(0x5ace_5eed),
        next: 0,
    };
    for _ in 0..ASTEROID_COUNT {
        let radius = random_radius(&mut field.rng);
        let position = random_field_position(
            &mut field.rng,
            Vec3::ZERO,
            radius,
            true,
            (-FIELD_LENGTH / 2.0, FIELD_LENGTH / 2.0),
        );
        spawn_rock(&mut commands, &mut field, radius, position, tuning.field_speed);
    }
    commands.insert_resource(field);
    commands.insert_resource(AppliedFieldSpeed(tuning.field_speed));
}

fn spawn_rock(
    commands: &mut Commands,
    field: &mut FieldRng,
    radius: f32,
    position: Vec3,
    field_speed: f32,
) {
    let rng = &mut field.rng;
    let (linear, angular) = drift(rng, field_speed);
    let rotation = Quat::from_scaled_axis(random_unit(rng) * rng.random_range(0.0..3.14));
    commands.spawn((
        Name::new(format!("Asteroid {}", field.next)),
        Asteroid {
            radius,
            seed: rng.random(),
        },
        RigidBody::Dynamic,
        SleepingDisabled,
        PreviousVelocity::default(),
        LinearVelocity(linear),
        AngularVelocity(angular),
        // Physics state, not a Transform: Position is what simulates (and
        // replicates); Transform is derived from it for rendering.
        Position(position),
        Rotation(rotation),
    ));
    field.next += 1;
}

/// Applies changes to `Tuning::field_speed` as a velocity delta to every rock.
pub fn apply_field_speed(
    tuning: Res<Tuning>,
    mut applied: ResMut<AppliedFieldSpeed>,
    mut rocks: Query<&mut LinearVelocity, With<Asteroid>>,
) {
    let delta = tuning.field_speed - applied.0;
    if delta == 0.0 {
        return;
    }
    applied.0 = tuning.field_speed;
    for mut v in &mut rocks {
        v.0 += FIELD_DIRECTION * delta;
    }
}

/// A place the field must be populated around: the ship, and every player
/// (who may ride a rock or fly far from the ship).
#[derive(Clone, Copy, Debug)]
pub struct InterestPoint {
    pub position: Vec3,
    /// Keep big rocks out of this point's flight corridor (the ship's).
    pub corridor: bool,
}

/// Interest points for this tick, gathered before recycling.
#[derive(Resource, Default)]
pub struct FieldInterest(pub Vec<InterestPoint>);

/// Respawned rocks never appear closer than this to any interest point.
const SPAWN_CLEARANCE: f32 = 250.0;

fn in_box(rel: Vec3, radius: f32) -> bool {
    let half_l = FIELD_LENGTH / 2.0 + radius;
    let half_w = FIELD_WIDTH / 2.0 + radius;
    rel.x.abs() <= half_w && rel.y.abs() <= half_w && rel.dot(FIELD_DIRECTION).abs() <= half_l
}

/// Rocks outside the box around every interest point are replaced by new
/// ones at the upstream end of one of them, out of sight of all of them. A
/// new rock rather than a moved one: to anything watching (a network client
/// interpolating it) a rock that jumps 800 m is a streak across the sky.
pub fn recycle_field(
    mut commands: Commands,
    tuning: Res<Tuning>,
    interest: Res<FieldInterest>,
    mut field: ResMut<FieldRng>,
    rocks: Query<(Entity, &Asteroid, &Position), Without<Pinned>>,
) {
    let points = &interest.0;
    if points.is_empty() {
        return;
    }
    let half_l = FIELD_LENGTH / 2.0;
    for (entity, rock, pos) in &rocks {
        if points
            .iter()
            .any(|p| in_box(pos.0 - p.position, rock.radius))
        {
            continue;
        }
        let mut candidate = pos.0;
        for _ in 0..8 {
            let p = points[field.rng.random_range(0..points.len())];
            candidate = random_field_position(
                &mut field.rng,
                p.position,
                rock.radius,
                p.corridor,
                (-half_l, -half_l + 60.0),
            );
            if points
                .iter()
                .all(|q| q.position.distance(candidate) > SPAWN_CLEARANCE)
            {
                break;
            }
        }
        commands.entity(entity).despawn();
        spawn_rock(
            &mut commands,
            &mut field,
            rock.radius,
            candidate,
            tuning.field_speed,
        );
    }
}
