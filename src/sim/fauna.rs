//! Life in the asteroid field.
//!
//! Two kinds, each made of the game's physics rather than around it:
//!
//! - The **Limpet** ([`Robot::Limpet`], brain in `bots::limpet`): a rock
//!   mimic. It's a body on boots with a tendril for a grapple, so it feels
//!   the crew through the rock it clings to, and what it does to whoever it
//!   catches is ropes and mass: reeled in, ridden, hauled about, torn off by
//!   a crewmate's winch.
//! - The **Grazer** ([`Grazer`]): a big, harmless drifting creature that
//!   swims bell-first in slow pulses from rock to rock, feeding on them (it
//!   tugs at a rock it grazes, so a small one wanders). It's a rigid body,
//!   so a crew member can stand on its back and ride, or hook it, which
//!   startles it: a startled grazer bolts, towing whoever hooked it, and
//!   its alarm spreads through its herd into a stampede that tramples
//!   whoever's in its way and scatters small rocks. It changes its own
//!   motion only in its pulses, so between them it's in free flight and the
//!   network has nothing to send (see `net::extrapolate`); and its pulses
//!   come on a schedule every peer knows, so they're drawn on time.
//!
//! The field is populated as it streams in: a new rock upstream sometimes
//! brings a limpet on it, or a herd grazing it, and fauna that drift out of
//! the field around the crew go with the rocks.

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::asteroids::{Asteroid, FieldInterest, FieldRock, FIELD_LENGTH};
use super::bodies::{Bodies, BodyInertia, PreviousVelocity, dynamic_mass};
use super::bots::{Affixes, Detonation, Robot, Wrecked, new_robot};
use super::events::PlayerEvent;
use super::mission::Staged;
use super::player::{Mode, Player};
use super::ship::HomeShip;
use super::shots::{Team, WeakSpot};
use super::tuning::Tuning;
use super::vitals::{Damage, Vitals};
use super::{SimTick, TICK_HZ};

/// A grazer. Every peer has this (it's what's replicated); the authority
/// also keeps its [`GrazerMind`].
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grazer {
    /// Varies its timing and where round a rock it feeds.
    pub seed: u32,
    pub mood: Mood,
    /// The tick its mood began: its pulses count from here.
    pub since: u32,
    /// On show (the gallery's): it pulses in place and nothing moves it.
    pub dormant: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mood {
    /// Swimming to a rock to graze.
    Cruising,
    /// Feeding at a rock.
    Grazing,
    /// Bolting from something.
    Startled,
}

impl Grazer {
    /// Ticks between its pulses.
    pub fn period(&self) -> u32 {
        let (base, jitter) = match self.mood {
            Mood::Cruising => (150, 30),
            Mood::Grazing => (200, 40),
            Mood::Startled => (33, 6),
        };
        base + self.seed % jitter
    }

    /// The tick of its latest pulse at or before `tick` (it pulses on the
    /// tick its mood begins, then every [`period`](Self::period)).
    pub fn last_pulse(&self, tick: u32) -> u32 {
        let age = tick.wrapping_sub(self.since);
        if age > u32::MAX / 2 {
            // Before its mood began (a client a tick behind).
            return self.since;
        }
        self.since.wrapping_add(age / self.period() * self.period())
    }

    fn set_mood(&mut self, mood: Mood, tick: u32) {
        self.mood = mood;
        self.since = tick;
    }
}

/// What a grazer is up to: authority only.
#[derive(Component, Clone, Debug, Default)]
pub struct GrazerMind {
    /// The rock it's grazing or making for, and where round it it feeds
    /// (a direction from the rock, in the world: it keeps to the sunny
    /// side whichever way the rock turns).
    rock: Option<Entity>,
    station: Vec3,
    /// Seconds left grazing, or startled.
    left: f32,
    /// What it's bolting from.
    threat: Vec3,
    /// The velocity of the rocks about it, last it knew (the stream it
    /// drifts in).
    stream: Vec3,
}

/// The bell's shape: an ellipsoid with these radii (m) about its centre,
/// +Y the way it swims (where a rider stands).
pub const GRAZER_RADII: Vec3 = Vec3::new(1.6, 0.7, 1.6);
/// Its mass (kg).
const GRAZER_MASS: f32 = 500.0;
const GRAZER_HEALTH: f32 = 160.0;
/// Its heart, in the bell's cavity (SOCKET_weak), and its size (m).
const GRAZER_HEART: Vec3 = Vec3::new(0.0, -0.3, 0.0);
const GRAZER_HEART_RADIUS: f32 = 0.28;

/// Every peer: a grazer gets its body from its shape, as a rock does.
pub fn shape_grazer(add: On<Add, Grazer>, mut commands: Commands) {
    let collider = grazer_collider();
    let volume = 4.0 / 3.0 * std::f32::consts::PI * GRAZER_RADII.x * GRAZER_RADII.y * GRAZER_RADII.z;
    commands.entity(add.entity).insert((dynamic_mass(&collider, GRAZER_MASS / volume), collider));
}

fn grazer_collider() -> Collider {
    let sphere = Sphere::new(1.0).mesh().ico(2).expect("ico subdivisions in range");
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(points)) = sphere.attribute(Mesh::ATTRIBUTE_POSITION) else {
        unreachable!("ico sphere has float3 positions");
    };
    let points: Vec<Vec3> = points.iter().map(|p| Vec3::from(*p) * GRAZER_RADII).collect();
    Collider::convex_hull(points).expect("an ellipsoid has a hull")
}

