//! Robots: players driven by a brain instead of a person.
//!
//! A robot is a [`Player`] like any crew member, with the same boots,
//! jetpack, grapple, guns, and [`Vitals`]. Its own [`BodyTuning`] sets its
//! size, speed and toughness. On the authority a [`Brain`] writes its
//! [`PlayerInput`] each tick, before players are simulated, from what it can
//! see. Clients never simulate robots; they receive them like other remote
//! players, plus the [`Robot`] kind to know what to draw.

use avian3d::prelude::*;
use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bodies::Bodies;
use super::events::EventLog;
use super::input::{InputCursor, PlayerInput};
use super::player::{Mode, Player, PlayerTelemetry, PreviousMode, capsule};
use super::shots::Team;
use super::tuning::Tuning;
use super::vitals::Vitals;
use super::weapons::{Gun, GunKind, Loadout, STANCES, Slot, Stance};
use super::{Layer, SimTick, TICK_HZ};

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Robot {
    /// Walks decks and hulls on mag-boots; a rifle arm; keeps its distance
    /// and strafes.
    Gunner,
    /// A small fast drone; rushes in and sprays at close range.
    Swarmer,
}

/// A body's own tuning, where it differs from the crew's.
#[derive(Component, Clone, Debug)]
pub struct BodyTuning(pub Tuning);

/// The tuning that applies to a body: its own, or the crew's.
pub fn tuning_of<'a>(base: &'a Tuning, own: Option<&'a BodyTuning>) -> &'a Tuning {
    own.map_or(base, |t| &t.0)
}

impl Robot {
    pub fn name(self) -> &'static str {
        match self {
            Robot::Gunner => "Gunner",
            Robot::Swarmer => "Swarmer",
        }
    }

    /// The crew's tuning, changed where this robot is built differently.
    pub fn tuning(self, base: &Tuning) -> Tuning {
        let mut t = base.clone();
        match self {
            Robot::Gunner => {
                t.player_mass = 220.0;
                t.capsule_radius = 0.38;
                t.capsule_height = 1.9;
                t.eye_height = 1.78;
                t.walk_speed = 3.0;
                t.sprint_speed = 4.5;
                t.player_health = 120.0;
                // It keeps to floors: a wall isn't somewhere to walk.
                t.wall_walk = false;
            }
            Robot::Swarmer => {
                t.player_mass = 25.0;
                t.capsule_radius = 0.38;
                t.capsule_height = 0.76;
                t.eye_height = 0.38;
                t.thrust_accel = 18.0;
                t.assist_max_speed = 16.0;
                t.player_health = 30.0;
            }
        }
        t
    }

    /// Its weak spot: a sphere's centre, in the body's frame about its
    /// middle (the capsule centre; −Z is its front), and radius (m). From
    /// the model's `SOCKET_weak`.
    pub fn weak_spot(self) -> (Vec3, f32) {
        match self {
            Robot::Gunner => (WEAK_GUNNER, 0.15),
            Robot::Swarmer => (WEAK_SWARMER, 0.09),
        }
    }

    fn gun(self) -> GunKind {
        match self {
            Robot::Gunner => GunKind::GunnerRifle,
            Robot::Swarmer => GunKind::SwarmerSprayer,
        }
    }

    fn mind(self) -> Mind {
        match self {
            Robot::Gunner => Mind {
                sight: 70.0,
                turn_rate: 2.0,
                aim_error: 0.035,
                burst_on: 0.5,
                burst_off: 1.2,
                fire_cone: 0.1,
                range: (12.0, 35.0),
            },
            Robot::Swarmer => Mind {
                sight: 120.0,
                turn_rate: 5.0,
                aim_error: 0.06,
                burst_on: 0.35,
                burst_off: 1.4,
                fire_cone: 0.15,
                range: (5.0, 12.0),
            },
        }
    }
}

/// How a kind of robot thinks.
struct Mind {
    /// How far it notices the crew (m), given a clear line.
    sight: f32,
    /// How fast it turns to aim (rad/s).
    turn_rate: f32,
    /// How far off it aims, wandering (rad).
    aim_error: f32,
    /// Firing in bursts: seconds on, seconds off.
    burst_on: f32,
    burst_off: f32,
    /// How close to on target it has to be to fire (rad).
    fire_cone: f32,
    /// The distance band it tries to fight from (m).
    range: (f32, f32),
}

