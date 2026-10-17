//! The grapple run: a race round a three-kilometre loop of twelve rings
//! through a field of rocks, from just off our stern and back, against the
//! clock. Rings count only flown the way they face, and in order. No
//! enemies: it's the grapple's show. The course turns between each ring and
//! the next, and inside every turn is a big anchor rock: hook it and the
//! rope swings you round the turn at full speed (the jetpack alone has to
//! brake for it, and tops out at a third of a reel's speed). The rest of
//! the field is handholds to reel along the straights by, and scenery. The
//! anchors are fixed in place, so the line is the same every run; the rest
//! sit still until knocked (a field of thousands at rest costs the physics
//! nothing). The field is grown from a seed on every peer, not sent (see
//! [`RockField`]).
//!
//! Each runner's time runs from their start ring to the finish; the board
//! keeps their progress and time. It's won when the whole crew is through
//! the finish, lost if the clock runs out first. Best times are kept for as
//! long as the host runs.
//!
//! Results: `finished` (runners through the finish), and on a win
//! `crew_seconds` (the last of them home, from the mission's start).

use std::collections::{BTreeMap, HashMap};

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::{Scenario, Stage, after_banner};
use crate::sim::bodies::Bodies;
use crate::sim::bots::{BodyTuning, Robot, tuning_of};
use crate::sim::design::{ShipDesign, design};
use crate::sim::devices::DeviceKind;
use crate::sim::mission::{CrewTally, MarkedBodies, Marker, MarkerKind, Mission, Phase, ScenarioKind, Staged, clock, countdown};
use crate::sim::player::{DeckSlot, Player};
use crate::sim::progression::Profile;
use crate::sim::ship;
use crate::sim::tuning::Tuning;
use crate::sim::asteroids::{FieldLayout, RockField, RockPlace, random_unit};
use crate::sim::{SimTick, TICK_HZ};

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::GrappleRun,
    id: "run",
    label: "Grapple run",
    build,
    arrive: None,
    systems: |app| {
        app.init_resource::<RunRecords>();
        super::judge(app, ScenarioKind::GrappleRun, run_course);
    },
    return_after: after_banner,
    homecoming: false,
    ambience: crate::sim::ambience::Choice::named("grapple_run"),
};

/// The course's design.
pub const COURSE: &str = "course";
/// Where the course's origin (its start ring) is: 17 m off our stern.
pub const COURSE_AT: Vec3 = Vec3::new(0.0, 1.5, 32.0);
/// How long the crew has (s).
pub const TIME_LIMIT: f32 = 420.0;
/// Rocks in the field, anchors included. `HULLBREAKERS_RACE_ROCKS=n`
/// overrides it, for stress runs (see docs/perf.md).
pub const ROCKS: usize = 6000;
/// Of them, movers: big rocks adrift through the field from the start,
/// tumbling, stirring what they hit. Placed clear of the line like any
/// rock, then free to cross it: a race has its hazards.
pub const MOVERS: usize = 16;
const ROCK_SEED: u64 = 0x6a7e_0006;
/// How far the field reaches past the course's box (m).
const FIELD_MARGIN: f32 = 250.0;
/// A move this far in one tick is a teleport (a respawn), not a flight
/// through a ring.
const TELEPORT: f32 = 20.0;
/// A rock reaches about twice its radius (its lobes and its noise).
const REACH: f32 = 2.0;

/// The course's rings, in the order they're flown: indices into its
/// devices.
pub fn rings(design: &ShipDesign) -> Vec<usize> {
    design
        .devices
        .iter()
        .enumerate()
        .filter(|(_, d)| matches!(d.kind, DeviceKind::Ring { .. }))
        .map(|(i, _)| i)
        .collect()
}

/// Where the course turns (course coordinates): its `apex1`, `apex2`, ...
/// markers, one between each ring and the next.
pub fn apexes(design: &ShipDesign) -> Vec<Vec3> {
    (1..).map_while(|k| design.markers.get(&format!("apex{k}")).copied()).collect()
}

