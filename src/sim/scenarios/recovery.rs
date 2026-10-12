//! Recovery: a ship has broken up nearby, and its crew is adrift in the
//! wreckage, scattered in every direction, each on a clock of oxygen. Find
//! them by their beacons (each pings every few seconds), rope them, and tow
//! them home into our hold. Four suited survivors are light enough for one
//! of us to tow; the lifeboat, with four aboard, is heavy (towing it alone
//! is slow to start and slower to stop; two ropes on it are twice the
//! pull). The clocks are staggered so a lone crew member can't reach
//! everyone: splitting up saves more.
//!
//! Won when everyone is accounted for (home or lost) with at least
//! [`NEEDED`] survivors home; lost as soon as that's out of reach.
//!
//! Results: `survivors_home` and `survivors_lost` (of 8).

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::{Scenario, Stage, after_banner};
use crate::sim::bodies::{Bodies, PreviousVelocity, dynamic_mass};
use crate::sim::bots::Robot;
use crate::sim::mission::{CrewTally, Debris, MarkedBodies, Marker, MarkerKind, Mission, Phase, ScenarioKind, Staged, countdown};
use crate::sim::player::Player;
use crate::sim::ship::{self, HomeShip, in_hold};
use crate::sim::{SimMaterial, SimTick, TICK_HZ, asteroids};

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::Recovery,
    id: "recovery",
    label: "Recovery",
    build,
    arrive: None,
    systems: |app| super::judge(app, ScenarioKind::Recovery, recover),
    return_after: after_banner,
    homecoming: true,
};

/// What's adrift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PodKind {
    /// One survivor in a suit.
    Survivor,
    /// A lifeboat with [`LIFEBOAT_CREW`] aboard.
    Lifeboat,
}

/// How many are in a lifeboat.
pub const LIFEBOAT_CREW: u32 = 4;

impl PodKind {
    /// Full size (m) and mass (kg).
    pub fn size(self) -> Vec3 {
        match self {
            PodKind::Survivor => Vec3::new(0.7, 1.9, 0.7),
            PodKind::Lifeboat => Vec3::new(1.6, 1.6, 2.6),
        }
    }

    pub fn mass(self) -> f32 {
        match self {
            PodKind::Survivor => 90.0,
            PodKind::Lifeboat => 600.0,
        }
    }

    pub fn souls(self) -> u32 {
        match self {
            PodKind::Survivor => 1,
            PodKind::Lifeboat => LIFEBOAT_CREW,
        }
    }
}

/// Whether a pod's crew is still out there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PodState {
    #[default]
    Adrift,
    /// Out of oxygen.
    Lost,
}

/// Something adrift with people in it: a light dynamic body, built from
/// this wherever it appears (see [`shape_pod`]). Replicated, so every peer
/// can draw its beacon.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pod {
    pub kind: PodKind,
    /// Its letter on the board (0 is A).
    pub index: u8,
    /// The tick its air runs out.
    pub oxygen_until: u32,
    pub state: PodState,
}

impl Pod {
    pub fn name(&self) -> String {
        let letter = (b'A' + self.index) as char;
        match self.kind {
            PodKind::Survivor => format!("Survivor {letter}"),
            PodKind::Lifeboat => format!("Lifeboat {letter}"),
        }
    }

    /// Whether its beacon is pinging at `tick` (each pod in its turn).
    pub fn pinging(&self, tick: u32) -> bool {
        let period = (PING_PERIOD * TICK_HZ as f32) as u32;
        let phase = (tick + self.index as u32 * period / 5) % period;
        self.state == PodState::Adrift && (phase as f32) < PING_SECONDS * TICK_HZ as f32
    }
}

/// A beacon pings for this long (s), once every period.
pub const PING_SECONDS: f32 = 1.2;
pub const PING_PERIOD: f32 = 4.0;

/// Gives a new pod its body.
pub fn shape_pod(add: On<Add, Pod>, pods: Query<&Pod>, mut commands: Commands) {
    let pod = pods.get(add.entity).expect("just added");
    let size = pod.kind.size();
    let collider = Collider::cuboid(size.x, size.y, size.z);
    let density = pod.kind.mass() / (size.x * size.y * size.z);
    commands
        .entity(add.entity)
        .insert((dynamic_mass(&collider, density), collider, SimMaterial::Trim));
}

/// How many survivors must be home to win.
pub const NEEDED: u32 = 4;