/// A robot's state of mind. Authority only.
#[derive(Component, Clone, Debug, Default)]
pub struct Brain {
    pub target: Option<Entity>,
    /// Whether it could see its target when it last looked, and seconds
    /// since it last did.
    visible: bool,
    unseen: f32,
    /// Seconds until it looks for a target again.
    rethink: f32,
    /// Where it is in its burst cycle (s).
    burst: f32,
    /// Strafing direction (±1, or 0 where neither side keeps the target in
    /// sight) and seconds until it changes.
    strafe: f32,
    strafe_timer: f32,
    /// Where it keeps to with nothing to fight, in its home body's frame.
    pub post: Vec3,
    /// Its own phase for wobble and timers, so robots don't move in lockstep.
    seed: f32,
}

impl MapEntities for Brain {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        if let Some(t) = &mut self.target {
            *t = mapper.get_mapped(*t);
        }
    }
}

/// A new robot of `kind` in `mode`, guarding `home` from `post` (in home's
/// frame), where it returns with nothing to fight. `index` varies its
/// timing.
pub fn new_robot(kind: Robot, mode: Mode, home: Entity, post: Vec3, base: &Tuning, index: u32) -> impl Bundle {
    let tuning = kind.tuning(base);
    let gun = Slot::Gun(0);
    let loadout = Loadout {
        guns: vec![Gun {
            reserve: u16::MAX,
            ..Gun::full(kind.gun())
        }],
        stances: [Stance {
            main: gun,
            off: Slot::Empty,
        }; STANCES],
        ..Loadout::default()
    };
    (
        Name::new(kind.name()),
        kind,
        Brain {
            post,
            seed: index as f32 * 1.618,
            strafe: if index % 2 == 0 { 1.0 } else { -1.0 },
            ..default()
        },
        Player {
            mode,
            boots_on: matches!(mode, Mode::Booted { .. }),
            flight_assist: true,
            grapple: None,
            reference_body: Some(home),
            detach_timer: 0.0,
            loadout,
            input: InputCursor::default(),
            events: EventLog::default(),
        },
        PreviousMode(mode),
        PlayerInput::default(),
        PlayerTelemetry::default(),
        Vitals::full(&tuning),
        Team::Robots,
        BodyTuning(tuning),
        super::vitals::HitLog::default(),
    )
}

/// Every peer: a robot gets its own tuning and a capsule sized to it.
pub fn add_body_tuning(add: On<Add, Robot>, robots: Query<&Robot>, base: Res<Tuning>, mut commands: Commands) {
    let kind = *robots.get(add.entity).expect("just added");
    let tuning = kind.tuning(&base);
    commands.entity(add.entity).insert((capsule(&tuning), BodyTuning(tuning)));
}

/// The look change (yaw right, pitch down) that turns a body (world
/// rotation `body`, its Y the axis it turns about) and its view `pitch` to
/// look along world direction `dir`: the bearing to it about the body's up,
/// and the pitch that matches its elevation. (Measured in the body's frame,
/// not the view's: from the view, a target behind and below seems to sink
/// further as the view tips down to turn toward it.)
fn look_toward(body: Quat, pitch: f32, dir: Vec3) -> Vec2 {
    let local = body.inverse() * dir.normalize_or(body * Vec3::NEG_Z);
    let yaw = local.x.atan2(-local.z);
    let elevation = local.y.clamp(-1.0, 1.0).asin();
    Vec2::new(yaw, pitch - elevation)
}

type CrewQuery<'a> = (Entity, &'a Player, Option<&'a Team>, Option<&'a BodyTuning>);