/// A grazer at `position`, swimming toward `rotation * Y`, moving at
/// `velocity`, in `mood` since `since`.
pub fn grazer(seed: u32, mood: Mood, since: u32, position: Vec3, rotation: Quat, velocity: Vec3, dormant: bool) -> impl Bundle {
    (
        Name::new("Grazer"),
        Grazer { seed, mood, since, dormant },
        GrazerMind {
            stream: velocity,
            ..default()
        },
        RigidBody::Dynamic,
        SleepingDisabled,
        PreviousVelocity::default(),
        Position(position),
        Rotation(rotation),
        LinearVelocity(velocity),
        AngularVelocity::default(),
        Vitals {
            health: GRAZER_HEALTH,
            since_hurt: f32::INFINITY,
        },
        WeakSpot {
            at: GRAZER_HEART,
            radius: GRAZER_HEART_RADIUS,
        },
        Staged,
    )
}

/// One for the gallery: at rest, pulsing in place.
pub fn spawn_exhibit(commands: &mut Commands, at: Vec3, rotation: Quat) {
    commands.spawn(grazer(7, Mood::Grazing, 0, at, rotation, Vec3::ZERO, true));
}

// --- Swimming -------------------------------------------------------------

/// How fast it swims (m/s, relative to the stream): to a rock, holding its
/// place at one, and bolting.
const CRUISE_SPEED: f32 = 3.5;
const GRAZE_SPEED: f32 = 0.8;
const BOLT_SPEED: f32 = 12.0;
/// The most one pulse changes its speed (m/s), by mood.
const CRUISE_PULSE: f32 = 1.2;
const GRAZE_PULSE: f32 = 0.4;
const BOLT_PULSE: f32 = 3.0;
/// The fastest it turns (rad/s), and the most one pulse changes that
/// (rad/s): gently, so a rider keeps their feet; bolting, not so.
const TURN: f32 = 0.6;
const TURN_CHANGE: f32 = 0.35;
const BOLT_TURN: f32 = 1.5;
const BOLT_TURN_CHANGE: f32 = 0.8;
/// How far off its bell's axis a pulse can push it (rad): it squeezes one
/// side of its bell harder to steer.
const STEER_CONE: f32 = 0.6;
/// What it counts on to slow down (m/s²): it eases off in time to stop
/// where it's going.
const EASE: f32 = 0.25;
/// It feeds this far off a rock's surface (m, from its centre), for this
/// long (s).
const GRAZE_GAP: f32 = 2.3;
const GRAZE_SECONDS: (f32, f32) = (20.0, 40.0);
/// The hardest it tugs at a rock it's feeding on, per pulse (N·s).
const TUG: f32 = 260.0;
/// The rocks it feeds on (radius, m), and how far it looks for one (m).
const FOOD: (f32, f32) = (0.6, 5.0);
const FORAGE: f32 = 150.0;

