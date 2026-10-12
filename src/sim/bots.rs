//! Robots: players driven by a brain instead of a person.
//!
//! A robot is a [`Player`] like any crew member, with the same boots,
//! jetpack, grapple, guns, and [`Vitals`]. Its own [`BodyTuning`] sets its
//! size, speed and toughness. On the authority a [`Brain`] writes its
//! [`PlayerInput`] each tick, before players are simulated, from what it can
//! see. Clients never simulate robots; they receive them like other remote
//! players, plus the [`Robot`] kind and its [`Affixes`] to know what to draw,
//! and its [`Condition`] for what its affixes are doing.
//!
//! Kinds: the **Gunner** (walks decks, rifle arm), the **Swarmer** (a fast
//! fragile drone), the **Grappler** (a heavy walker that harpoons a player,
//! reels them in hard and lets go: off their feet, out of cover, off the
//! hull) and the **Turret** (a gun on the hull, tracking EVA players with a
//! searchlight). On top of its kind a robot may have affixes (see
//! [`affixes`]), rolled when a ship is crewed (see [`crew`]).

mod affixes;
mod blast;
mod crew;

pub use affixes::{Affix, Affixes, Condition, absorb, robot_upkeep};

/// A robot destroyed: it's gone the tick after. Inserted (and replicated,
/// reliably, as its despawn is) so every peer knows it was destroyed and not
/// tidied away; in its last tick it's inert (no brain, no input).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wrecked;
pub use blast::{Detonation, blast_of, detonate};
pub use crew::{Posting, Threat, compose_crew, crew_ship, mount, spawn_zoo};

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
use super::weapons::{GunKind, Loadout, STANCES, Slot, Stance};
use super::{Layer, SimTick, TICK_HZ};

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Robot {
    /// Walks decks and hulls on mag-boots; a rifle arm; keeps its distance
    /// and strafes.
    Gunner,
    /// A small fast drone; rushes in and sprays at close range.
    Swarmer,
    /// A heavy walker with a harpoon arm and a winch: hooks a player, reels
    /// them in hard, and lets go, flinging them.
    Grappler,
    /// A gun on a hull, bolted down: tracks whoever crosses open space to
    /// its ship, leading them, by a searchlight.
    Turret,
    /// A small six-legged robot on magnetic feet that walks any surface:
    /// over the outer hull to whoever's crossing to it, up walls and across
    /// ceilings inside to come at the crew from above. Heard through the
    /// hull before it's seen.
    Crawler,
}

/// How a kind of robot gets about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gait {
    /// Stands on decks, walks the ship's floors.
    Walker,
    /// Flies about its ship.
    Flyer,
    /// Bolted to a hull surface; only turns.
    Mount,
    /// Walks any surface: floors, walls, ceilings, the outside of the hull.
    /// Posted on a hull surface, as a mount is.
    Crawler,
}

/// A body's own tuning, where it differs from the crew's.
#[derive(Component, Clone, Debug)]
pub struct BodyTuning(pub Tuning);

/// The tuning that applies to a body: its own, or the crew's.
pub fn tuning_of<'a>(base: &'a Tuning, own: Option<&'a BodyTuning>) -> &'a Tuning {
    own.map_or(base, |t| &t.0)
}

impl Robot {
    pub const ALL: [Robot; 5] = [Robot::Gunner, Robot::Swarmer, Robot::Grappler, Robot::Turret, Robot::Crawler];