/// Authority, before players move: every robot decides what to do this
/// tick and writes it as input.
pub fn think(
    tick: Res<SimTick>,
    time: Res<Time>,
    base: Res<Tuning>,
    bodies: Bodies,
    spatial: SpatialQuery,
    everyone: Query<CrewQuery>,
    mut robots: Query<(Entity, &Robot, &mut Brain, &mut PlayerInput)>,
) {
    let dt = time.delta_secs();
    let now = tick.0 as f32 / TICK_HZ as f32;
    for (entity, kind, mut brain, mut input) in &mut robots {
        let Ok((_, me, _, own)) = everyone.get(entity) else {
            continue;
        };
        let tuning = tuning_of(&base, own);
        let mind = kind.mind();
        let frame = me.mode.body().and_then(|b| bodies.frame(b));
        let (eye, view) = me.mode.world_view(tuning, frame.as_ref());
        // The body's rotation (its Y the axis it turns about) and view pitch.
        let body = body_rotation(&me.mode, frame.as_ref());
        let pitch = match me.mode {
            Mode::Booted { pitch, .. } | Mode::Eva { pitch, .. } => pitch,
        };
        let (position, _) = me.mode.world_kinematics(tuning, frame.as_ref());
        let sees = |target: Vec3| {
            let Ok(dir) = Dir3::new(target - eye) else {
                return true;
            };
            let distance = eye.distance(target);
            spatial
                .cast_ray(eye, dir, distance, true, &SpatialQueryFilter::from_mask(Layer::World))
                .is_none_or(|hit| hit.distance > distance - 0.6)
        };
        let where_is = |e: Entity| -> Option<Vec3> {
            let (_, p, _, t) = everyone.get(e).ok()?;
            let f = p.mode.body().and_then(|b| bodies.frame(b));
            Some(p.mode.world_kinematics(tuning_of(&base, t), f.as_ref()).0)
        };
        // Who to fight: the nearest of the crew in sight, looked for now
        // and then (sight lines cost a ray each). One that slips out of
        // sight is followed a while (it's probably just behind something),
        // but not shot at.
        brain.rethink -= dt;
        brain.unseen += dt;
        if brain.rethink <= 0.0 {
            brain.rethink = 0.3;
            let current = brain.target.and_then(|t| Some((t, where_is(t)?)));
            if let Some((t, at)) = current
                && at.distance(eye) < mind.sight
                && sees(at)
            {
                brain.target = Some(t);
                brain.visible = true;
            } else if let Some((t, _)) = everyone
                .iter()
                .filter(|(_, _, team, _)| team.copied().unwrap_or_default() != Team::Robots)
                .filter_map(|(e, ..)| Some((e, where_is(e)?)))
                .filter(|(_, at)| at.distance(eye) < mind.sight && sees(*at))
                .min_by(|a, b| a.1.distance(eye).total_cmp(&b.1.distance(eye)))
            {
                brain.target = Some(t);
                brain.visible = true;
            } else {
                brain.visible = false;
                if current.is_none() || brain.unseen > MEMORY {
                    brain.target = None;
                }
            }
            if brain.visible {
                brain.unseen = 0.0;
                // Strafe only to a side it would still see the target from.
                if let Some(at) = brain.target.and_then(where_is) {
                    let right = view * Vec3::X;
                    let side = if brain.strafe == 0.0 { 1.0 } else { brain.strafe };
                    let sees_from = |s: f32| {
                        let from = eye + right * s * STRAFE_PROBE;
                        Dir3::new(at - from).is_ok_and(|dir| {
                            let d = from.distance(at);
                            spatial
                                .cast_ray(from, dir, d, true, &SpatialQueryFilter::from_mask(Layer::World))
                                .is_none_or(|hit| hit.distance > d - 0.6)
                        })
                    };
                    brain.strafe = if sees_from(side) {
                        side
                    } else if sees_from(-side) {
                        -side
                    } else {
                        0.0
                    };
                }
            }
        }
        let target = brain.target.and_then(where_is);
        // Aim: turn toward the target (off by a wandering error), at the
        // robot's turn rate.
        let mut look = Vec2::ZERO;
        let mut on_target = false;
        if let Some(at) = target {
            let wobble = Vec2::new((now * 1.3 + brain.seed).sin(), (now * 0.9 + brain.seed * 2.0).cos()) * mind.aim_error;
            let want = look_toward(body, pitch, at - eye) + wobble;
            let step = mind.turn_rate * dt;
            look = want.clamp(Vec2::splat(-step), Vec2::splat(step));
            on_target = brain.visible && want.length() < mind.fire_cone;
        }
        input.add_look(look);
        // Fire in bursts while on target.
        brain.burst = (brain.burst + dt) % (mind.burst_on + mind.burst_off);
        input.main.held = on_target && brain.burst < mind.burst_on;
        // Move: keep within its fighting range, strafing; with nothing to
        // fight, go back to its post.
        brain.strafe_timer -= dt;
        if brain.strafe_timer <= 0.0 {
            brain.strafe = if brain.strafe == 0.0 { 1.0 } else { -brain.strafe };
            brain.strafe_timer = 1.5 + (brain.seed * 7.3 + now).sin().abs() * 2.0;
        }
        let (near, far) = mind.range;
        input.movement = match (target, me.mode) {
            (Some(at), Mode::Booted { up, .. }) => {
                let distance = at.distance(position);
                // Lost sight of it: stop strafing and go after it.
                let hunting = !brain.visible;
                let mut advance = if hunting || distance > far {
                    1.0
                } else if distance < near {
                    -1.0
                } else {
                    0.0
                };
                // Walking into a wall climbs it; a robot keeps off walls
                // (and other robots) instead: it turns its strafe round,
                // and won't press on toward something in the way.
                let up = frame.map_or(up, |f| f.rotation * up);
                let flat = |v: Vec3| (v - up * v.dot(up)).normalize_or_zero();
                let (forward, right) = (flat(view * Vec3::NEG_Z), flat(view * Vec3::X));
                let blocked = |dir: Vec3| {
                    Dir3::new(dir).is_ok_and(|dir| {
                        spatial
                            .cast_ray(position, dir, WALL_CLEARANCE, true, &SpatialQueryFilter::default().with_excluded_entities([entity]))
                            .is_some()
                    })
                };
                if blocked(right * brain.strafe) {
                    brain.strafe = -brain.strafe;
                }
                if advance != 0.0 && blocked(forward * advance) {
                    advance = 0.0;
                }
                // Hunting, it sidesteps to line up with where the target is
                // (through a doorway, say); fighting, it strafes.
                let across = right.dot(at - position);
                let strafe = if hunting {
                    if across.abs() > 0.1 { across.signum() } else { 0.0 }
                } else if blocked(right * brain.strafe) {
                    0.0
                } else {
                    brain.strafe
                };
                Vec3::new(strafe, 0.0, advance)
            }
            (Some(at), Mode::Eva { .. }) => {
                // Hold a point beside the target, swinging round it.
                let toward = (at - position).normalize_or_zero();
                let side = toward.any_orthonormal_vector() * brain.strafe;
                let goal = at - toward * (near + far) / 2.0 + side * 4.0;
                to_input(view, goal - position)
            }
            (None, Mode::Booted { feet, .. }) => {
                let off = brain.post - feet;
                if off.length() > 1.5 {
                    // Walk back: turn toward the post first.
                    let world = frame.map_or(off, |f| f.rotation * off);
                    input.add_look(Vec2::new(look_toward(body, pitch, world).x.clamp(-0.05, 0.05), 0.0));
                    Vec3::new(0.0, 0.0, 1.0)
                } else {
                    Vec3::ZERO
                }
            }
            (None, Mode::Eva { .. }) => {
                let home = me.reference_body.and_then(|b| bodies.frame(b));
                let post = home.map_or(brain.post, |f| f.point_to_world(brain.post));
                let off = post - position;
                if off.length() > 3.0 { to_input(view, off) } else { Vec3::ZERO }
            }
        };
    }
}

