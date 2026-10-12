//! The dead ship: a dark derelict lies off our stern. Board it (the suit
//! lamp is your light), restart its reactor, and route the power through its
//! junctions to the vault, reading the power board in engineering (one
//! reads and calls, the others act); turn the vault's two keys, far apart,
//! together; release the data core; and get everyone back aboard before the
//! tripped reactor goes. Solo, key B holds long enough to race to key A.
//!
//! The puzzle is all data, in the derelict's design (`assets/ships/
//! derelict`): its devices and how they're wired. This module only reads
//! their state, to tell the crew what to do next and to decide the end.
//!
//! Wave 4 replaces "release the data core" with the core itself, carried
//! home as cargo: see [`core_secured`].

use std::collections::BTreeMap;

use bevy::prelude::*;

use super::{Scenario, Stage, after_banner};
use crate::sim::bodies::Bodies;
use crate::sim::bots::Robot;
use crate::sim::design::ShipDesign;
use crate::sim::devices::{DeviceKind, Devices};
use crate::sim::mission::{CrewTally, Marker, MarkerKind, Mission, Phase, ScenarioKind, Staged, break_up, clock, presses};
use crate::sim::player::Player;
use crate::sim::ship::{self, ControlAction, HomeShip, ShipHull};
use crate::sim::tuning::Tuning;
use crate::sim::{SimTick, TICK_HZ};

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::DeadShip,
    id: "derelict",
    label: "Dead ship",
    build,
    arrive: None,
    systems: |app| super::judge(app, ScenarioKind::DeadShip, salvage),
    return_after: after_banner,
};

/// The derelict's design.
pub const DERELICT: &str = "derelict";
/// Where it lies: off our stern, turned about so its airlock (its stern)
/// faces our cargo door across a 22 m gap.
pub const DERELICT_AT: Vec3 = Vec3::new(0.0, 0.0, 55.0);
/// How long the crew has to get home once the core is out (s).
pub const OVERLOAD_SECONDS: f32 = 75.0;

fn build(stage: &mut Stage) -> Entity {
    let home = stage.commands.spawn(ship::drifting_home_ship()).id();
    let derelict = stage
        .commands
        .spawn(ship::site("Derelict", DERELICT, DERELICT_AT, Quat::from_rotation_y(std::f32::consts::PI)))
        .id();
    stage.commands.spawn((
        Name::new("Salvage"),
        Staged,
        Salvage {
            derelict,
            released: None,
            reactor: false,
            vault: false,
            turns: BTreeMap::new(),
        },
    ));
    home
}

/// How the salvage is going, beyond what the derelict's devices say.
#[derive(Component)]
struct Salvage {
    derelict: Entity,
    /// When the core was released (the reactor's countdown starts).
    released: Option<u32>,
    /// Whether the reactor's restart and the vault's opening are reported.
    reactor: bool,
    vault: bool,
    /// Junction turns, by who.
    turns: BTreeMap<Entity, u32>,
}

/// Where the salvage stands, from the derelict's devices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Restart,
    Route,
    Keys,
    Release,
    Escape,
}

/// Whether the mission's prize is safe: for now, released from its cradle.
/// (Wave 4: the data core, as cargo, aboard our ship.)
fn core_secured(salvage: &Salvage) -> bool {
    salvage.released.is_some()
}