    pub fn name(self) -> &'static str {
        match self {
            Robot::Gunner => "Gunner",
            Robot::Swarmer => "Swarmer",
            Robot::Grappler => "Grappler",
            Robot::Turret => "Turret",
            Robot::Crawler => "Crawler",
        }
    }

    pub fn gait(self) -> Gait {
        match self {
            Robot::Gunner | Robot::Grappler => Gait::Walker,
            Robot::Swarmer => Gait::Flyer,
            Robot::Turret => Gait::Mount,
            Robot::Crawler => Gait::Crawler,
        }
    }

    /// The crew's tuning, changed where this kind of robot is built
    /// differently (before any affixes: see [`body_tuning`]).
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
            Robot::Grappler => {
                t.player_mass = 300.0;
                t.capsule_radius = 0.42;
                t.capsule_height = 2.0;
                t.eye_height = 1.72;
                t.walk_speed = 2.4;
                t.sprint_speed = 3.8;
                t.player_health = 170.0;
                t.wall_walk = false;
                // Its boots anchor it while it reels: the winch out-pulls
                // anyone's boots, never its own.
                t.adhesion_accel = 90.0;
                t.shear_accel = 90.0;
                t.grapple_range = GRAPPLE_REACH + 5.0;
                t.reel_force = 14_000.0;
                t.reel_speed = 24.0;
                t.thrust_accel = 12.0;
            }
            Robot::Turret => {
                t.player_mass = 400.0;
                t.capsule_radius = 0.55;
                t.capsule_height = 1.3;
                t.eye_height = 0.95;
                t.walk_speed = 0.0;
                t.sprint_speed = 0.0;
                t.player_health = 150.0;
                t.wall_walk = false;
                // Bolted on: no manoeuvre of its ship shakes it loose.
                t.adhesion_accel = 1000.0;
                t.shear_accel = 1000.0;
            }
            Robot::Crawler => {
                t.player_mass = 60.0;
                t.capsule_radius = 0.3;
                t.capsule_height = 0.6;
                t.eye_height = 0.34;
                t.walk_speed = 4.5;
                t.sprint_speed = 6.5;
                t.walk_accel = 40.0;
                t.player_health = 50.0;
                // Any surface is somewhere to walk, and its magnets hold it
                // there upside down.
                t.wall_walk = true;
                t.adhesion_accel = 90.0;
                t.shear_accel = 90.0;
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
            Robot::Grappler => (WEAK_GRAPPLER, 0.15),
            Robot::Turret => (WEAK_TURRET, 0.17),
            Robot::Crawler => (WEAK_CRAWLER, 0.08),
        }
    }

    /// Its gun, if it has one (the Grappler has its harpoon instead).
    fn gun(self, affixes: Affixes) -> Option<GunKind> {
        match self {
            Robot::Gunner if affixes.has(Affix::Marksman) => Some(GunKind::MarksmanRifle),
            Robot::Gunner => Some(GunKind::GunnerRifle),
            Robot::Swarmer => Some(GunKind::SwarmerSprayer),
            Robot::Grappler => None,
            Robot::Turret => Some(GunKind::TurretCannon),
            Robot::Crawler => Some(GunKind::SwarmerSprayer),
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
                lead: false,
                calls: CALL_RANGE,
            },
            Robot::Swarmer => Mind {
                sight: 120.0,
                turn_rate: 5.0,
                aim_error: 0.06,
                burst_on: 0.35,
                burst_off: 1.4,
                fire_cone: 0.15,
                range: (5.0, 12.0),
                lead: false,
                calls: CALL_RANGE,
            },
            Robot::Grappler => Mind {
                sight: 60.0,
                turn_rate: 1.8,
                aim_error: 0.015,
                burst_on: 0.0,
                burst_off: 1.0,
                fire_cone: 0.05,
                range: (6.0, GRAPPLE_REACH - 4.0),
                lead: false,
                calls: CALL_RANGE,
            },
            Robot::Crawler => Mind {
                sight: 60.0,
                turn_rate: 4.0,
                aim_error: 0.05,
                burst_on: 0.4,
                burst_off: 1.0,
                fire_cone: 0.15,
                range: (0.0, CRAWLER_REACH),
                lead: false,
                calls: CALL_RANGE,
            },
            Robot::Turret => Mind {
                sight: 320.0,
                turn_rate: 0.9,
                aim_error: 0.004,
                burst_on: 0.9,
                burst_off: 1.4,
                fire_cone: 0.04,
                range: (0.0, 300.0),
                lead: true,
                calls: CALL_RANGE,
            },
        }
    }
}

/// A robot's tuning: its kind's, changed by its affixes.
pub fn body_tuning(kind: Robot, affixes: Affixes, base: &Tuning) -> Tuning {
    let mut t = kind.tuning(base);
    affixes::apply_tuning(&mut t, affixes);
    t
}

/// How a kind of robot thinks.
#[derive(Clone, Copy, Debug)]
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
    /// Whether it aims ahead of a moving target, by its bullets' flight
    /// time (slow shells at long range need it).
    lead: bool,
    /// How far its call carries when it spots someone (m).
    calls: f32,
}