/// Weak spot centres (see [`Robot::weak_spot`]): the Gunner's backpack
/// core, the Swarmer's reactor between its rear thrusters.
// Gunner: SOCKET_weak at (0, 1.42, 0.31) from its feet, its middle 0.95 up.
const WEAK_GUNNER: Vec3 = Vec3::new(0.0, 0.47, 0.31);
// Swarmer: SOCKET_weak at (0, 0.035, 0.19) from its middle (its origin).
const WEAK_SWARMER: Vec3 = Vec3::new(0.0, 0.035, 0.19);

/// Damage multiplier for a hit on a weak spot.
pub const WEAK_MULTIPLIER: f32 = 2.5;

/// A body's rotation: its Y the axis it stands or turns about, its −Z its
/// front.
pub fn body_rotation(mode: &Mode, frame: Option<&super::frame::BodyFrame>) -> Quat {
    match *mode {
        Mode::Booted { orientation, .. } => frame.map_or(orientation, |f| f.rotation * orientation),
        Mode::Eva { orientation, .. } => orientation,
    }
}

/// How far to the side a robot checks it would still see its target from,
/// before strafing that way (m).
const STRAFE_PROBE: f32 = 1.0;

/// How long a robot keeps after a target it can't see (s).
const MEMORY: f32 = 3.0;