/// Authority, before the physics step: every grazer swims, changing its
/// motion only on its pulses.
pub fn swim(
    tick: Res<SimTick>,
    time: Res<Time>,
    mut grazers: Query<(&mut Grazer, &mut GrazerMind, &Position, &Rotation, &mut LinearVelocity, &mut AngularVelocity)>,
    mut rocks: Query<(Entity, &Asteroid, &Position, &mut LinearVelocity, &BodyInertia), Without<Grazer>>,
) {
    let dt = time.delta_secs();
    for (mut grazer, mut mind, position, rotation, mut velocity, mut spin) in &mut grazers {
        if grazer.dormant {
            continue;
        }
        let (pos, vel) = (position.0, velocity.0);
        mind.left -= dt;
        let rock = mind.rock.and_then(|r| rocks.get(r).ok()).map(|(e, a, p, v, _)| (e, a.radius, p.0, v.0));
        if let Some((.., v)) = rock {
            mind.stream = v;
        }
        // What it's doing.
        match grazer.mood {
            Mood::Startled if mind.left <= 0.0 => {
                mind.rock = find_food(&rocks, pos, None);
                grazer.set_mood(Mood::Cruising, tick.0);
            }
            Mood::Startled => {}
            Mood::Cruising => match rock {
                None => mind.rock = find_food(&rocks, pos, None),
                Some((_, radius, at, _)) => {
                    if mind.station == Vec3::ZERO {
                        mind.station = station(grazer.seed);
                    }
                    let spot = at + mind.station * (radius + GRAZE_GAP);
                    if spot.distance(pos) < 2.0 {
                        let mut rng = ChaCha8Rng::seed_from_u64(grazer.seed as u64 ^ tick.0 as u64);
                        mind.left = rng.random_range(GRAZE_SECONDS.0..GRAZE_SECONDS.1);
                        grazer.set_mood(Mood::Grazing, tick.0);
                    }
                }
            },
            Mood::Grazing => {
                if mind.left <= 0.0 || rock.is_none() {
                    // On to another, near this one: the herd, leaving the
                    // same rock, makes for the same next one.
                    let from = rock.map_or(pos, |(_, _, at, _)| at);
                    mind.rock = find_food(&rocks, from, mind.rock);
                    grazer.set_mood(Mood::Cruising, tick.0);
                }
            }
        }
        if grazer.last_pulse(tick.0) != tick.0 {
            continue;
        }
        // A pulse: the velocity it wants, relative to the stream.
        let rock = mind.rock.and_then(|r| rocks.get(r).ok()).map(|(e, a, p, v, _)| (e, a.radius, p.0, v.0));
        let stream = rock.map_or(mind.stream, |(.., v)| v);
        let (want, pulse, turn, turn_change) = match (grazer.mood, rock) {
            (Mood::Startled, _) => {
                let away = (pos - mind.threat).normalize_or(rotation.0 * Vec3::Y);
                (stream + away * BOLT_SPEED, BOLT_PULSE, BOLT_TURN, BOLT_TURN_CHANGE)
            }
            (mood, Some((_, radius, at, _))) => {
                let spot = at + mind.station * (radius + GRAZE_GAP);
                let top = if mood == Mood::Grazing { GRAZE_SPEED } else { CRUISE_SPEED };
                let pulse = if mood == Mood::Grazing { GRAZE_PULSE } else { CRUISE_PULSE };
                let off = spot - pos;
                let speed = top.min((2.0 * EASE * off.length()).sqrt());
                (stream + off.normalize_or_zero() * speed, pulse, TURN, TURN_CHANGE)
            }
            (_, None) => (stream, CRUISE_PULSE, TURN, TURN_CHANGE),
        };
        let change = want - vel;
        let forward = rotation.0 * Vec3::Y;
        // Grazing, it keeps its mouth (under its bell) to the rock; else it
        // turns its bell the way it wants to go.
        let face = match (grazer.mood, rock) {
            (Mood::Grazing, Some((_, _, at, _))) => (pos - at).try_normalize(),
            _ => (change.length() > 0.15).then(|| change.normalize()),
        };
        let period = grazer.period() as f32 / TICK_HZ as f32;
        let wanted_spin = face.map_or(Vec3::ZERO, |face| {
            let axis = forward.cross(face);
            let angle = forward.angle_between(face);
            axis.normalize_or_zero() * (angle / period).min(turn)
        });
        let current = spin.0;
        spin.0 += (wanted_spin - current).clamp_length_max(turn_change);
        // Its jet pushes bell-first, steering a little to the side.
        let along = change.dot(forward);
        if along > 0.0 {
            let side = (change - forward * along).clamp_length_max(along * STEER_CONE.tan());
            velocity.0 += (forward * along + side).clamp_length_max(pulse);
        }
        // Feeding: it tugs at the rock (and itself toward it).
        if grazer.mood == Mood::Grazing
            && let Some((r, radius, at, _)) = rock
            && let Ok((.., mut rock_velocity, inertia)) = rocks.get_mut(r)
        {
            let gap = pos.distance(at) - (radius + GRAZE_GAP);
            if gap > 0.0 {
                let toward = (pos - at).normalize_or_zero();
                let tug = (gap * GRAZER_MASS * 0.3).min(TUG);
                rock_velocity.0 += toward * tug * inertia.inverse_mass;
                velocity.0 -= toward * tug / GRAZER_MASS;
            }
        }
    }
}