/// The racing line, in the world: the start ring, then each apex and the
/// ring after it, to the finish (the line as if every turn were a corner;
/// the swings cut inside them).
pub fn racing_line() -> Vec<Vec3> {
    let course = design(COURSE);
    let (rings, apexes) = (rings(course), apexes(course));
    let mut line = vec![course.devices[rings[0]].at];
    for (k, apex) in apexes.iter().enumerate() {
        line.push(*apex);
        line.push(course.devices[rings[k + 1]].at);
    }
    line.into_iter().map(|p| COURSE_AT + p).collect()
}

/// The anchor rock inside each turn (world centre, radius), one per apex:
/// big, and placed so a rope on it swings you from the leg in to the leg
/// out (the circle round it at its clearance touches both legs).
pub fn anchors() -> Vec<(Vec3, f32)> {
    let course = design(COURSE);
    let rings = rings(course);
    apexes(course)
        .iter()
        .enumerate()
        .map(|(k, &apex)| {
            let into = (apex - course.devices[rings[k]].at).normalize();
            let out = (course.devices[rings[k + 1]].at - apex).normalize();
            let turn = into.angle_between(out);
            // Bigger for the sharper turns: 12 m at 45 degrees, 19.5 at
            // 150.
            let radius = 9.0 + 4.0 * turn;
            // Its reach 12 m off both legs: on the bisector, as far out
            // as that takes (a hairpin's is far out, between its legs).
            let clearance = radius * REACH + 12.0;
            let reach = clearance / (turn / 2.0).cos();
            let inside = (out - into).normalize();
            (COURSE_AT + apex + inside * reach, radius)
        })
        .collect()
}

/// The distance from `p` to the nearest point of the polyline `line`.
fn off_line(line: &[Vec3], p: Vec3) -> f32 {
    line.windows(2)
        .map(|leg| {
            let d = leg[1] - leg[0];
            let t = ((p - leg[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0);
            p.distance(leg[0] + d * t)
        })
        .fold(f32::MAX, f32::min)
}

/// Rocks placed so far, bucketed by where they are, so a field of
/// thousands checks each new one against its neighbours only.
struct Placed {
    rocks: Vec<(Vec3, f32)>,
    cells: HashMap<IVec3, Vec<usize>>,
}

impl Placed {
    /// Big enough that two rocks that could touch are never more than a
    /// cell apart: two of the biggest (anchors of 19.5 m) need 79 m.
    const CELL: f32 = 80.0;

    fn cell(p: Vec3) -> IVec3 {
        (p / Self::CELL).floor().as_ivec3()
    }

    fn clear(&self, at: Vec3, radius: f32) -> bool {
        let c = Self::cell(at);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(near) = self.cells.get(&(c + IVec3::new(dx, dy, dz))) else { continue };
                    if near.iter().any(|&i| {
                        let (p, r) = self.rocks[i];
                        p.distance(at) < (r + radius) * REACH + 1.0
                    }) {
                        return false;
                    }
                }
            }
        }
        true
    }

    fn push(&mut self, at: Vec3, radius: f32) {
        self.cells.entry(Self::cell(at)).or_default().push(self.rocks.len());
        self.rocks.push((at, radius));
    }
}

/// How many rocks the field has: [`ROCKS`], or `HULLBREAKERS_RACE_ROCKS`.
pub fn rock_count() -> usize {
    std::env::var("HULLBREAKERS_RACE_ROCKS").ok().and_then(|v| v.parse().ok()).unwrap_or(ROCKS)
}

