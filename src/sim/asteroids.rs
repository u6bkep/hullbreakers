//! Procedural asteroid field streaming past the ship.

use std::collections::HashMap;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_replicon::prelude::Signature;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::SimMaterial;
use super::bodies::{BodyInertia, PreviousVelocity, dynamic_mass};
use super::mission::Director;
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
/// (see [`shape_asteroids`]), so replicating this is replicating the rock.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asteroid {
    pub radius: f32,
    pub seed: u64,
    /// Fixed in place (a static body; a race's anchors): nothing can move
    /// it, so it has no mass to push against. Replicated with the rock,
    /// since a client makes every rock it's sent static (it doesn't
    /// simulate them) and can't tell otherwise.
    pub fixed: bool,
}

/// A field of rocks laid out from a seed, grown wherever this component
/// appears (on the body it belongs to: a race's course). A field of
/// thousands reaches a client as these few bytes rather than a rock at a
/// time (about 160 B each on the wire: 6000 took a joining client a second,
/// 32,000 timed it out; docs/perf.md). Every peer grows the same rocks in
/// the same order ([`grow_fields`]): the authority as bodies, a client as
/// shapes to stand on. Each is signed (replicon's [`Signature`]) from the
/// field and its index, so when the authority does replicate one (once
/// something happens to it: a rope, a boot, a knock; see `net`) it lands on
/// the client's own copy. Until then nothing of it is sent.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RockField {
    pub layout: FieldLayout,
    pub seed: u64,
    pub count: u32,
    /// The tick it was laid, in its rocks' signatures: a field laid after
    /// another with the same layout and seed (the race run again) must not
    /// sign its rocks the same, in case a client still has the old one.
    pub laid: u32,
}

/// How a [`RockField`]'s rocks are placed (see `scenarios`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldLayout {
    GrappleRun,
    /// A scene's field (see `sim::scene`), round the sites of that seed.
    Scene(super::scene::SceneSeed),
}

/// Where one of a field's rocks goes, and how it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RockPlace {
    pub at: Vec3,
    pub radius: f32,
    /// Immovable (an anchor rocks are swung round); the rest of a field is
    /// at rest but can be knocked adrift.
    pub fixed: bool,
    /// Adrift from the start (a mover, stirring the field as it goes):
    /// its velocity (m/s) and spin (rad/s). Zero for a rock at rest.
    pub velocity: Vec3,
    pub spin: Vec3,
}

impl RockPlace {
    /// A rock at rest.
    pub fn still(at: Vec3, radius: f32, fixed: bool) -> Self {
        Self {
            at,
            radius,
            fixed,
            velocity: Vec3::ZERO,
            spin: Vec3::ZERO,
        }
    }
}

/// A rock grown from a [`RockField`]: which, and its index in its places.
#[derive(Component, Clone, Copy, Debug)]
pub struct Grown {
    pub field: Entity,
    pub index: u32,
}

/// A grown rock's signature: the same on every peer, different from every
/// other rock's (of this field or one laid before it).
fn rock_signature(field: &RockField, index: u32) -> u64 {
    // splitmix64's finalizer.
    fn mix(mut x: u64) -> u64 {
        x ^= x >> 30;
        x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^ (x >> 31)
    }
    mix(mix(field.seed ^ ((field.laid as u64) << 32) ^ field.layout.tag()) ^ index as u64)
}

/// Grows every new [`RockField`]'s rocks. Each becomes a body as it's shaped
/// ([`shape_asteroids`], a batch a frame). On the authority (which has the
/// [`Director`]) a rock that isn't fixed is a dynamic body: born awake,
/// since a body spawned asleep is woken by its own first step, it falls
/// asleep on its own, and sleeps until something knocks it (Avian wakes it
/// on contact and on a change to its velocity). A mover has its velocity
/// from the start, so it's replicated from the start, and reaches a client
/// as its motion on the client's own copy, which is grown at rest.
///
/// A rock the authority has already replicated to this client (a mover's,
/// or one touched before the client joined) may be here before the field
/// is grown: its signature then had nothing to map onto, and it arrived as
/// a rock of its own. Growing the field finds it by its seed (every rock's
/// is its own) and makes it the grown one, rather than growing a twin.
pub fn grow_fields(
    fields: Query<(Entity, &RockField), Added<RockField>>,
    arrived: Query<(Entity, &Asteroid), Without<Grown>>,
    director: Option<Res<Director>>,
    mut commands: Commands,
) {
    for (entity, field) in &fields {
        let places = field.layout.places(field.seed, field.count as usize);
        info!("growing a field of {} rocks ({:?})", places.len(), field.layout);
        let arrived: HashMap<u64, Entity> = arrived.iter().map(|(e, a)| (a.seed, e)).collect();
        let rng = &mut ChaCha8Rng::seed_from_u64(field.seed);
        for (index, place) in places.iter().enumerate() {
            let rotation = Quat::from_scaled_axis(random_unit(rng) * rng.random_range(0.0..3.14));
            let seed: u64 = rng.random();
            let grown = Grown {
                field: entity,
                index: index as u32,
            };
            let signature = Signature::from_hash(rock_signature(field, index as u32));
            if let Some(&rock) = arrived.get(&seed) {
                commands.entity(rock).insert((grown, signature));
                continue;
            }
            let (velocity, spin) = if director.is_some() { (place.velocity, place.spin) } else { (Vec3::ZERO, Vec3::ZERO) };
            commands.spawn((
                Name::new(format!("Rock {index}")),
                Asteroid {
                    radius: place.radius,
                    seed,
                    fixed: place.fixed,
                },
                grown,
                signature,
                PreviousVelocity::default(),
                LinearVelocity(velocity),
                AngularVelocity(spin),
                Position(place.at),
                Rotation(rotation),
                // A client's physics doesn't write transforms (its bodies
                // are posed from their motion, and these have none).
                Transform::from_translation(place.at).with_rotation(rotation),
            ));
        }
    }
}

