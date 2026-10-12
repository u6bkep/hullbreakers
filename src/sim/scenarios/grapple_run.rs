//! The grapple run: ten gates on a switchback just off our stern, a
//! hairpin round a pylon after each, in a cluster of rocks, against the
//! clock. Gates count only flown the way they face. No enemies: it's the
//! grapple's show (a rope swings you round a hairpin at full speed; the
//! jetpack has to brake for it). Everyone flies the gates in order; the board
//! keeps each runner's progress and time. It's won when the whole crew is
//! through the last gate, lost if the clock runs out first. Best times are
//! kept for as long as the host runs.
//!
//! Results: `finished` (runners through the last gate), and on a win
//! `crew_seconds` (the last of them home).

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
use crate::sim::{SimTick, TICK_HZ, asteroids};

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
};

/// The course's design.
pub const COURSE: &str = "course";
/// Where the course's origin (its first gate) is: 17 m off our stern.
pub const COURSE_AT: Vec3 = Vec3::new(0.0, 1.5, 32.0);
/// How long the crew has (s).
pub const TIME_LIMIT: f32 = 240.0;
/// Rocks in the cluster, and how far out from the course they lie (m).
const ROCKS: usize = 240;
const ROCK_SPREAD: f32 = 45.0;
const ROCK_SEED: u64 = 0x6a7e_0005;
/// A move this far in one tick is a teleport (a respawn), not a flight
/// through a gate.
const TELEPORT: f32 = 20.0;

/// The course's gates, in the order they're flown: indices into its
/// devices.
pub fn gates(design: &ShipDesign) -> Vec<usize> {
    design
        .devices
        .iter()
        .enumerate()
        .filter(|(_, d)| matches!(d.kind, DeviceKind::Gate { .. }))
        .map(|(i, _)| i)
        .collect()
}

/// Where the cluster's rocks are (centre, radius): around the course, in
/// a shell outside the box its gates and pylons fill (the course is flown
/// on its pylons; the rocks are the scenery, and anchors for a wide line),
/// clear of our ship and of each other.
pub fn rock_places() -> Vec<(Vec3, f32)> {
    let course = design(COURSE);
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for &i in &gates(course) {
        let at = COURSE_AT + course.devices[i].at;
        (lo, hi) = (lo.min(at), hi.max(at));
    }
    for b in &course.boxes {
        (lo, hi) = (lo.min(COURSE_AT + b.center - b.size / 2.0), hi.max(COURSE_AT + b.center + b.size / 2.0));
    }
    // The gates' openings and frames, and room to swing wide.
    let (lo, hi) = (lo - Vec3::splat(10.0), hi + Vec3::splat(10.0));
    let mut rng = ChaCha8Rng::seed_from_u64(ROCK_SEED);
    let mut placed: Vec<(Vec3, f32)> = Vec::new();
    // A rock reaches about twice its radius (its lobes and its noise).
    const REACH: f32 = 2.0;
    for _ in 0..ROCKS * 40 {
        if placed.len() == ROCKS {
            break;
        }
        // Mostly small rocks, a few big ones.
        let radius = 1.0 * 9.0f32.powf(rng.random::<f32>().powi(2));
        let at = Vec3::new(
            rng.random_range(lo.x - ROCK_SPREAD..hi.x + ROCK_SPREAD),
            rng.random_range(lo.y - ROCK_SPREAD..hi.y + ROCK_SPREAD),
            rng.random_range(lo.z - ROCK_SPREAD..hi.z + ROCK_SPREAD),
        );
        // Outside the course's box by its reach.
        let outside = (lo - at).max(at - hi).max_element();
        if outside < radius * REACH {
            continue;
        }
        if at.distance(Vec3::ZERO) < radius * REACH + 40.0 {
            continue;
        }
        if placed.iter().any(|&(p, r)| p.distance(at) < (r + radius) * REACH + 1.0) {
            continue;
        }
        placed.push((at, radius));
    }
    placed
}

fn build(stage: &mut Stage) -> Entity {
    let home = stage.commands.spawn(ship::parked_home_ship()).id();
    let course = stage.commands.spawn(ship::site("Course", COURSE, COURSE_AT, Quat::IDENTITY)).id();
    // The rocks are the course, so they're there even when a test asks
    // for a quiet field.
    asteroids::spawn_rocks(stage.commands, ROCK_SEED, &rock_places());
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
    /// The gate they're to fly next (an index into the course's gates).
    next: usize,
    /// When they flew each gate.
    splits: Vec<u32>,
    finished: Option<u32>,
    /// Where they were last tick, in the course's coordinates.
    last: Option<Vec3>,
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
    let gates = gates(course);
    let started = mission.started;
    let time = |at: u32| at.wrapping_sub(started) as f32 / TICK_HZ as f32;
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
        if course.devices[gates[runner.next]].passed_through(last, here) {
            runner.next += 1;
            runner.splits.push(tick.0);
            if runner.next == gates.len() {
                runner.finished = Some(tick.0);
                let t = time(tick.0);
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
        let last_home = crew.iter().filter_map(|r| r.finished).map(time).fold(0.0, f32::max);
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
            mission.report.push(format!("{}: out of time at gate {} of {}", r.name, r.next + 1, gates.len()));
        }
        mission.result("finished", crew.iter().filter(|r| r.finished.is_some()).count() as f32);
        mission.decide(Phase::Lost, tick.0, "OUT OF TIME");
    }
    // The board: the finished by time, then everyone else by how far
    // they've got.
    let mut board: Vec<&Runner> = run.runners.values().collect();
    board.sort_by(|a, b| match (a.finished, b.finished) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.next.cmp(&a.next).then(a.name.cmp(&b.name)),
    });
    let status = board
        .iter()
        .map(|r| match r.finished {
            Some(at) => format!("{:<10} {}", r.name, clock(time(at))),
            None if r.next == 0 => format!("{:<10} to gate 1", r.name),
            None => format!("{:<10} gate {}/{}  {}", r.name, r.next, gates.len(), clock(time(*r.splits.last().unwrap()))),
        })
        .collect();
    let course_id = marked.id(run.course);
    let markers = run
        .runners
        .values()
        .filter(|r| r.finished.is_none())
        .filter_map(|r| {
            let i = gates[r.next];
            let gate = Marker::device(course_id?, course, &course.devices[i].id, MarkerKind::Waypoint);
            Some(gate.for_player(r.slot))
        })
        .collect();
    let objective = match mission.phase {
        Phase::Underway => {
            format!("Fly the gates in order — {} left", countdown(TIME_LIMIT - mission.elapsed(tick.0)))
        }
        Phase::Won => "Course clear!".into(),
        Phase::Lost => "Out of time".into(),
    };
    Mission::show(&mut mission, objective, status, markers);
}
