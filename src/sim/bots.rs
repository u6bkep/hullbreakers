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
use super::events::PlayerEvent;
use super::input::{InputCursor, PlayerInput};
use super::nav::{NavGrid, NavGrids};
use super::ship::ShipHull;
use super::vitals::Damage;
use super::player::{Mode, Player, PlayerTelemetry, PreviousMode, capsule};
use super::shots::Team;
use super::tuning::Tuning;
use super::vitals::Vitals;
use super::weapons::{GunKind, Loadout};
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
#[derive(Component, Clone, Debug)]
pub struct Brain {
    /// Who it's after.
    pub target: Option<Entity>,
    /// Whether it could see its target when it last looked.
    visible: bool,
    /// Where it believes its target is, in its home's frame, and seconds
    /// since it last saw, heard, or was hit by it.
    lead: Option<Vec3>,
    since_lead: f32,
    /// Seconds since it last saw its target.
    since_seen: f32,
    /// Seconds spent looking about where the lead ran out.
    searching: f32,
    /// Seconds until it looks for a target again.
    rethink: f32,
    /// Where it is in its burst cycle (s).
    burst: f32,
    /// Strafing direction (±1, or 0 where neither side keeps the target in
    /// sight) and seconds until it changes.
    strafe: f32,
    strafe_timer: f32,
    /// The ship it belongs to, and where it keeps to there with nothing to
    /// fight, in that ship's frame.
    pub home: Entity,
    pub post: Vec3,
    /// The way it's walking (points in its home's frame), where to, and
    /// seconds until it plans again.
    route: Vec<Vec3>,
    route_goal: Option<Vec3>,
    replan: f32,
    /// Its own phase for wobble and timers, so robots don't move in lockstep.
    seed: f32,
}

impl Brain {
    fn new(home: Entity, post: Vec3, index: u32) -> Brain {
        Brain {
            target: None,
            visible: false,
            lead: None,
            since_lead: 0.0,
            since_seen: f32::INFINITY,
            searching: 0.0,
            rethink: 0.0,
            burst: 0.0,
            strafe: if index % 2 == 0 { 1.0 } else { -1.0 },
            strafe_timer: 0.0,
            home,
            post,
            route: Vec::new(),
            route_goal: None,
            replan: 0.0,
            seed: index as f32 * 1.618,
        }
    }

    /// It has word of `target` at `at` (in its home's frame): a sighting,
    /// a sound, a hit, a call from another robot.
    fn hear_of(&mut self, target: Entity, at: Vec3) {
        self.target = Some(target);
        self.lead = Some(at);
        self.since_lead = 0.0;
        self.searching = 0.0;
    }

    fn forget(&mut self) {
        self.target = None;
        self.visible = false;
        self.lead = None;
        self.searching = 0.0;
    }
}

impl MapEntities for Brain {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        if let Some(t) = &mut self.target {
            *t = mapper.get_mapped(*t);
        }
        self.home = mapper.get_mapped(self.home);
    }
}