/// How close (m, from a walking robot's middle) it lets walls come before
/// stepping away.
const WALL_CLEARANCE: f32 = 1.3;

/// A world direction as jetpack input (x right, y up, z forward in the
/// view), full strength.
fn to_input(view: Quat, dir: Vec3) -> Vec3 {
    let local = view.inverse() * dir.normalize_or_zero();
    Vec3::new(local.x, local.y, -local.z)
}

/// Authority: a robot out of health is destroyed.
pub fn settle_robot_deaths(robots: Query<(Entity, &Vitals), With<Robot>>, mut commands: Commands) {
    for (entity, vitals) in &robots {
        if vitals.health <= 0.0 {
            commands.entity(entity).despawn();
        }
    }
}

/// Debug: robots to spawn by our ship, for trying out a fight. Read by the
/// authority (so only the host's buttons work).
#[derive(Message, Clone, Copy, Debug)]
pub struct SpawnRobots(pub Robot);

/// Authority: a Gunner on our bridge, facing aft; or a pack of three
/// Swarmers 40 m off our port side.
pub fn debug_spawn_robots(
    mut orders: MessageReader<SpawnRobots>,
    tuning: Res<Tuning>,
    bodies: Bodies,
    home: Option<Single<Entity, With<super::ship::HomeShip>>>,
    mut commands: Commands,
    mut spawned: Local<u32>,
) {
    let Some(home) = home else {
        orders.clear();
        return;
    };
    let Some(frame) = bodies.frame(*home) else {
        return;
    };
    for SpawnRobots(kind) in orders.read() {
        *spawned += 1;
        match kind {
            Robot::Gunner => {
                let feet = Vec3::new(0.0, 0.01, -11.0);
                let mode = Mode::Booted {
                    body: *home,
                    feet,
                    up: Vec3::Y,
                    velocity: Vec3::ZERO,
                    orientation: Quat::from_rotation_y(std::f32::consts::PI),
                    pitch: 0.0,
                };
                commands.spawn(new_robot(Robot::Gunner, mode, *home, feet, &tuning, *spawned));
            }
            Robot::Swarmer => {
                for i in 0..3 {
                    let post = Vec3::new(-40.0, 2.0 * i as f32, -4.0 + 4.0 * i as f32);
                    let position = frame.point_to_world(post);
                    let mode = Mode::Eva {
                        position,
                        velocity: frame.point_velocity(position),
                        orientation: frame.rotation,
                        pitch: 0.0,
                    };
                    commands.spawn(new_robot(Robot::Swarmer, mode, *home, post, &tuning, *spawned * 3 + i));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looking_toward_a_direction_turns_the_right_way() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        let ahead = look_toward(Quat::IDENTITY, 0.0, Vec3::NEG_Z);
        assert!(ahead.length() < 1e-6);
        let right = look_toward(Quat::IDENTITY, 0.0, Vec3::X);
        assert!((right.x - FRAC_PI_2).abs() < 1e-5 && right.y.abs() < 1e-5);
        // Up is a negative pitch-down.
        let up = look_toward(Quat::IDENTITY, 0.0, Vec3::new(0.0, 1.0, -1.0));
        assert!((up.y + FRAC_PI_4).abs() < 1e-5, "{up}");
        // Behind and a little below: turn round, and pitch only to the
        // target's elevation, however the view is pitched now.
        let behind = look_toward(Quat::IDENTITY, -0.5, Vec3::new(0.0, -0.1, 1.0));
        assert!((behind.x.abs() - PI).abs() < 1e-5);
        assert!((behind.y - (-0.5 + 0.1f32.atan())).abs() < 1e-3, "{behind}");
    }
}