/// Where round its rock a grazer feeds: a direction of its own.
fn station(seed: u32) -> Vec3 {
    let mut rng = ChaCha8Rng::seed_from_u64(seed as u64 * 7919);
    loop {
        let v = Vec3::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0));
        if v.length_squared() > 0.05 && v.length_squared() <= 1.0 {
            return v.normalize();
        }
    }
}

/// The nearest rock it could feed on within reach of `from`, other than
/// `not`.
fn find_food(
    rocks: &Query<(Entity, &Asteroid, &Position, &mut LinearVelocity, &BodyInertia), Without<Grazer>>,
    from: Vec3,
    not: Option<Entity>,
) -> Option<Entity> {
    rocks
        .iter()
        .filter(|(e, a, p, ..)| Some(*e) != not && (FOOD.0..FOOD.1).contains(&a.radius) && p.0.distance(from) < FORAGE)
        .min_by(|a, b| a.2.0.distance(from).total_cmp(&b.2.0.distance(from)))
        .map(|(e, ..)| e)
}

// --- Alarm ------------------------------------------------------------------

/// How long a startled grazer bolts (s), and how far its alarm carries
/// through its herd (m) while it's fresh (s).
const STARTLE_SECONDS: f32 = 6.0;
const HERD_ALARM: f32 = 35.0;
const FRESH_ALARM: f32 = 0.6;
/// What startles it: gunfire this near (m), a blast this near (m).
const GUNFIRE: f32 = 45.0;
const BLAST: f32 = 60.0;
/// Bolting into someone faster than this (m/s), it tramples them: hurting
/// this much at that speed, more the faster.
const TRAMPLE_SPEED: f32 = 3.0;
const TRAMPLE_HURT: f32 = 12.0;

/// Startles a grazer (or keeps it startled), from `from`. Its replicated
/// state is written only when its mood actually changes: a grazer kept
/// bolting (hooked, say) isn't re-sent every tick.
fn startle(grazer: &mut Mut<Grazer>, mind: &mut GrazerMind, tick: u32, from: Vec3) {
    if grazer.dormant {
        return;
    }
    if grazer.mood != Mood::Startled {
        grazer.set_mood(Mood::Startled, tick);
    }
    mind.left = STARTLE_SECONDS;
    mind.threat = from;
}

fn where_is(bodies: &Bodies, base: &Tuning, player: &Player, own: Option<&super::bots::BodyTuning>) -> (Vec3, Vec3) {
    let frame = player.mode.body().and_then(|b| bodies.frame(b));
    player.mode.world_kinematics(super::bots::tuning_of(base, own), frame.as_ref())
}