/// Where the field's `count` rocks are, in the world: the anchors (fixed),
/// then the movers (adrift), then handholds strung along the legs (25-90 m
/// off the line, something to reel by every few seconds), then the rest
/// scattered through the course's box and well past it. All clear of the
/// racing line (by their reach and more), of the rings, of our ship and of
/// each other, to begin with.
pub fn rock_places(count: usize) -> Vec<RockPlace> {
    let course = design(COURSE);
    let line = racing_line();
    let rings: Vec<Vec3> = rings(course).iter().map(|&i| COURSE_AT + course.devices[i].at).collect();
    let mut rng = ChaCha8Rng::seed_from_u64(ROCK_SEED);
    let mut placed = Placed {
        rocks: Vec::new(),
        cells: HashMap::new(),
    };
    let clear_of_course = |at: Vec3, radius: f32, margin: f32| {
        off_line(&line, at) >= radius * REACH + margin
            && rings.iter().all(|r| r.distance(at) >= radius * REACH + 30.0)
            && at.length() >= radius * REACH + 60.0
    };
    for (at, radius) in anchors() {
        placed.push(at, radius);
    }
    // The movers: through the course's box, 10-24 m, at 2-6 m/s any way,
    // turning slowly.
    let (lo, hi) = line.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), &p| (lo.min(p), hi.max(p)));
    let mut movers = Vec::new();
    for _ in 0..MOVERS * 40 {
        if movers.len() == MOVERS || placed.rocks.len() >= count {
            break;
        }
        let radius = rng.random_range(10.0..24.0);
        let at = Vec3::new(rng.random_range(lo.x..hi.x), rng.random_range(lo.y..hi.y), rng.random_range(lo.z..hi.z));
        if clear_of_course(at, radius, 20.0) && placed.clear(at, radius) {
            placed.push(at, radius);
            let velocity = random_unit(&mut rng) * rng.random_range(2.0..6.0);
            let spin = random_unit(&mut rng) * rng.random_range(0.02..0.08);
            movers.push((velocity, spin));
        }
    }
    // Handholds: every 40 m along each leg, maybe one off to a side.
    let mut handholds = Vec::new();
    for leg in line.windows(2) {
        let (from, d) = (leg[0], leg[1] - leg[0]);
        let (side, up) = d.normalize().any_orthonormal_pair();
        let steps = (d.length() / 40.0) as usize;
        for i in 1..steps {
            if rng.random::<f32>() < 0.4 {
                continue;
            }
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let off = rng.random_range(25.0..90.0);
            let radius = rng.random_range(1.5..6.0);
            handholds.push((from + d * (i as f32 / steps as f32) + (side * angle.cos() + up * angle.sin()) * off, radius));
        }
    }
    for (at, radius) in handholds {
        if placed.rocks.len() < count && clear_of_course(at, radius, 8.0) && placed.clear(at, radius) {
            placed.push(at, radius);
        }
    }
    // The rest, through the course's box and its margin.
    let (lo, hi) = (lo - Vec3::splat(FIELD_MARGIN), hi + Vec3::splat(FIELD_MARGIN));
    for _ in 0..count * 40 {
        if placed.rocks.len() >= count {
            break;
        }
        // Mostly small rocks, a few big ones.
        let radius = 1.0 * 9.0f32.powf(rng.random::<f32>().powi(2));
        let at = Vec3::new(rng.random_range(lo.x..hi.x), rng.random_range(lo.y..hi.y), rng.random_range(lo.z..hi.z));
        if clear_of_course(at, radius, 10.0) && placed.clear(at, radius) {
            placed.push(at, radius);
        }
    }
    let anchors = anchors().len();
    placed
        .rocks
        .into_iter()
        .enumerate()
        .map(|(i, (at, radius))| {
            let mut place = RockPlace::still(at, radius, i < anchors);
            if let Some(&(velocity, spin)) = i.checked_sub(anchors).and_then(|m| movers.get(m)) {
                place.velocity = velocity;
                place.spin = spin;
            }
            place
        })
        .collect()
}