/// A new robot of `kind` in `mode`, guarding `home` from `post` (in home's
/// frame), where it returns with nothing to fight. `index` varies its
/// timing.
pub fn new_robot(kind: Robot, mode: Mode, home: Entity, post: Vec3, base: &Tuning, index: u32) -> impl Bundle {
    let tuning = kind.tuning(base);
    let loadout = Loadout::robot(kind.gun());
    (
        Name::new(kind.name()),
        kind,
        Brain::new(home, post, index),
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
///
/// What a robot knows of the crew is what it has seen, heard (gunfire,
/// through walls), felt (a hit tells it where the shot came from), or been
/// told by another robot that saw them. Out of sight, it goes where it last
/// knew its target to be, by the ship's walkways if it walks; finding
/// nothing there, it looks about a while, then goes back to its post.
#[allow(clippy::too_many_arguments)]
pub fn think(
    tick: Res<SimTick>,
    time: Res<Time>,
    base: Res<Tuning>,
    bodies: Bodies,
    spatial: SpatialQuery,
    everyone: Query<CrewQuery>,
    hulls: Query<&ShipHull>,
    mut grids: ResMut<NavGrids>,
    mut damage: MessageReader<Damage>,
    mut robots: Query<(Entity, &Robot, &mut Brain, &mut PlayerInput)>,
) {
    let dt = time.delta_secs();
    let now = tick.0 as f32 / TICK_HZ as f32;
    let where_is = |e: Entity| -> Option<Vec3> {
        let (_, p, _, t) = everyone.get(e).ok()?;
        let f = p.mode.body().and_then(|b| bodies.frame(b));
        Some(p.mode.world_kinematics(tuning_of(&base, t), f.as_ref()).0)
    };
    let crew = |team: Option<&Team>| team.copied().unwrap_or_default() != Team::Robots;
    // Gunfire from the crew this tick or last, and who hit whom.
    let shots: Vec<(Entity, Vec3)> = everyone
        .iter()
        .filter(|(_, p, team, _)| {
            crew(*team)
                && p.events
                    .iter()
                    .any(|(at, e)| matches!(e, PlayerEvent::Fired { .. }) && tick.0.wrapping_sub(at) <= 1)
        })
        .filter_map(|(e, ..)| Some((e, where_is(e)?)))
        .collect();
    let hits: Vec<(Entity, Entity)> = damage.read().filter_map(|d| Some((d.target, d.source?))).collect();
    // Robots that caught sight of someone just now, and who.
    let mut sightings: Vec<(Vec3, Entity)> = Vec::new();
    for (entity, kind, mut brain, mut input) in &mut robots {
        let Ok((_, me, _, own)) = everyone.get(entity) else {
            continue;
        };
        let brain = &mut *brain;
        let tuning = tuning_of(&base, own);
        let mind = kind.mind();
        let frame = me.mode.body().and_then(|b| bodies.frame(b));
        let home = bodies.frame(brain.home);
        let to_home = |w: Vec3| home.as_ref().map_or(w, |f| f.point_to_local(w));
        let to_world = |l: Vec3| home.as_ref().map_or(l, |f| f.point_to_world(l));
        let (eye, view) = me.mode.world_view(tuning, frame.as_ref());
        // The body's rotation (its Y the axis it turns about) and view pitch.
        let body = body_rotation(&me.mode, frame.as_ref());
        let pitch = match me.mode {
            Mode::Booted { pitch, .. } | Mode::Eva { pitch, .. } => pitch,
        };
        let (position, _) = me.mode.world_kinematics(tuning, frame.as_ref());
        let clear_line = |from: Vec3, to: Vec3| {
            let Ok(dir) = Dir3::new(to - from) else {
                return true;
            };
            let distance = from.distance(to);
            spatial
                .cast_ray(from, dir, distance, true, &SpatialQueryFilter::from_mask(Layer::World))
                .is_none_or(|hit| hit.distance > distance - 0.6)
        };

        // Sight: the nearest of the crew in view, looked for now and then
        // (sight lines cost a ray each).
        brain.rethink -= dt;
        brain.since_lead += dt;
        brain.since_seen += dt;
        if brain.rethink <= 0.0 {
            brain.rethink = 0.3;
            let was_visible = brain.visible;
            let current = brain.target.and_then(|t| Some((t, where_is(t)?)));
            let seen = current
                .filter(|(_, at)| at.distance(eye) < mind.sight && clear_line(eye, *at))
                .or_else(|| {
                    everyone
                        .iter()
                        .filter(|(_, _, team, _)| crew(*team))
                        .filter_map(|(e, ..)| Some((e, where_is(e)?)))
                        .filter(|(_, at)| at.distance(eye) < mind.sight && clear_line(eye, *at))
                        .min_by(|a, b| a.1.distance(eye).total_cmp(&b.1.distance(eye)))
                });
            brain.visible = seen.is_some();
            if let Some((t, at)) = seen {
                if !was_visible || brain.target != Some(t) {
                    sightings.push((position, t));
                }
                brain.target = Some(t);
                brain.since_seen = 0.0;
                // Strafe only to a side it would still see the target from.
                let right = view * Vec3::X;
                let side = if brain.strafe == 0.0 { 1.0 } else { brain.strafe };
                let sees_from = |s: f32| clear_line(eye + right * s * STRAFE_PROBE, at);
                brain.strafe = if sees_from(side) {
                    side
                } else if sees_from(-side) {
                    -side
                } else {
                    0.0
                };
            }
        }
        let target_at = brain.target.and_then(where_is);
        if let (Some(t), Some(at)) = (brain.target, target_at)
            && brain.since_seen < GLIMPSE
        {
            // In sight, or only just out of it: it saw where they went.
            brain.hear_of(t, to_home(at));
        } else if !brain.visible {
            // Out of sight: shot at, or gunfire within earshot, tells it
            // where someone is.
            let hit_by = hits.iter().find(|(victim, _)| *victim == entity).map(|(_, s)| *s);
            let heard = || {
                shots
                    .iter()
                    .filter(|(_, at)| at.distance(position) < HEARING)
                    .min_by(|a, b| a.1.distance(position).total_cmp(&b.1.distance(position)))
                    .map(|(s, _)| *s)
            };
            let fresh = brain.lead.is_none() || brain.since_lead > 1.0;
            if let Some(source) = hit_by.or_else(|| fresh.then(heard).flatten())
                && let Some(at) = where_is(source)
            {
                brain.hear_of(source, to_home(at));
                brain.rethink = 0.0;
            }
        }
        if brain.since_lead > FORGET || brain.searching > SEARCH_SECONDS || brain.target.is_some_and(|t| everyone.get(t).is_err()) {
            brain.forget();
        }

        // Aim: in sight, turn toward the target (off by a wandering error)
        // at the robot's turn rate, and fire in bursts while on target.
        let step = mind.turn_rate * dt;
        let turn = |want: Vec2| want.clamp(Vec2::splat(-step), Vec2::splat(step));
        let mut on_target = false;
        let aiming = target_at.filter(|_| brain.visible);
        if let Some(at) = aiming {
            let wobble = Vec2::new((now * 1.3 + brain.seed).sin(), (now * 0.9 + brain.seed * 2.0).cos()) * mind.aim_error;
            let want = look_toward(body, pitch, at - eye) + wobble;
            input.add_look(turn(want));
            on_target = want.length() < mind.fire_cone;
        }
        brain.burst = (brain.burst + dt) % (mind.burst_on + mind.burst_off);
        input.main.held = on_target && brain.burst < mind.burst_on;
        input.sprint = false;

        brain.strafe_timer -= dt;
        if brain.strafe_timer <= 0.0 {
            brain.strafe = if brain.strafe == 0.0 { 1.0 } else { -brain.strafe };
            brain.strafe_timer = 1.5 + (brain.seed * 7.3 + now).sin().abs() * 2.0;
        }
        let (near, far) = mind.range;
        // Where it's going, in its home's frame: after the target, to where
        // it was last known, or back to its post.
        let distance = target_at.map(|at| at.distance(position));
        let goal = match (aiming, brain.lead) {
            (Some(_), _) if distance.is_some_and(|d| d <= far) => None,
            (_, Some(lead)) => Some(lead),
            (None, None) => Some(brain.post),
            (Some(at), None) => Some(to_home(at)),
        };
        // Looking about where the trail ran cold.
        let search = |input: &mut PlayerInput, brain: &mut Brain| {
            brain.searching += dt;
            let dir = if brain.seed.sin() > 0.0 { 1.0 } else { -1.0 };
            input.add_look(Vec2::new(dir * SEARCH_TURN * dt, turn(Vec2::new(0.0, pitch)).y));
        };
        input.movement = match me.mode {
            Mode::Booted { body: on, feet, up, orientation, .. } => {
                let up_w = frame.map_or(up, |f| f.rotation * up);
                let flat = |v: Vec3| (v - up_w * v.dot(up_w)).normalize_or_zero();
                let (forward, right) = (flat(view * Vec3::NEG_Z), flat(view * Vec3::X));
                let blocked = |dir: Vec3| {
                    Dir3::new(dir).is_ok_and(|dir| {
                        spatial
                            .cast_ray(position, dir, WALL_CLEARANCE, true, &SpatialQueryFilter::default().with_excluded_entities([entity]))
                            .is_some()
                    })
                };
                // Walking a route: the grid of the ship underfoot, if that's
                // its home (routes are in its home's frame).
                let grid = (on == brain.home).then(|| hulls.get(on).ok()).flatten().map(|h| grids.get(&h.0));
                let walk = match goal {
                    None => {
                        // Fighting where it stands: the route is history.
                        brain.route.clear();
                        brain.route_goal = None;
                        Walk::Arrived
                    }
                    Some(goal) => {
                        let route = grid.and_then(|grid| {
                            let stale = brain.route_goal.is_none_or(|g| g.distance(goal) > REPLAN_DISTANCE);
                            brain.replan -= dt;
                            if stale || brain.replan <= 0.0 {
                                brain.route = grid.path(feet, goal).unwrap_or_default();
                                brain.route_goal = Some(goal);
                                brain.replan = REPLAN_SECONDS + brain.seed.fract() * 0.5;
                            }
                            // No route at all: off the grid, or nowhere it
                            // can reach.
                            (!brain.route.is_empty() || at_goal(feet, goal)).then(|| follow(grid, &mut brain.route, feet))
                        });
                        match route {
                            Some(Some(dir)) => Walk::Toward(dir),
                            Some(None) => Walk::Arrived,
                            // Without a route, straight at it, as far as
                            // nothing's in the way.
                            None => {
                                let local = frame.map_or(goal, |f| f.point_to_local(to_world(goal)));
                                let dir = Vec3::new(local.x - feet.x, 0.0, local.z - feet.z);
                                let world = frame.map_or(dir, |f| f.rotation * dir);
                                if at_goal(feet, local) || blocked(world) {
                                    Walk::Arrived
                                } else {
                                    Walk::Toward(dir.normalize_or_zero())
                                }
                            }
                        }
                    }
                };
                // A local walking direction as input, in the body's own
                // frame (x right, z forward).
                let to_input = |dir: Vec3| {
                    let local = orientation.inverse() * dir;
                    Vec3::new(local.x, 0.0, -local.z).normalize_or_zero()
                };
                match (aiming, walk) {
                    // Closing on a target in sight: walk the route, gun on it.
                    (Some(_), Walk::Toward(dir)) => to_input(dir),
                    (Some(at), _) => {
                        // Fighting: keep within range, strafing; walking into
                        // a wall climbs it, so a robot keeps off walls (and
                        // other robots), turning its strafe round, and won't
                        // press on toward something in the way.
                        let d = at.distance(position);
                        let mut advance = if d > far {
                            1.0
                        } else if d < near {
                            -1.0
                        } else {
                            0.0
                        };
                        if blocked(right * brain.strafe) {
                            brain.strafe = -brain.strafe;
                        }
                        if advance != 0.0 && blocked(forward * advance) {
                            advance = 0.0;
                        }
                        let strafe = if blocked(right * brain.strafe) { 0.0 } else { brain.strafe };
                        Vec3::new(strafe, 0.0, advance)
                    }
                    // Going somewhere: face the way it's walking.
                    (None, Walk::Toward(dir)) => {
                        let world = frame.map_or(dir, |f| f.rotation * dir);
                        input.add_look(turn(look_toward(body, pitch, world)));
                        input.sprint = brain.lead.is_some();
                        to_input(dir)
                    }
                    // Got there (or can't get anywhere).
                    _ => {
                        if brain.lead.is_some() {
                            search(&mut input, brain);
                        }
                        Vec3::ZERO
                    }
                }
            }
            Mode::Eva { .. } => {
                let goal = match aiming {
                    // Hold a point beside the target, swinging round it.
                    Some(at) => {
                        let toward = (at - position).normalize_or_zero();
                        let side = toward.any_orthonormal_vector() * brain.strafe;
                        Some(at - toward * (near + far) / 2.0 + side * 4.0)
                    }
                    None => goal.map(to_world),
                };
                let heading = goal.filter(|g| g.distance(position) > ARRIVED_EVA).and_then(|goal| steer_clear(&spatial, entity, position, goal));
                match heading {
                    Some(dir) => {
                        if aiming.is_none() {
                            input.add_look(turn(look_toward(body, pitch, dir)));
                        }
                        to_input(view, dir)
                    }
                    _ => {
                        if aiming.is_none() && brain.lead.is_some() {
                            search(&mut input, brain);
                        }
                        Vec3::ZERO
                    }
                }
            }
        };
    }
    // A robot that catches sight of someone tells the others near it.
    for (from, target) in sightings {
        let Some(at) = where_is(target) else { continue };
        for (entity, _, mut brain, _) in &mut robots {
            if brain.visible || brain.lead.is_some() && brain.since_lead < 1.0 {
                continue;
            }
            if where_is(entity).is_some_and(|p| p.distance(from) < CALL_RANGE) {
                let local = bodies.frame(brain.home).map_or(at, |f| f.point_to_local(at));
                brain.hear_of(target, local);
            }
        }
    }
}

/// The way to walk along `route` from `feet` (both in the grid's frame):
/// past the points already reached, toward the furthest of the next few it
/// can walk straight to. `None` when the route's done.
fn follow(grid: &NavGrid, route: &mut Vec<Vec3>, feet: Vec3) -> Option<Vec3> {
    let flat = |p: Vec3| Vec2::new(p.x - feet.x, p.z - feet.z).length();
    while route.first().is_some_and(|p| flat(*p) < REACHED && (p.y - feet.y).abs() < 1.0) {
        route.remove(0);
    }
    let next = (0..route.len().min(LOOK_AHEAD)).rev().find(|&i| grid.walkable(feet, route[i])).unwrap_or(0);
    let p = route.get(next)?;
    let dir = Vec3::new(p.x - feet.x, 0.0, p.z - feet.z);
    dir.try_normalize()
}

/// Which way a flyer at `from` should head for `goal`, turned aside from
/// anything in the way: along its surface (or round it, met head on), and
/// away from it the closer it is. `None` when what's in the way is right in
/// front of the goal (it's inside a hull, say): that's as near as it gets.
fn steer_clear(spatial: &SpatialQuery, me: Entity, from: Vec3, goal: Vec3) -> Option<Vec3> {
    let d = Dir3::new(goal - from).ok()?;
    let filter = SpatialQueryFilter::from_mask(Layer::World).with_excluded_entities([me]);
    let Some(hit) = spatial.cast_ray(from, d, AVOID_RANGE, true, &filter) else {
        return Some(*d);
    };
    let n = Vec3::from(hit.normal).normalize_or(-*d);
    let beyond = from.distance(goal) - hit.distance;
    if beyond > 0.0 && beyond < BEHIND_SURFACE {
        return None;
    }
    let mut along = *d - n * d.dot(n).min(0.0);
    if along.length() < 0.3 {
        // Head on: round it, either way.
        along = n.cross(Vec3::Y).try_normalize().unwrap_or_else(|| n.any_orthonormal_vector());
    }
    Some((along.normalize_or_zero() + n * (1.0 - hit.distance / AVOID_RANGE)).normalize_or_zero())
}

/// Whether a walker at `feet` is at `goal` (m, across the deck).
fn at_goal(feet: Vec3, goal: Vec3) -> bool {
    Vec2::new(goal.x - feet.x, goal.z - feet.z).length() < 1.0
}

/// What walking a route comes to this tick.
enum Walk {
    Toward(Vec3),
    Arrived,
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

/// How long after losing sight a robot still knows where its target went (s).
const GLIMPSE: f32 = 0.6;

/// How long word of a target lasts with nothing new (s).
const FORGET: f32 = 25.0;

/// How long a robot looks about where the trail ran cold (s), and how fast
/// it turns doing so (rad/s).
const SEARCH_SECONDS: f32 = 4.0;
const SEARCH_TURN: f32 = 1.4;

/// How far gunfire carries, through walls and all (m).
const HEARING: f32 = 30.0;

/// How far a robot's call carries when it spots someone (m).
const CALL_RANGE: f32 = 25.0;

/// How close (m, from a walking robot's middle) it lets walls come before
/// stepping away.
const WALL_CLEARANCE: f32 = 1.3;

/// Following a route: a point is reached within this (m, across the deck);
/// it looks this many points ahead for one to walk straight to; it plans
/// again this often (s), or when its goal moves this far (m).
const REACHED: f32 = 0.45;
const LOOK_AHEAD: usize = 6;
const REPLAN_SECONDS: f32 = 1.0;
const REPLAN_DISTANCE: f32 = 1.5;

/// A flyer is where it's going within this (m), and looks this far ahead
/// for things in its way.
const ARRIVED_EVA: f32 = 3.0;
const AVOID_RANGE: f32 = 6.0;
/// A flyer's goal less than this behind the surface in its way is as
/// reached as it can be (m).
const BEHIND_SURFACE: f32 = 12.0;

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