/// Authority, after damage is dealt: what startles grazers (being hurt or
/// hooked, gunfire, blasts, a herdmate bolting).
pub fn alarm(
    tick: Res<SimTick>,
    base: Res<Tuning>,
    bodies: Bodies,
    mut grazers: Query<(Entity, &mut Grazer, &mut GrazerMind)>,
    players: Query<(&Player, Option<&Team>, Option<&super::bots::BodyTuning>)>,
    mut hurt: MessageReader<Damage>,
    mut blasts: MessageReader<Detonation>,
) {
    let at = |e: Entity| bodies.frame(e).map(|f| f.center_of_mass);
    // Who's alarming what, from where.
    let mut alarms: Vec<(Entity, Vec3)> = Vec::new();
    for d in hurt.read() {
        if grazers.contains(d.target)
            && let Some(from) = d.from.or_else(|| at(d.target))
        {
            alarms.push((d.target, from));
        }
    }
    for blast in blasts.read() {
        for (e, ..) in &grazers {
            if at(e).is_some_and(|p| p.distance(blast.at) < BLAST) {
                alarms.push((e, blast.at));
            }
        }
    }
    for (player, team, own) in &players {
        let (position, _) = where_is(&bodies, &base, player, own);
        // Hooked: it bolts while the hook's in.
        if let Some(g) = player.grapple
            && grazers.contains(g.target)
        {
            alarms.push((g.target, position));
        }
        let crew = team.copied().unwrap_or_default() == Team::Crew;
        // A laser beam that's on is gunfire too (it isn't a `Fired` entry).
        let fired = player.events.iter().any(|(t, e)| t == tick.0 && matches!(e, PlayerEvent::Fired { .. }))
            || player.loadout.hands.iter().any(|h| h.beam.is_some());
        if crew && fired {
            for (e, ..) in &grazers {
                if at(e).is_some_and(|p| p.distance(position) < GUNFIRE) {
                    alarms.push((e, position));
                }
            }
        }
    }
    for (e, from) in alarms {
        if let Ok((_, mut grazer, mut mind)) = grazers.get_mut(e) {
            startle(&mut grazer, &mut mind, tick.0, from);
        }
    }
    // The alarm spreads through the herd, a grazer at a time.
    let fresh: Vec<(Vec3, Vec3)> = grazers
        .iter()
        .filter(|(_, g, _)| g.mood == Mood::Startled && (tick.0.wrapping_sub(g.since) as f32) < FRESH_ALARM * TICK_HZ as f32 && !g.dormant)
        .filter_map(|(e, _, m)| Some((at(e)?, m.threat)))
        .collect();
    for (e, mut grazer, mut mind) in &mut grazers {
        if grazer.mood == Mood::Startled {
            continue;
        }
        let Some(p) = at(e) else { continue };
        if let Some((_, threat)) = fresh.iter().find(|(q, _)| q.distance(p) < HERD_ALARM) {
            startle(&mut grazer, &mut mind, tick.0, *threat);
        }
    }
}

/// Authority: a bolting grazer that strikes someone hard tramples them.
/// The blow itself is the bounce (see `player::eva_tick`: the two trade
/// momentum like any body and a crew member); this is the hurt.
pub fn trample(
    tick: Res<SimTick>,
    grazers: Query<(&Grazer, &Position)>,
    players: Query<(Entity, &Player)>,
    mut damage: MessageWriter<Damage>,
) {
    for (victim, player) in &players {
        for (at, event) in player.events.iter() {
            let PlayerEvent::Impact { speed, body } = event else { continue };
            if at != tick.0 || speed < TRAMPLE_SPEED {
                continue;
            }
            let Ok((grazer, position)) = grazers.get(body) else { continue };
            if grazer.mood != Mood::Startled {
                continue;
            }
            damage.write(Damage {
                target: victim,
                amount: TRAMPLE_HURT * (speed / 8.0).clamp(0.5, 2.0),
                source: Some(body),
                weak: false,
                from: Some(position.0),
            });
        }
    }
}

/// Authority: a grazer out of health dies: marked [`Wrecked`], gone the
/// tick after.
pub fn settle_grazer_deaths(grazers: Query<(Entity, &Vitals, Has<Wrecked>), With<Grazer>>, mut commands: Commands) {
    for (entity, vitals, wrecked) in &grazers {
        if wrecked {
            commands.entity(entity).despawn();
        } else if vitals.health <= 0.0 {
            commands.entity(entity).insert(Wrecked).remove::<GrazerMind>();
        }
    }
}

// --- The field --------------------------------------------------------------

/// The field has fauna (it's the asteroid field's, and the world wants
/// life: tests of other things leave it out).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct FaunaField;

/// Of the rocks that stream in, the share that bring a limpet (of those big
/// enough to hide one: radius, m), or a herd grazing them (of those small
/// enough to feed on), and a herd's size.
const LIMPET_CHANCE: f32 = 0.07;
const LIMPET_ROCKS: (f32, f32) = (1.5, 12.0);
const HERD_CHANCE: f32 = 0.025;
const HERD_SIZE: (u32, u32) = (3, 6);
/// Nothing comes in this close to the home ship (m): the crew start in
/// peace.
const CLEAR_OF_HOME: f32 = 70.0;