#[allow(clippy::too_many_arguments)]
fn salvage(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    bodies: Bodies,
    mut commands: Commands,
    salvage: Option<Single<&mut Salvage>>,
    ships: Query<(&ShipHull, &Devices)>,
    homes: Query<Entity, With<HomeShip>>,
    mut players: Query<(Entity, &mut Player, Has<Robot>)>,
    names: Query<&Name>,
    mission: Single<&mut Mission>,
    mut tally: ResMut<CrewTally>,
) {
    let Some(salvage) = salvage else { return };
    let mut salvage = salvage.into_inner();
    let mut mission = mission.into_inner();
    let design: &'static ShipDesign = ShipHull::new(DERELICT).design();
    let derelict = salvage.derelict;
    let name = |e: Entity| names.get(e).map_or("?".to_string(), |n| n.to_string());
    let started = mission.started;
    let since = |at: u32| clock(at.wrapping_sub(started) as f32 / TICK_HZ as f32);
    let home = homes.single().ok();
    let crew: Vec<Entity> = players.iter().filter(|(.., robot)| !robot).map(|(e, ..)| e).collect();
    let aboard = players
        .iter()
        .filter(|(_, p, robot)| !robot && home.is_some() && p.mode.body() == home)
        .count();

    if let Ok((_, devices)) = ships.get(derelict) {
        let live = |id: &str| devices.live(design, id);
        // Who did what, from this tick's presses (the devices have already
        // answered them).
        let pressed: Vec<_> = presses(tick.0, players.iter().map(|(e, p, _)| (e, p))).collect();
        for (who, body, action) in pressed {
            let ControlAction::Device(i) = action else { continue };
            if body != derelict {
                continue;
            }
            match design.devices[i as usize].id.as_str() {
                "reactor" if live("reactor") && !salvage.reactor => {
                    salvage.reactor = true;
                    tally.credit(who, "restarted the reactor");
                    mission.report.push(format!("Reactor restarted by {} at {}.", name(who), since(tick.0)));
                }
                "j1" | "j2" | "j3" => *salvage.turns.entry(who).or_default() += 1,
                "key_a" if live("key_a") => tally.credit(who, "turned key A"),
                "key_b" if live("key_b") => tally.credit(who, "turned key B"),
                "core_clamp" if live("core_clamp") && salvage.released.is_none() => {
                    salvage.released = Some(tick.0);
                    tally.credit(who, "released the data core");
                    mission.report.push(format!("Data core released by {} at {}.", name(who), since(tick.0)));
                }
                _ => {}
            }
        }
        if live("vault") && !salvage.vault {
            salvage.vault = true;
            mission.report.push(format!("Vault opened at {}.", since(tick.0)));
        }
    }

    let step = match ships.get(derelict) {
        Ok((_, d)) if salvage.released.is_some() || d.live(design, "core_clamp") => Step::Escape,
        Ok((_, d)) if d.live(design, "vault") => Step::Release,
        Ok((_, d)) if d.live(design, "vault_bus") => Step::Keys,
        Ok((_, d)) if d.live(design, "reactor") => Step::Route,
        Ok(_) => Step::Restart,
        // Gone: it's over one way or the other.
        Err(_) => Step::Escape,
    };

    // The end: everyone home with the core, or the reactor goes first.
    if mission.phase == Phase::Underway && let Some(released) = salvage.released {
        let left = OVERLOAD_SECONDS - tick.0.wrapping_sub(released) as f32 / TICK_HZ as f32;
        let all_home = !crew.is_empty() && aboard == crew.len();
        let outcome = if all_home && core_secured(&salvage) {
            mission.report.push(format!("Everyone home with {} to spare.", clock(left)));
            Some((Phase::Won, "SALVAGE COMPLETE"))
        } else if left <= 0.0 {
            let left_behind = crew.len() - aboard;
            mission.report.push(format!("The reactor went with {left_behind} of the crew still out."));
            Some((Phase::Lost, "DERELICT LOST"))
        } else {
            None
        };
        if let Some((phase, outcome)) = outcome {
            for (who, turns) in &salvage.turns {
                tally.credit(*who, format!("turned junctions {turns} times"));
            }
            mission.decide(phase, tick.0, outcome);
            // Either way, it goes: the crew watches from home, or is thrown
            // off it.
            if let Some(frame) = bodies.frame(derelict) {
                break_up(&mut commands, derelict, design, &frame, tick.0, &tuning, &mut players);
            }
        }
    }

    // What to do now, and where.
    let device = |id: &str, kind| Marker::device(derelict, design, id, kind);
    let (objective, markers) = match (mission.phase, step) {
        (Phase::Won, _) => ("Salvage complete".to_string(), Vec::new()),
        (Phase::Lost, _) => ("The derelict is gone".to_string(), Vec::new()),
        (_, Step::Restart) => (
            "Board the derelict and restart its reactor (engineering, aft)".to_string(),
            vec![
                Marker {
                    label: "Airlock".into(),
                    kind: MarkerKind::Goal,
                    body: Some(derelict),
                    at: design.marker("entry") + Vec3::Y * 1.2,
                    player: None,
                    device: None,
                },
                device("reactor", MarkerKind::Device),
            ],
        ),
        (_, Step::Route) => (
            "Route power to the vault: read the power board, set the junctions".to_string(),
            ["board", "j1", "j2", "j3"].into_iter().map(|id| device(id, MarkerKind::Device)).collect(),
        ),
        (_, Step::Keys) => {
            let hold = |id: &str| match design.devices[design.device(id)].kind {
                DeviceKind::Key { hold, solo } => (hold, solo),
                _ => (0.0, None),
            };
            let objective = match (crew.len(), hold("key_b")) {
                (1, (_, Some(solo))) => format!("Turn key B, then race to key A: alone, B holds {solo:.0} s"),
                (_, (hold, _)) => format!("Turn keys A and B together (within {hold:.1} s)"),
            };
            (objective, vec![device("key_a", MarkerKind::Device), device("key_b", MarkerKind::Device)])
        }
        (_, Step::Release) => ("Release the data core in the vault".to_string(), vec![device("core_clamp", MarkerKind::Goal)]),
        (_, Step::Escape) => (
            "Reactor overload — everyone back to the ship!".to_string(),
            home.map(|home| Marker {
                label: "Ship".into(),
                kind: MarkerKind::Exit,
                body: Some(home),
                at: Vec3::new(0.0, 1.5, 8.0),
                player: None,
                device: None,
            })
            .into_iter()
            .collect(),
        ),
    };
    let status = match salvage.released {
        Some(released) if mission.phase == Phase::Underway => {
            let left = (OVERLOAD_SECONDS - tick.0.wrapping_sub(released) as f32 / TICK_HZ as f32).max(0.0);
            vec![format!("Overload in {}", clock(left)), format!("Aboard: {aboard} of {}", crew.len())]
        }
        _ => Vec::new(),
    };
    Mission::show(&mut mission, objective, status, markers);
}