fn build(stage: &mut Stage) -> Entity {
    let home = stage.commands.spawn(ship::parked_home_ship()).id();
    // The rocks are the course (grown with it, wherever it goes), so
    // they're there even when a test asks for a quiet field.
    let course = stage
        .commands
        .spawn((
            ship::site("Course", COURSE, COURSE_AT, Quat::IDENTITY),
            RockField {
                layout: FieldLayout::GrappleRun,
                seed: ROCK_SEED,
                count: rock_count() as u32,
                laid: stage.tick,
            },
        ))
        .id();
    stage.commands.spawn((
        Name::new("Grapple run"),
        Staged,
        Run {
            course,
            runners: BTreeMap::new(),
        },
    ));
    home
}

/// The run under way: the course, and how everyone's doing.
#[derive(Component)]
struct Run {
    course: Entity,
    runners: BTreeMap<Entity, Runner>,
}

struct Runner {
    name: String,
    /// Who they are, for their records: their profile's id (a player
    /// whose profile hasn't arrived is only themselves this run).
    who: u64,
    /// Their deck slot, which their markers go by.
    slot: u32,
    /// The ring they're to fly next (an index into the course's rings).
    next: usize,
    /// When they flew each ring (the first is their start).
    splits: Vec<u32>,
    finished: Option<u32>,
    /// Where they were last tick, in the course's coordinates.
    last: Option<Vec3>,
}

impl Runner {
    /// Their time (s) from their start ring to `at`.
    fn time(&self, at: u32) -> f32 {
        self.splits.first().map_or(0.0, |&start| at.wrapping_sub(start) as f32 / TICK_HZ as f32)
    }
}

/// The best times so far, kept by the host across runs: each runner's (by
/// their profile's id, not their name or slot: a new player in a slot, or
/// one who takes another's name, doesn't inherit a best), and the crew's
/// (the last of them home).
#[derive(Resource, Default)]
pub struct RunRecords {
    pub best: HashMap<u64, f32>,
    pub crew: Option<f32>,
}

