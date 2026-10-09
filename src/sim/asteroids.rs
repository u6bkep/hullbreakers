//! Procedural asteroid field streaming past the ship.

use avian3d::prelude::*;
use bevy::prelude::*;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::SimMaterial;
use super::bodies::PreviousVelocity;
use super::ship::Ship;
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

#[derive(Component)]
pub struct Asteroid {
    pub radius: f32,
}

/// Recycling must not teleport a rock the player is attached to.
#[derive(Component)]
pub struct Pinned;

#[derive(Resource)]
pub struct FieldRng(pub ChaCha8Rng);

/// The field velocity actually applied to rocks, so a change to
/// `Tuning::field_speed` can be applied as a delta.
#[derive(Resource)]
pub struct AppliedFieldSpeed(pub f32);

/// Builds a lumpy rock as a compound of 1–3 noisy convex lobes.
pub fn asteroid_collider(rng: &mut impl Rng, radius: f32) -> Collider {
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

/// A position in the field box (relative to `center`), keeping big rocks out
/// of the ship's flight corridor. `z_range` selects the slab to spawn into.
fn random_field_position(
    rng: &mut impl Rng,
    center: Vec3,
    radius: f32,
    z_range: (f32, f32),
) -> Vec3 {
    loop {
        let x = rng.random_range(-FIELD_WIDTH / 2.0..FIELD_WIDTH / 2.0);
        let y = rng.random_range(-FIELD_WIDTH / 2.0..FIELD_WIDTH / 2.0);
        let lateral = Vec2::new(x, y).length();
        if radius > CORRIDOR_MIN_RADIUS && lateral < CORRIDOR_RADIUS + radius {
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
    let mut rng = ChaCha8Rng::seed_from_u64(0x5ace_5eed);
    for i in 0..ASTEROID_COUNT {
        let radius = random_radius(&mut rng);
        let position = random_field_position(
            &mut rng,
            Vec3::ZERO,
            radius,
            (-FIELD_LENGTH / 2.0, FIELD_LENGTH / 2.0),
        );
        let (linear, angular) = drift(&mut rng, tuning.field_speed);
        commands.spawn((
            Name::new(format!("Asteroid {i}")),
            Asteroid { radius },
            asteroid_collider(&mut rng, radius),
            ColliderDensity(ROCK_DENSITY),
            RigidBody::Dynamic,
            SleepingDisabled,
            PreviousVelocity::default(),
            LinearVelocity(linear),
            AngularVelocity(angular),
            Transform::from_translation(position).with_rotation(Quat::from_scaled_axis(
                random_unit(&mut rng) * rng.random_range(0.0..3.14),
            )),
            SimMaterial::Rock,
        ));
    }
    commands.insert_resource(FieldRng(rng));
    commands.insert_resource(AppliedFieldSpeed(tuning.field_speed));
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

/// Rocks that leave the box around the ship are respawned on the far side
/// with fresh lateral positions and drift.
pub fn recycle_field(
    tuning: Res<Tuning>,
    mut rng: ResMut<FieldRng>,
    ship: Single<&Position, With<Ship>>,
    mut rocks: Query<
        (
            &Asteroid,
            &mut Position,
            &mut LinearVelocity,
            &mut AngularVelocity,
        ),
        (Without<Ship>, Without<Pinned>),
    >,
) {
    let center = ship.0;
    let half_l = FIELD_LENGTH / 2.0;
    let half_w = FIELD_WIDTH / 2.0;
    for (rock, mut pos, mut lin, mut ang) in &mut rocks {
        let rel = pos.0 - center;
        let out_lateral = rel.x.abs() > half_w + rock.radius || rel.y.abs() > half_w + rock.radius;
        let downstream = rel.dot(FIELD_DIRECTION) > half_l + rock.radius;
        let upstream = rel.dot(FIELD_DIRECTION) < -half_l - rock.radius;
        if !(out_lateral || downstream || upstream) {
            continue;
        }
        // Re-enter at the upstream end, in a thin slab so rocks don't pop in close.
        pos.0 = random_field_position(&mut rng.0, center, rock.radius, (-half_l, -half_l + 60.0));
        let (linear, angular) = drift(&mut rng.0, tuning.field_speed);
        lin.0 = linear;
        ang.0 = angular;
    }
}