/// A telegraphed attack, in progress: the marksman's aimed shot, the
/// grappler's throw.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Attack {
    #[default]
    Ready,
    /// Winding up (seconds so far): the marksman holding its aim, the
    /// grappler drawing back.
    Winding(f32),
    /// The grappler's hook is in someone (seconds so far).
    Reeling(f32),
    /// Resting before it can again (seconds left).
    Cooldown(f32),
}

/// A robot's state of mind, and the state of its affixes. Authority only.
#[derive(Component, Clone, Debug)]
pub struct Brain {
    /// Who it's after.
    pub target: Option<Entity>,
    /// On show, not fighting: it holds still and thinks nothing (the
    /// gallery's robots). Its affixes still work.
    pub dormant: bool,
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
    attack: Attack,
    /// A crawler's way about (see [`crawl`]).
    crawling: Crawling,
    /// What its affixes are doing (see [`affixes`]).
    pub(crate) state: affixes::State,
}

/// How a crawler is getting about: where its feet were a moment ago (in its
/// body's frame) and how long until it looks again, how long it has left
/// wandering sideways to get unstuck, whether it's on its way round to its
/// ship's way in (its goal is on the other side of the hull), and a climb
/// it's committed to: the way to a wall (in its body's frame), from the
/// surface it was on then (its up), and how long it keeps at it.
#[derive(Clone, Copy, Debug, Default)]
struct Crawling {
    from: Vec3,
    check: f32,
    wander: f32,
    via_entry: bool,
    climb: Option<(Vec3, Vec3, f32)>,
}