/// Authority: rocks new to the field sometimes bring fauna.
#[allow(clippy::too_many_arguments)]
pub fn populate_field(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    rocks: Query<(Entity, &Asteroid, &Position, &LinearVelocity, &Collider), Added<FieldRock>>,
    homes: Query<&Position, With<HomeShip>>,
    mut commands: Commands,
) {
    for (rock, asteroid, position, velocity, collider) in &rocks {
        if homes.iter().any(|h| h.0.distance(position.0) < CLEAR_OF_HOME + asteroid.radius) {
            continue;
        }
        let mut rng = ChaCha8Rng::seed_from_u64(asteroid.seed ^ 0xfa_0a);
        let roll: f32 = rng.random();
        if (LIMPET_ROCKS.0..LIMPET_ROCKS.1).contains(&asteroid.radius) && roll < LIMPET_CHANCE {
            let count = if asteroid.radius > 6.0 { 2 } else { 1 };
            for i in 0..count {
                if let Some(mode) = clinging(&mut rng, rock, collider, asteroid.radius) {
                    let Mode::Booted { feet, .. } = mode else { continue };
                    let index = (asteroid.seed as u32).wrapping_add(i);
                    commands.spawn(new_robot(Robot::Limpet, Affixes::NONE, mode, rock, feet, &tuning, index));
                }
            }
        } else if (FOOD.0..FOOD.1).contains(&asteroid.radius) && roll > 1.0 - HERD_CHANCE {
            let size = rng.random_range(HERD_SIZE.0..HERD_SIZE.1);
            for i in 0..size {
                let seed: u32 = rng.random();
                let dir = station(seed);
                let at = position.0 + dir * (asteroid.radius + GRAZE_GAP + rng.random_range(0.0..1.5));
                // Mouth to the rock, at the stream's speed, out of step.
                let turn = Quat::from_rotation_arc(Vec3::Y, dir);
                let since = tick.0.wrapping_sub(seed % 120);
                let mut body = commands.spawn(grazer(seed, Mood::Grazing, since, at, turn, velocity.0, false));
                let left = rng.random_range(GRAZE_SECONDS.0..GRAZE_SECONDS.1) + i as f32;
                body.insert(GrazerMind {
                    rock: Some(rock),
                    station: dir,
                    left,
                    stream: velocity.0,
                    ..default()
                });
            }
        }
    }
}

/// Standing on `rock` (its `collider`, its `radius`) somewhere on its
/// surface picked by `seed`, facing any way: where a limpet clings (or
/// anyone stands, in a test).
pub fn cling_spot(seed: u64, rock: Entity, collider: &Collider, radius: f32) -> Option<Mode> {
    clinging(&mut ChaCha8Rng::seed_from_u64(seed), rock, collider, radius)
}

fn clinging(rng: &mut ChaCha8Rng, rock: Entity, collider: &Collider, radius: f32) -> Option<Mode> {
    for _ in 0..8 {
        let dir = Vec3::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0)).try_normalize()?;
        let from = dir * radius * 3.0;
        let (t, normal) = collider.cast_ray(Vec3::ZERO, Quat::IDENTITY, from, -dir, radius * 3.0, true)?;
        let up = normal.normalize_or(dir);
        if up.dot(dir) < 0.5 {
            // A steep face: somewhere else.
            continue;
        }
        let feet = from - dir * t + up * 0.01;
        let orientation = (Quat::from_axis_angle(up, rng.random_range(0.0..std::f32::consts::TAU)) * Quat::from_rotation_arc(Vec3::Y, up)).normalize();
        return Some(Mode::Booted {
            body: rock,
            feet,
            up,
            velocity: Vec3::ZERO,
            orientation,
            pitch: 0.0,
        });
    }
    None
}

/// Authority: fauna that have drifted out of the field around the crew go
/// with the rocks (see `asteroids::recycle_field`).
pub fn cull_fauna(
    interest: Res<FieldInterest>,
    base: Res<Tuning>,
    bodies: Bodies,
    grazers: Query<(Entity, &Position), With<Grazer>>,
    limpets: Query<(Entity, &Robot, &Player, Option<&super::bots::BodyTuning>)>,
    mut commands: Commands,
) {
    if interest.0.is_empty() {
        return;
    }
    let reach = FIELD_LENGTH / 2.0 + 60.0;
    let out = |p: Vec3| interest.0.iter().all(|i| i.position.distance(p) > reach);
    for (e, p) in &grazers {
        if out(p.0) {
            commands.entity(e).despawn();
        }
    }
    for (e, kind, player, own) in &limpets {
        if !kind.fauna() {
            continue;
        }
        let frame = player.mode.body().and_then(|b| bodies.frame(b));
        let (p, _) = player.mode.world_kinematics(super::bots::tuning_of(&base, own), frame.as_ref());
        if out(p) {
            commands.entity(e).despawn();
        }
    }
}