/// The wreck's crew, where they're found (in our ship's frame, before a
/// jitter), and how long their air lasts (s): the near ones go first.
const ADRIFT: [(PodKind, Vec3, f32); 5] = [
    (PodKind::Survivor, Vec3::new(110.0, 15.0, -50.0), 140.0),
    (PodKind::Survivor, Vec3::new(-90.0, -25.0, 100.0), 160.0),
    (PodKind::Survivor, Vec3::new(40.0, 60.0, 150.0), 190.0),
    (PodKind::Survivor, Vec3::new(-140.0, 10.0, -40.0), 220.0),
    (PodKind::Lifeboat, Vec3::new(10.0, -45.0, -160.0), 300.0),
];

fn build(stage: &mut Stage) -> Entity {
    let commands = &mut *stage.commands;
    let tick = stage.tick;
    let home = commands.spawn(ship::drifting_home_ship()).id();
    let mut rng = ChaCha8Rng::seed_from_u64(0x7e5c_0e00 ^ tick as u64);
    let mut rocks = Vec::new();
    for (index, &(kind, at, air)) in ADRIFT.iter().enumerate() {
        let jitter = Vec3::new(rng.random_range(-12.0..12.0), rng.random_range(-12.0..12.0), rng.random_range(-12.0..12.0));
        let at = at + jitter;
        // Drifting slowly outward, turning over.
        let drift = at.normalize() * rng.random_range(0.1..0.4);
        let spin = Vec3::new(rng.random_range(-0.3..0.3), rng.random_range(-0.3..0.3), rng.random_range(-0.3..0.3));
        let pod = Pod {
            kind,
            index: index as u8,
            oxygen_until: tick + (air * TICK_HZ as f32) as u32,
            state: PodState::Adrift,
        };
        commands.spawn((
            // As the board and the beacons name it: what the HUD's rope and
            // reference readouts say.
            Name::new(pod.name()),
            Staged,
            pod,
            RigidBody::Dynamic,
            SleepingDisabled,
            PreviousVelocity::default(),
            LinearVelocity(drift),
            AngularVelocity(spin),
            Position(at),
            Rotation(Quat::from_scaled_axis(spin * 3.0)),
        ));
        // The wreckage round them: hull plates to grapple, and a few rocks
        // the wreck broke up among.
        for _ in 0..8 {
            let offset = Vec3::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0)) * 22.0;
            if offset.length() < 5.0 {
                continue;
            }
            let size = Vec3::new(rng.random_range(1.5..6.0), rng.random_range(0.2..0.5), rng.random_range(1.5..8.0));
            commands.spawn((
                Name::new("Debris"),
                Debris { size, hostile: false },
                RigidBody::Dynamic,
                SleepingDisabled,
                PreviousVelocity::default(),
                LinearVelocity(drift + offset.normalize() * rng.random_range(0.0..0.3)),
                AngularVelocity(Vec3::new(rng.random_range(-0.2..0.2), rng.random_range(-0.2..0.2), rng.random_range(-0.2..0.2))),
                Position(at + offset),
                Rotation(Quat::from_scaled_axis(Vec3::new(rng.random(), rng.random(), rng.random()) * 3.0)),
            ));
        }
        // Rocks along the way out, for grapple swings.
        for k in 1..4 {
            let along = at * (k as f32 / 4.0);
            let side = Vec3::new(rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0), rng.random_range(-1.0..1.0)).normalize_or(Vec3::Y);
            let radius = rng.random_range(1.5..5.0);
            let place = along + side * rng.random_range(radius * 2.0 + 10.0..30.0);
            if place.length() > 40.0 {
                rocks.push((place, radius));
            }
        }
    }
    asteroids::spawn_rocks(commands, tick as u64 ^ 0x7e5c, &rocks);
    commands.spawn((
        Name::new("Recovery"),
        Staged,
        Rescue {
            home: BTreeMap::new(),
            towed: BTreeMap::new(),
        },
    ));
    home
}

/// Who's been brought home, and who towed whom.
#[derive(Component)]
struct Rescue {
    /// Pods home, by index: their kind.
    home: BTreeMap<u8, PodKind>,
    /// Everyone who has had a rope on each pod, by index.
    towed: BTreeMap<u8, BTreeSet<Entity>>,
}