impl Brain {
    fn new(home: Entity, post: Vec3, index: u32, state: affixes::State) -> Brain {
        Brain {
            target: None,
            dormant: false,
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
            attack: Attack::Ready,
            crawling: Crawling::default(),
            state,
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
        if let Some(t) = &mut self.state.last_hit_by {
            *t = mapper.get_mapped(*t);
        }
        self.home = mapper.get_mapped(self.home);
    }
}

/// A new robot of `kind` with `affixes`, in `mode`, guarding `home` from
/// `post` (in home's frame), where it returns with nothing to fight.
/// `index` varies its timing.
pub fn new_robot(kind: Robot, affixes: Affixes, mode: Mode, home: Entity, post: Vec3, base: &Tuning, index: u32) -> impl Bundle {
    let tuning = body_tuning(kind, affixes, base);
    let loadout = match kind.gun(affixes) {
        Some(gun) => Loadout::robot(gun),
        None => {
            // The grappler's harpoon is its grapple, in its main hand.
            let mut l = Loadout::base();
            l.stances = [Stance {
                main: Slot::Grapple,
                off: Slot::Empty,
            }; STANCES];
            l.current = 0;
            l
        }
    };
    let state = affixes::State::fresh(affixes, &tuning);
    (
        Name::new(kind.name()),
        kind,
        affixes,
        Condition::of(&state, affixes),
        Brain::new(home, post, index, state),
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

/// Every peer: a robot gets its own tuning and a capsule sized to it, once
/// its kind is known (and again once its affixes are, which arrive with it
/// on a client but may be inserted after).
pub fn add_body_tuning(add: On<Add, Robot>, robots: Query<(&Robot, Option<&Affixes>)>, base: Res<Tuning>, commands: Commands) {
    fit_body(add.entity, &robots, &base, commands);
}

pub fn add_affix_tuning(add: On<Add, Affixes>, robots: Query<(&Robot, Option<&Affixes>)>, base: Res<Tuning>, commands: Commands) {
    fit_body(add.entity, &robots, &base, commands);
}

fn fit_body(entity: Entity, robots: &Query<(&Robot, Option<&Affixes>)>, base: &Tuning, mut commands: Commands) {
    let Ok((kind, affixes)) = robots.get(entity) else { return };
    let tuning = body_tuning(*kind, affixes.copied().unwrap_or_default(), base);
    commands.entity(entity).insert((capsule(&tuning), BodyTuning(tuning)));
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

type RobotQuery<'a> = (
    Entity,
    &'a Robot,
    &'a Affixes,
    &'a Condition,
    &'a mut Brain,
    &'a mut PlayerInput,
);

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
    hulls: Query<(&ShipHull, Option<&super::devices::DoorsBuilt>)>,
    mut grids: ResMut<NavGrids>,
    mut damage: MessageReader<Damage>,
    mut robots: Query<RobotQuery>,
) {
    let dt = time.delta_secs();
    let now = tick.0 as f32 / TICK_HZ as f32;
    let motion_of = |e: Entity| -> Option<(Vec3, Vec3)> {
        let (_, p, _, t) = everyone.get(e).ok()?;
        let f = p.mode.body().and_then(|b| bodies.frame(b));
        Some(p.mode.world_kinematics(tuning_of(&base, t), f.as_ref()))
    };
    let where_is = |e: Entity| motion_of(e).map(|(p, _)| p);
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
    // Robots that caught sight of someone just now: where, who, and how far
    // their call carries.
    let mut sightings: Vec<(Vec3, Entity, f32)> = Vec::new();
    for (entity, kind, affixes, condition, mut brain, mut input) in &mut robots {
        let Ok((_, me, _, own)) = everyone.get(entity) else {
            continue;
        };
        let brain = &mut *brain;
        if brain.dormant {
            // Holding still: hands and jets idle.
            input.movement = Vec3::ZERO;
            input.main.held = false;
            input.reel_in = false;
            continue;
        }
        let tuning = tuning_of(&base, own);
        let mind = affixes::mind(kind.mind(), *kind, *affixes, condition.rallied);
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
        let (position, own_velocity) = me.mode.world_kinematics(tuning, frame.as_ref());
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
                    sightings.push((position, t, mind.calls));
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
        let target_motion = brain.target.and_then(motion_of);
        let target_at = target_motion.map(|(p, _)| p);
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

        // Aim: in sight, turn toward the target (off by a wandering error),
        // at the robot's turn rate; a slow gun leads a moving target.
        let step = mind.turn_rate * dt;
        let turn = |want: Vec2| want.clamp(Vec2::splat(-step), Vec2::splat(step));
        let aiming = target_motion.filter(|_| brain.visible);
        let distance = target_at.map(|at| at.distance(position));
        // A marksman holds its aim still for the last moment of its wind-up,
        // so a target that moves then makes it miss.
        let locked = matches!(brain.attack, Attack::Winding(t) if affixes.has(Affix::Marksman) && t > MARKSMAN_AIM - MARKSMAN_LOCK);
        let mut on_target = false;
        if let Some((at, velocity)) = aiming {
            let mut aim_at = at;
            if mind.lead
                && let Some(speed) = me.loadout.inventory.weapons[0].map(|w| w.stats().muzzle_speed)
            {
                // The shell keeps the shooter's velocity: lead by the
                // target's motion relative to ours.
                aim_at += (velocity - own_velocity) * (at.distance(eye) / speed);
            }
            let wobble = Vec2::new((now * 1.3 + brain.seed).sin(), (now * 0.9 + brain.seed * 2.0).cos()) * mind.aim_error;
            let want = look_toward(body, pitch, aim_at - eye) + wobble;
            if !locked {
                input.add_look(turn(want));
            }
            // On target by the angle off it (yaw and pitch alone mislead
            // where the target is nearly along its up: a crawler looking
            // up off a wall), allowing for its aim's own wander.
            on_target = (view * Vec3::NEG_Z).angle_between(aim_at - eye) < mind.fire_cone + mind.aim_error * 1.5;
        }
        input.sprint = false;
        input.reel_in = false;
        // Fire control: in bursts while on target, or by the kind's or
        // affix's own way of attacking.
        let venting = brain.state.venting();
        let marksman = affixes.has(Affix::Marksman);
        input.main.held = false;
        if venting {
            // Overheated: its guns are cold until it's vented.
            brain.attack = Attack::Ready;
        } else if *kind == Robot::Grappler {
            grapple_attack(brain, &mut input, me, aiming.map(|(p, _)| p), position, on_target, dt);
        } else if marksman {
            brain.attack = match brain.attack {
                Attack::Ready | Attack::Reeling(_) if on_target => Attack::Winding(0.0),
                Attack::Ready | Attack::Reeling(_) => Attack::Ready,
                // Lost them: the shot's off.
                Attack::Winding(_) if aiming.is_none() => Attack::Ready,
                Attack::Winding(t) if t + dt >= MARKSMAN_AIM => {
                    input.main.held = true;
                    Attack::Cooldown(MARKSMAN_REST)
                }
                Attack::Winding(t) => Attack::Winding(t + dt),
                Attack::Cooldown(t) if t - dt <= 0.0 => Attack::Ready,
                Attack::Cooldown(t) => Attack::Cooldown(t - dt),
            };
        } else {
            brain.burst = (brain.burst + dt) % (mind.burst_on + mind.burst_off);
            let in_reach = *kind != Robot::Crawler || distance.is_some_and(|d| d < CRAWLER_REACH * 2.0);
            input.main.held = on_target && in_reach && brain.burst < mind.burst_on;
        }
        // Standing its ground: venting, or in the middle of an attack that
        // needs it braced.
        let braced = venting || matches!(brain.attack, Attack::Winding(_) | Attack::Reeling(_));

        brain.strafe_timer -= dt;
        if brain.strafe_timer <= 0.0 {
            brain.strafe = if brain.strafe == 0.0 { 1.0 } else { -brain.strafe };
            brain.strafe_timer = 1.5 + (brain.seed * 7.3 + now).sin().abs() * 2.0;
        }
        let (near, far) = mind.range;
        // Where it's going, in its home's frame: after the target, to where
        // it was last known, or back to its post.
        let goal = match (aiming, brain.lead) {
            (Some(_), _) if distance.is_some_and(|d| d <= far) => None,
            (_, Some(lead)) => Some(lead),
            (None, None) => Some(brain.post),
            (Some((at, _)), None) => Some(to_home(at)),
        };
        // Looking about where the trail ran cold.
        let search = |input: &mut PlayerInput, brain: &mut Brain| {
            brain.searching += dt;
            let dir = if brain.seed.sin() > 0.0 { 1.0 } else { -1.0 };
            input.add_look(Vec2::new(dir * SEARCH_TURN * dt, turn(Vec2::new(0.0, pitch)).y));
        };
        if kind.gait() == Gait::Mount {
            // Bolted down: it only turns. With nothing in sight its light
            // sweeps the sky above it, back and forth.
            input.movement = Vec3::ZERO;
            if aiming.is_none() {
                let sweep = if (now * SWEEP_RATE / std::f32::consts::PI + brain.seed).sin() > 0.0 { 1.0 } else { -1.0 };
                input.add_look(Vec2::new(sweep * SWEEP_RATE * dt, turn(Vec2::new(0.0, pitch - SWEEP_PITCH)).y));
            }
            continue;
        }
        if braced {
            input.movement = Vec3::ZERO;
            continue;
        }
        if kind.gait() == Gait::Crawler
            && let Mode::Booted { feet, up, orientation, .. } = me.mode
            && let Some(f) = frame
        {
            // Over whatever surface it's on: to a perch above whoever it can
            // see, or where it last knew them to be, or its post.
            let ship_up = home.as_ref().map_or(Vec3::Y, |h| h.rotation * Vec3::Y);
            let (goal, under_ceiling) = match aiming {
                Some((at, _)) => perch(&spatial, at, ship_up),
                None => (to_world(brain.lead.unwrap_or(brain.post)), false),
            };
            let entry = hulls.get(brain.home).ok().and_then(|h| h.0.design().markers.get("entry").copied()).map(to_world);
            let dir = crawl(&mut brain.crawling, &spatial, entity, &f, feet, up, position, goal, under_ceiling, entry, dt);
            if aiming.is_none()
                && let Some(d) = dir
            {
                input.add_look(turn(look_toward(body, pitch, d)));
            }
            input.sprint = aiming.is_some() || brain.lead.is_some();
            input.movement = dir.map_or(Vec3::ZERO, |d| {
                let local = (f.rotation * orientation).inverse() * d;
                Vec3::new(local.x, 0.0, -local.z).normalize_or_zero()
            });
            continue;
        }
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
                // (As its doors and bridges are just now.)
                let grid = (on == brain.home)
                    .then(|| hulls.get(on).ok())
                    .flatten()
                    .map(|(h, built)| grids.get(&h.0, built.map(|b| b.0.as_slice())));
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
                    (Some((at, _)), _) => {
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
                    Some((at, _)) => {
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
    for (from, target, reach) in sightings {
        let Some(at) = where_is(target) else { continue };
        for (entity, _, _, _, mut brain, _) in &mut robots {
            if brain.dormant || brain.visible || brain.lead.is_some() && brain.since_lead < 1.0 {
                continue;
            }
            if where_is(entity).is_some_and(|p| p.distance(from) < reach) {
                let local = bodies.frame(brain.home).map_or(at, |f| f.point_to_local(at));
                brain.hear_of(target, local);
            }
        }
    }
}

/// The grappler's attack: on target and within reach, it draws back (a
/// moment's warning), throws its hook, and if the hook takes the target,
/// reels them in hard until they're on top of it or it's had long enough;
/// then it lets go and rests. A throw that misses, or takes anything else,
/// is let go at once.
fn grapple_attack(brain: &mut Brain, input: &mut PlayerInput, me: &Player, target: Option<Vec3>, position: Vec3, on_target: bool, dt: f32) {
    let hooked = me.grapple.map(|g| g.target);
    let distance = target.map(|t| t.distance(position));
    let in_reach = distance.is_some_and(|d| d < GRAPPLE_REACH);
    brain.attack = match brain.attack {
        Attack::Ready if on_target && in_reach => Attack::Winding(0.0),
        Attack::Ready => {
            // Never holding a rope it didn't mean to.
            if hooked.is_some() {
                input.presses.main.press();
            }
            Attack::Ready
        }
        Attack::Winding(_) if !in_reach => Attack::Ready,
        Attack::Winding(t) if t + dt >= GRAPPLE_WIND_UP => {
            input.presses.main.press();
            Attack::Reeling(0.0)
        }
        Attack::Winding(t) => Attack::Winding(t + dt),
        Attack::Reeling(t) => {
            // The throw has been made (this tick's press is seen by the hands
            // this tick, so the hook is in by the next).
            let on_them = hooked.is_some_and(|h| Some(h) == brain.target);
            let done = t > GRAPPLE_REEL_SECONDS || distance.is_none_or(|d| d < GRAPPLE_LET_GO);
            if t > 0.0 && (!on_them || done) {
                if hooked.is_some() {
                    input.presses.main.press();
                }
                Attack::Cooldown(if on_them { GRAPPLE_REST } else { GRAPPLE_REST / 2.0 })
            } else {
                input.reel_in = on_them;
                Attack::Reeling(t + dt)
            }
        }
        Attack::Cooldown(t) if t - dt <= 0.0 => Attack::Ready,
        Attack::Cooldown(t) => Attack::Cooldown(t - dt),
    };
}

/// Where a crawler goes for a target at `at`: the ceiling above them, if
/// there's one within reach (it comes at them from above: the point is
/// just into the ceiling, so making for it carries the crawler up the wall
/// and onto it), else the target itself. And whether it was a ceiling.
fn perch(spatial: &SpatialQuery, at: Vec3, ship_up: Vec3) -> (Vec3, bool) {
    let Ok(up) = Dir3::new(ship_up) else { return (at, false) };
    match spatial.cast_ray(at, up, PERCH_REACH, true, &SpatialQueryFilter::from_mask(Layer::World)) {
        Some(hit) => (at + ship_up * (hit.distance + 0.25), true),
        None => (at, false),
    }
}

/// Which way (world, along the surface) a crawler at `position`, its `feet`
/// and `up` in `frame`, walks for `goal`; `None` to hold where it is.
///
/// Along the surface toward it: walking into a wall climbs it and walking
/// off an edge wraps round it (the boots do both), so heading straight for
/// the goal takes it up walls, across ceilings and round the hull. Under a
/// goal that's above it (a perch under a ceiling), it makes for the nearest
/// wall to climb. A goal on the far side of the surface it's on (the crew
/// inside, it outside) sends it round to its ship's way in (`entry`) first.
/// Making no headway, it wanders sideways a moment.
#[allow(clippy::too_many_arguments)]
fn crawl(
    c: &mut Crawling,
    spatial: &SpatialQuery,
    me: Entity,
    frame: &super::frame::BodyFrame,
    feet: Vec3,
    up: Vec3,
    position: Vec3,
    goal: Vec3,
    under_ceiling: bool,
    entry: Option<Vec3>,
    dt: f32,
) -> Option<Vec3> {
    let up_w = (frame.rotation * up).normalize_or(Vec3::Y);
    let along = |to: Vec3| to - up_w * to.dot(up_w);
    let to = goal - position;
    // Round to the way in while the goal's through the hull.
    if c.via_entry && entry.is_none_or(|e| e.distance(position) < 1.5) {
        c.via_entry = false;
    }
    let to = match entry {
        Some(e) if c.via_entry => e - position,
        _ => to,
    };
    let flat = along(to);
    // A climb under way holds until it's on the wall (its surface changed),
    // or it's been at it long enough.
    if let Some((_, from_up, left)) = &mut c.climb {
        *left -= dt;
        if *left <= 0.0 || from_up.dot(up) < 0.99 {
            c.climb = None;
        }
    }
    let want = if let Some((dir, ..)) = c.climb {
        Some(frame.rotation * dir)
    } else if to.dot(up_w) > 1.0 && under_ceiling && flat.length() < 1.5 {
        // Right under its perch: up the nearest wall, all the way.
        let wall = nearest_wall(spatial, me, position, up_w);
        c.climb = wall.map(|w| (frame.rotation.inverse() * w, up, CLIMB_SECONDS));
        wall
    } else if flat.length() > 0.8 {
        flat.try_normalize()
    } else if to.dot(up_w) < -1.0 && !c.via_entry && entry.is_some() {
        // On the wrong side of the hull from it: round by the way in.
        c.via_entry = true;
        entry.map(|e| along(e - position)).and_then(|d| d.try_normalize())
    } else {
        None
    };
    // Stuck (against something it can't climb, in a corner): sideways a
    // moment.
    c.check -= dt;
    if c.check <= 0.0 {
        if want.is_some() && feet.distance(c.from) < CRAWL_STUCK {
            c.wander = CRAWL_WANDER;
        }
        c.from = feet;
        c.check = CRAWL_CHECK;
    }
    if c.wander > 0.0 {
        c.wander -= dt;
        let base = want.unwrap_or_else(|| up_w.any_orthonormal_vector());
        return up_w.cross(base).try_normalize();
    }
    want
}

/// The way (along the surface with normal `up`) to the nearest wall within
/// reach of `position`, if there's one.
fn nearest_wall(spatial: &SpatialQuery, me: Entity, position: Vec3, up: Vec3) -> Option<Vec3> {
    let first = up.any_orthonormal_vector();
    let filter = SpatialQueryFilter::from_mask(Layer::World).with_excluded_entities([me]);
    (0..8)
        .filter_map(|k| {
            let dir = Quat::from_axis_angle(up, k as f32 * std::f32::consts::FRAC_PI_4) * first;
            let hit = spatial.cast_ray(position, Dir3::new(dir).ok()?, WALL_SEEK, true, &filter)?;
            Some((hit.distance, dir))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, dir)| dir)
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

/// Weak spot centres (see [`Robot::weak_spot`]).
// Gunner: its backpack core, SOCKET_weak at (0, 1.42, 0.31) from its feet,
// its middle 0.95 up.
const WEAK_GUNNER: Vec3 = Vec3::new(0.0, 0.47, 0.31);
// Swarmer: its reactor between its rear thrusters, SOCKET_weak at
// (0, 0.035, 0.19) from its middle (its origin).
const WEAK_SWARMER: Vec3 = Vec3::new(0.0, 0.035, 0.19);
// Grappler: the winch's power cell in its back, SOCKET_weak at
// (0, 1.30, 0.40) from its feet, its middle 1.0 up.
const WEAK_GRAPPLER: Vec3 = Vec3::new(0.0, 0.30, 0.40);
// Turret: the coolant core behind its gun, SOCKET_weak at (0, 0.86, 0.56)
// from its base, its middle 0.65 up.
const WEAK_TURRET: Vec3 = Vec3::new(0.0, 0.21, 0.56);

// Crawler: the core in its back, SOCKET_weak at (0, 0.36, 0.12) from its
// feet, its middle 0.3 up.
const WEAK_CRAWLER: Vec3 = Vec3::new(0.0, 0.06, 0.12);

/// A crawler sprays what's within this (m) of it.
pub const CRAWLER_REACH: f32 = 9.0;
/// How far above a target a crawler looks for a ceiling to come at them
/// from (m), and how far round it looks for a wall to climb to it (m).
const PERCH_REACH: f32 = 4.5;
const WALL_SEEK: f32 = 8.0;
/// A crawler that's moved less than this (m) in this long (s) is stuck,
/// and wanders sideways this long (s).
const CRAWL_STUCK: f32 = 0.25;
const CRAWL_CHECK: f32 = 1.0;
const CRAWL_WANDER: f32 = 1.2;
/// How long a crawler keeps making for a wall to climb it (s).
const CLIMB_SECONDS: f32 = 4.0;

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

/// A marksman holds its aim this long before each shot (s), keeping it
/// still for the last part (s), and rests this long after (s).
pub const MARKSMAN_AIM: f32 = 1.1;
pub const MARKSMAN_LOCK: f32 = 0.3;
const MARKSMAN_REST: f32 = 0.9;

/// The grappler throws its hook at someone within this (m), after drawing
/// back this long (s); reels them in for at most this long (s) or until
/// they're this close (m); then rests (s).
pub const GRAPPLE_REACH: f32 = 28.0;
pub const GRAPPLE_WIND_UP: f32 = 0.7;
const GRAPPLE_REEL_SECONDS: f32 = 1.3;
const GRAPPLE_LET_GO: f32 = 3.0;
const GRAPPLE_REST: f32 = 4.0;

/// A turret with nothing in sight sweeps its light at this rate (rad/s),
/// this far above its horizon (rad, as pitch: negative is up).
const SWEEP_RATE: f32 = 0.35;
const SWEEP_PITCH: f32 = -0.3;

/// A world direction as jetpack input (x right, y up, z forward in the
/// view), full strength.
fn to_input(view: Quat, dir: Vec3) -> Vec3 {
    let local = view.inverse() * dir.normalize_or_zero();
    Vec3::new(local.x, local.y, -local.z)
}

/// Authority: a robot out of health is destroyed: marked [`Wrecked`] (and
/// stilled), and gone the tick after. A volatile one goes up (see
/// [`blast`]).
pub fn settle_robot_deaths(
    base: Res<Tuning>,
    bodies: Bodies,
    robots: Query<(Entity, &Robot, &Affixes, &Player, &Vitals, Option<&Brain>, Option<&BodyTuning>, Has<Wrecked>)>,
    mut detonations: MessageWriter<Detonation>,
    mut commands: Commands,
) {
    for (entity, kind, affixes, player, vitals, brain, own, wrecked) in &robots {
        if wrecked {
            commands.entity(entity).despawn();
            continue;
        }
        if vitals.health > 0.0 {
            continue;
        }
        if affixes.has(Affix::Volatile) {
            let frame = player.mode.body().and_then(|b| bodies.frame(b));
            let (at, _) = player.mode.world_kinematics(tuning_of(&base, own), frame.as_ref());
            detonations.write(blast_of(*kind, at, entity, brain.and_then(|b| b.state.last_hit_by)));
        }
        commands.entity(entity).insert(Wrecked).remove::<(Brain, PlayerInput)>();
    }
}

/// Debug: robots to spawn by our ship, for trying out a fight. Read by the
/// authority (so only the host's buttons work).
#[derive(Message, Clone, Copy, Debug)]
pub struct SpawnRobots(pub Robot);

/// Authority: a walker on our bridge, facing aft; or a pack of three
/// Swarmers 40 m off our port side; or a turret on our roof.
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
        match kind.gait() {
            Gait::Walker | Gait::Mount | Gait::Crawler => {
                let feet = Vec3::new(0.0, 0.01, -11.0);
                let mode = Mode::Booted {
                    body: *home,
                    feet,
                    up: Vec3::Y,
                    velocity: Vec3::ZERO,
                    orientation: Quat::from_rotation_y(std::f32::consts::PI),
                    pitch: 0.0,
                };
                commands.spawn(new_robot(*kind, Affixes::NONE, mode, *home, feet, &tuning, *spawned));
            }
            Gait::Flyer => {
                for i in 0..3 {
                    let post = Vec3::new(-40.0, 2.0 * i as f32, -4.0 + 4.0 * i as f32);
                    let position = frame.point_to_world(post);
                    let mode = Mode::Eva {
                        position,
                        velocity: frame.point_velocity(position),
                        orientation: frame.rotation,
                        pitch: 0.0,
                    };
                    commands.spawn(new_robot(*kind, Affixes::NONE, mode, *home, post, &tuning, *spawned * 3 + i));
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