/// Authority: a grown rock knocked adrift (a dynamic one that's moving at
/// all) never sleeps again. A rock adrift in space doesn't stop, and Avian
/// would stop one drifting slower than its sleep threshold (0.15 m/s).
pub fn keep_knocked_adrift(
    rocks: Query<(Entity, &RigidBody, &LinearVelocity, &AngularVelocity), (With<Grown>, Without<SleepingDisabled>)>,
    mut commands: Commands,
) {
    for (rock, body, linear, angular) in &rocks {
        if body.is_dynamic() && (linear.0 != Vec3::ZERO || angular.0 != Vec3::ZERO) {
            commands.entity(rock).insert(SleepingDisabled);
        }
    }
}

/// A field's rocks go with it (on a client, that's how it hears the
/// scenario's over; the authority clears them with the rest of the world).
pub fn clear_field(despawn: On<Despawn, RockField>, rocks: Query<(Entity, &Grown)>, mut commands: Commands) {
    for (rock, grown) in &rocks {
        if grown.field == despawn.entity {
            commands.entity(rock).try_despawn();
        }
    }
}

/// A rock of the streaming field (not one set about a battle), new to it
/// when this is added: fauna may come with it (see `fauna`).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct FieldRock;

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

/// How many rocks [`shape_asteroids`] shapes a frame: a field of thousands
/// takes a few dozen frames, so the frame stays alive (and a loading
/// veil's bar moves) instead of stalling for the lot. Per frame, not per
/// tick: a long tick makes the fixed loop run several ticks the next frame
/// to catch up, so a budget per tick bounded nothing (traced: 32,000 rocks
/// in batches of 1000 a tick ran 24 ticks inside three frames, one of them
/// 2.5 s). A batch costs about a tenth of a millisecond a rock, most of it
/// applying the shape (Avian's collider and mass hooks, the mesh), so this
/// is about 25 ms a frame. A count, not a time, so the order rocks enter
/// the physics is the same on every run.
pub const SHAPE_PER_FRAME: usize = 200;

/// What's left of this frame's [`SHAPE_PER_FRAME`].
#[derive(Resource)]
pub struct ShapeBudget(pub usize);

impl Default for ShapeBudget {
    fn default() -> Self {
        Self(SHAPE_PER_FRAME)
    }
}

/// Each frame starts with a fresh budget.
pub fn reset_shape_budget(mut budget: ResMut<ShapeBudget>) {
    budget.0 = SHAPE_PER_FRAME;
}