#[allow(clippy::too_many_arguments)]
fn run_course(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    bodies: Bodies,
    run: Option<Single<&mut Run>>,
    players: Query<(Entity, &Player, Option<&Name>, Option<&BodyTuning>, Option<&DeckSlot>, Option<&Profile>), Without<Robot>>,
    marked: MarkedBodies,
    mission: Single<&mut Mission>,
    mut tally: ResMut<CrewTally>,
    mut records: ResMut<RunRecords>,
) {
    let Some(run) = run else {
        return;
    };
    let mut run = run.into_inner();
    let mut mission = mission.into_inner();
    let Some(course_frame) = bodies.frame(run.course) else {
        return;
    };
    let course = design(COURSE);
    let rings = rings(course);
    let started = mission.started;
    let since_start = |at: u32| at.wrapping_sub(started) as f32 / TICK_HZ as f32;
    let underway = mission.phase == Phase::Underway;
    for (entity, player, name, own, slot, profile) in &players {
        let who = profile.map(|p| p.id).filter(|id| *id != 0).unwrap_or(1 << 63 | entity.to_bits());
        let runner = run.runners.entry(entity).or_insert_with(|| Runner {
            name: String::new(),
            who,
            slot: slot.map_or(0, |s| s.0),
            next: 0,
            splits: Vec::new(),
            finished: None,
            last: None,
        });
        // As they're called now (a rename shows at once), and who they
        // are once their profile's in.
        runner.name = name.map_or("?".to_string(), |n| n.to_string());
        runner.who = who;
        let frame = player.mode.body().and_then(|b| bodies.frame(b));
        let (center, _) = player.mode.world_kinematics(tuning_of(&tuning, own), frame.as_ref());
        let here = course_frame.point_to_local(center);
        let last = runner.last.replace(here);
        if !underway || runner.finished.is_some() {
            continue;
        }
        let Some(last) = last.filter(|l| l.distance(here) < TELEPORT) else {
            continue;
        };
        if course.devices[rings[runner.next]].passed_through(last, here) {
            runner.next += 1;
            runner.splits.push(tick.0);
            if runner.next == rings.len() {
                runner.finished = Some(tick.0);
                let t = runner.time(tick.0);
                tally.credit(entity, format!("ran the course in {}", clock(t)));
                let line = match records.best.get(&runner.who) {
                    Some(&best) if t < best => format!("{}: {} — a personal best (was {})", runner.name, clock(t), clock(best)),
                    Some(&best) => format!("{}: {} (best {})", runner.name, clock(t), clock(best)),
                    None => format!("{}: {}", runner.name, clock(t)),
                };
                mission.report.push(line);
                let best = records.best.entry(runner.who).or_insert(t);
                *best = best.min(t);
            }
        }
    }
    // Players who left stop counting.
    run.runners.retain(|e, _| players.contains(*e));
    let crew: Vec<&Runner> = run.runners.values().collect();
    if underway && !crew.is_empty() && crew.iter().all(|r| r.finished.is_some()) {
        let last_home = crew.iter().filter_map(|r| r.finished).map(since_start).fold(0.0, f32::max);
        if crew.len() > 1 {
            let line = match records.crew {
                Some(best) if last_home < best => format!("The whole crew in {} — a crew record (was {})", clock(last_home), clock(best)),
                Some(best) => format!("The whole crew in {} (record {})", clock(last_home), clock(best)),
                None => format!("The whole crew in {}", clock(last_home)),
            };
            mission.report.push(line);
            records.crew = Some(records.crew.map_or(last_home, |b| b.min(last_home)));
        }
        mission.result("finished", crew.len() as f32);
        mission.result("crew_seconds", last_home);
        mission.decide(Phase::Won, tick.0, "COURSE CLEAR");
    } else if underway && mission.elapsed(tick.0) >= TIME_LIMIT {
        for r in crew.iter().filter(|r| r.finished.is_none()) {
            mission.report.push(format!("{}: out of time at ring {} of {}", r.name, r.next + 1, rings.len()));
        }
        mission.result("finished", crew.iter().filter(|r| r.finished.is_some()).count() as f32);
        mission.decide(Phase::Lost, tick.0, "OUT OF TIME");
    }
    // The board: the finished by time, then everyone else by how far
    // they've got (and who got there first). The clocks stop when the
    // run's decided.
    let now = mission.ended.unwrap_or(tick.0);
    let mut board: Vec<&Runner> = run.runners.values().collect();
    board.sort_by(|a, b| match (a.finished, b.finished) {
        (Some(x), Some(y)) => a.time(x).total_cmp(&b.time(y)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.next.cmp(&a.next).then(a.splits.last().cmp(&b.splits.last())).then(a.name.cmp(&b.name)),
    });
    let status = board
        .iter()
        .map(|r| match r.finished {
            Some(at) => format!("{:<10} {}", r.name, clock(r.time(at))),
            None if r.next == 0 => format!("{:<10} to the start", r.name),
            None => format!("{:<10} ring {}/{}  {}", r.name, r.next, rings.len(), clock(r.time(now))),
        })
        .collect();
    let course_id = marked.id(run.course);
    let markers = run
        .runners
        .values()
        .filter(|r| r.finished.is_none())
        .filter_map(|r| {
            let i = rings[r.next];
            let ring = Marker::device(course_id?, course, &course.devices[i].id, MarkerKind::Waypoint);
            Some(ring.for_player(r.slot))
        })
        .collect();
    let objective = match mission.phase {
        Phase::Underway => {
            format!("Fly the rings in order — {} left", countdown(TIME_LIMIT - mission.elapsed(tick.0)))
        }
        Phase::Won => "Course clear!".into(),
        Phase::Lost => "Out of time".into(),
    };
    Mission::show(&mut mission, objective, status, markers);
}