#[allow(clippy::too_many_arguments)]
fn recover(
    tick: Res<SimTick>,
    bodies: Bodies,
    mut commands: Commands,
    rescue: Option<Single<&mut Rescue>>,
    mut pods: Query<(Entity, &mut Pod, &Position)>,
    homes: Query<Entity, With<HomeShip>>,
    players: Query<(Entity, &Player, Option<&Name>), Without<Robot>>,
    marked: MarkedBodies,
    mission: Single<&mut Mission>,
    mut tally: ResMut<CrewTally>,
) {
    let Some(rescue) = rescue else { return };
    let mut rescue = rescue.into_inner();
    let mut mission = mission.into_inner();
    let Some(home) = homes.iter().next() else { return };
    let Some(ship) = bodies.frame(home) else { return };
    let name = |e: Entity| players.get(e).ok().and_then(|(_, _, n)| n).map_or("?".to_string(), |n| n.to_string());
    let started = mission.started;
    let since = |t: u32| crate::sim::mission::clock(t.wrapping_sub(started) as f32 / TICK_HZ as f32);
    let underway = mission.phase == Phase::Underway;
    for (entity, mut pod, at) in &mut pods {
        // Whoever has a rope on it is towing it.
        for (who, player, _) in &players {
            if player.grapple.is_some_and(|g| g.target == entity) {
                rescue.towed.entry(pod.index).or_default().insert(who);
            }
        }
        if !underway || pod.state != PodState::Adrift {
            continue;
        }
        if in_hold(ship.point_to_local(at.0)) {
            rescue.home.insert(pod.index, pod.kind);
            let crew: Vec<Entity> = rescue.towed.get(&pod.index).map(|s| s.iter().copied().collect()).unwrap_or_default();
            let by: Vec<String> = crew.iter().map(|&e| name(e)).collect();
            let who = if by.is_empty() { "nobody's rope".to_string() } else { by.join(" and ") };
            mission.report.push(format!("{} home at {}, by {who}.", pod.name(), since(tick.0)));
            for e in crew {
                tally.credit(e, format!("brought {} home", pod.name()));
            }
            // They climb out into the hold.
            commands.entity(entity).despawn();
        } else if tick.0 >= pod.oxygen_until {
            pod.state = PodState::Lost;
            mission.report.push(format!("{} ran out of air at {}.", pod.name(), since(tick.0)));
        }
    }
    let saved: u32 = rescue.home.values().map(|k| k.souls()).sum();
    // Still out there (not counting those home this tick, whose despawn
    // is still to come).
    let still_out = |p: &Pod| p.state == PodState::Adrift && !rescue.home.contains_key(&p.index);
    let out: u32 = pods.iter().filter(|(_, p, _)| still_out(p)).map(|(_, p, _)| p.kind.souls()).sum();
    let total: u32 = ADRIFT.iter().map(|(k, ..)| k.souls()).sum();
    if underway {
        mission.result("survivors_home", saved as f32);
        mission.result("survivors_lost", (total - saved - out) as f32);
        if saved + out < NEEDED {
            mission.report.push(format!("{saved} of {total} home: too few to call it a rescue."));
            mission.decide(Phase::Lost, tick.0, "RESCUE FAILED");
        } else if out == 0 {
            mission.report.push(format!("{saved} of {total} survivors home."));
            mission.decide(Phase::Won, tick.0, if saved == total { "EVERYONE HOME" } else { "SURVIVORS HOME" });
        }
    }
    // The board, and the beacons that are pinging.
    let mut adrift: Vec<(Entity, Pod)> = pods.iter().filter(|(_, p, _)| still_out(p)).map(|(e, p, _)| (e, *p)).collect();
    adrift.sort_by_key(|(_, p)| p.index);
    let status: Vec<String> = std::iter::once(format!("Home: {saved} of {total} (need {NEEDED})"))
        .chain(adrift.iter().map(|(_, p)| {
            let left = p.oxygen_until.saturating_sub(tick.0) as f32 / TICK_HZ as f32;
            format!("{:<11} air {}", p.name(), countdown(left))
        }))
        .collect();
    let mut markers: Vec<Marker> = adrift
        .iter()
        .filter(|(_, p)| p.pinging(tick.0))
        .filter_map(|(e, p)| Some(Marker::on(marked.id(*e)?, Vec3::ZERO, &p.name(), MarkerKind::Goal)))
        .collect();
    // Someone with a rope on a pod is bringing it home: the hold.
    let towing = players.iter().any(|(_, p, _)| p.grapple.is_some_and(|g| pods.contains(g.target)));
    if towing && let Some(h) = marked.id(home) {
        markers.push(Marker::on(h, Vec3::new(0.0, 1.5, 9.0), "Hold", MarkerKind::Exit));
    }
    let objective = match mission.phase {
        Phase::Underway => "Find the survivors by their beacons and tow them into the hold".to_string(),
        Phase::Won => "Rescue complete".to_string(),
        Phase::Lost => "Rescue failed".to_string(),
    };
    Mission::show(&mut mission, objective, status, markers);
}