/// Gives new [`Asteroid`]s their colliders, a batch a frame
/// ([`SHAPE_PER_FRAME`]), each batch on every core: a rock's hulls take
/// about 0.2 ms to build, so a field of thousands arriving together (a
/// scenario's, or a client's on joining) built one by one stalled a frame
/// for seconds (8000 rocks: 3.5 s, which timed a joining client out; see
/// docs/perf.md). Runs every tick before anything that needs a rock's
/// shape (the fauna setting on new field rocks, the physics step), so a
/// rock spawned or received since is shaped by then. They're built in
/// parallel but given out in the query's order, which is the same on every
/// run: the order bodies enter the physics decides how it steps them, and
/// the sim must be deterministic.
///
/// A grown rock (see [`RockField`]) becomes a body here, with its shape:
/// a dynamic body without one would spend the ticks until its batch
/// massless (which Avian warns about, per body). On the authority (which
/// has the [`Director`]) a rock that isn't fixed is dynamic; a client's are
/// all static, since it doesn't simulate rocks, and one that moves reaches
/// it by its motion.
pub fn shape_asteroids(
    rocks: Query<(Entity, &Asteroid, Has<Grown>), Without<Collider>>,
    director: Option<Res<Director>>,
    mut budget: ResMut<ShapeBudget>,
    mut commands: Commands,
) {
    let todo: Vec<(Entity, Asteroid, bool)> = rocks.iter().take(budget.0).map(|(e, r, g)| (e, *r, g)).collect();
    if todo.is_empty() {
        return;
    }
    budget.0 -= todo.len();
    let pool = bevy::tasks::ComputeTaskPool::get();
    let chunk = todo.len().div_ceil(pool.thread_num().max(1) * 4).max(1);
    let shapes: Vec<Collider> = pool
        .scope(|scope| {
            for rocks in todo.chunks(chunk) {
                scope.spawn(async move { rocks.iter().map(|(_, r, _)| asteroid_collider(r.seed, r.radius)).collect::<Vec<_>>() });
            }
        })
        .into_iter()
        .flatten()
        .collect();
    for ((entity, rock, grown), collider) in todo.into_iter().zip(shapes) {
        let mut e = commands.entity(entity);
        // A fixed rock must take no impulses: a static body's velocity is
        // never stepped or cleared, so a bump would leave it "moving" for
        // good (and sent to clients as moving, and handed on to whoever
        // stands on it).
        if rock.fixed {
            e.insert(BodyInertia::immovable(&collider));
        } else {
            e.insert(dynamic_mass(&collider, ROCK_DENSITY));
        }
        if grown {
            e.insert(if director.is_some() && !rock.fixed { RigidBody::Dynamic } else { RigidBody::Static });
        }
        e.insert((collider, SimMaterial::Rock));
    }
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

/// A direction, uniformly.
pub fn random_unit(rng: &mut impl Rng) -> Vec3 {
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

pub fn spawn_field(commands: &mut Commands, tuning: &Tuning) {
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
        let (linear, angular) = drift(&mut field.rng, tuning.field_speed);
        let rock = spawn_rock(commands, &mut field, RigidBody::Dynamic, radius, position, linear, angular);
        commands.entity(rock).insert(FieldRock);
    }
    commands.insert_resource(field);
    commands.insert_resource(AppliedFieldSpeed(tuning.field_speed));
}

/// Rocks scattered around a battle, at rest and slowly turning, kept clear
/// of where the ships fly. Each is placed near one of `near` (points the
/// ships pass through) so the rocks are where the crew needs handholds;
/// `clear` says whether a rock of some radius at some point is out of the
/// ships' way.
pub fn spawn_battlefield(
    commands: &mut Commands,
    near: &[Vec3],
    count: usize,
    clear: impl Fn(Vec3, f32) -> bool,
) {
    let mut field = FieldRng {
        rng: ChaCha8Rng::seed_from_u64(0xba77_1e),
        next: 0,
    };
    let mut placed = 0;
    // Bounded, in case the ships leave little room.
    for _ in 0..count * 20 {
        if placed == count {
            break;
        }
        let rng = &mut field.rng;
        let radius = random_radius(rng);
        let around = near[rng.random_range(0..near.len())];
        let position = around + random_unit(rng) * rng.random_range(30.0..260.0);
        if !clear(position, radius) {
            continue;
        }
        let angular = random_unit(rng) * rng.random_range(0.0..0.2);
        spawn_rock(commands, &mut field, RigidBody::Dynamic, radius, position, Vec3::ZERO, angular);
        placed += 1;
    }
}

/// Rocks at rest, slowly turning, at the given places (centre, radius),
/// shaped and turned from `seed`. Not part of a field: nothing recycles
/// them.
pub fn spawn_rocks(commands: &mut Commands, seed: u64, rocks: &[(Vec3, f32)]) {
    let mut field = FieldRng {
        rng: ChaCha8Rng::seed_from_u64(seed),
        next: 0,
    };
    for &(position, radius) in rocks {
        let angular = random_unit(&mut field.rng) * field.rng.random_range(0.0..0.08);
        spawn_rock(commands, &mut field, RigidBody::Dynamic, radius, position, Vec3::ZERO, angular);
    }
}

fn spawn_rock(
    commands: &mut Commands,
    field: &mut FieldRng,
    body: RigidBody,
    radius: f32,
    position: Vec3,
    linear: Vec3,
    angular: Vec3,
) -> Entity {
    let rng = &mut field.rng;
    let rotation = Quat::from_scaled_axis(random_unit(rng) * rng.random_range(0.0..3.14));
    let mut rock = commands.spawn((
        Name::new(format!("Asteroid {}", field.next)),
        Asteroid {
            radius,
            seed: rng.random(),
            fixed: body.is_static(),
        },
        body,
        PreviousVelocity::default(),
        LinearVelocity(linear),
        AngularVelocity(angular),
        // Physics state, not a Transform: Position is what simulates (and
        // replicates); Transform is derived from it for rendering.
        Position(position),
        Rotation(rotation),
    ));
    if body.is_dynamic() {
        rock.insert(SleepingDisabled);
    }
    field.next += 1;
    rock.id()
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
        let (linear, angular) = drift(&mut field.rng, tuning.field_speed);
        let rock = spawn_rock(&mut commands, &mut field, RigidBody::Dynamic, rock.radius, candidate, linear, angular);
        commands.entity(rock).insert(FieldRock);
    }
}
