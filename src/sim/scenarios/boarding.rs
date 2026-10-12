//! The boarding action: our ship and an enemy carrier duel among rocks. The
//! carrier's guns are slow and its shield holds against ours; the fast way
//! to win is to cross over, fight through it to its bridge and drop the
//! shield, then get clear before it breaks apart. Lose our ship, and the
//! crew drifts a while, then it's back to the field.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{Scenario, Stage};
use crate::sim::bodies::{Bodies, PreviousVelocity};
use crate::sim::bots::{self, Robot};
use crate::sim::combat::Combatant;
use crate::sim::design::ShipDesign;
use crate::sim::flight::{Duel, Flight, Path};
use crate::sim::mission::{CrewTally, MarkedBodies, Marker, MarkerKind, Mission, Phase, ScenarioKind, break_up, presses};
use crate::sim::player::{Mode, Player};
use crate::sim::ship::{self, ControlAction, HomeShip, ShipHull};
use crate::sim::tuning::Tuning;
use crate::sim::{SimTick, TICK_HZ, asteroids};

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::Boarding,
    id: "boarding",
    label: "Boarding action",
    build,
    arrive: None,
    systems: |app| super::judge(app, ScenarioKind::Boarding, (drop_shields, settle_battle, show).chain()),
    // Won, the crew picks what's next; lost, they drift a while first.
    return_after: |phase, tuning| (phase == Phase::Lost).then_some(tuning.wreck_return_seconds),
    homecoming: true,
};

/// The enemy ship in a boarding action.
pub const CARRIER: &str = "carrier";

/// Rocks around a battle.
const BATTLE_ROCKS: usize = 320;
/// How far (m) a rock's surface keeps from where a ship's origin passes: a
/// ship's half-length and some.
const SHIP_CLEARANCE: f32 = 40.0;
/// The ships' paths are sampled every half second for ten minutes when
/// placing rocks.
const SAMPLE_TICKS: u32 = TICK_HZ as u32 / 2;
const BATTLE_SAMPLES: u32 = 1200;

/// Two ships in a duel: ours, and the carrier weaving in and out around it.
/// Returns ours.
fn build(stage: &mut Stage) -> Entity {
    let (commands, tuning, tick) = (&mut *stage.commands, stage.tuning, stage.tick);
    let duel = Duel {
        center: Vec3::ZERO,
        radius: 300.0,
        lap: 120.0,
        bob: 20.0,
        near: tuning.duel_near,
        far: tuning.duel_far,
        pass: tuning.carrier_pass_seconds,
        sweep: 240.0,
    };
    let (orbit, weave) = (
        Flight {
            start: tick,
            path: Path::Orbit(duel),
            shaken: true,
        },
        Flight {
            start: tick,
            path: Path::Weave(duel),
            shaken: true,
        },
    );
    // Rocks among the fight, for the crew to swing between, clear of
    // everywhere either ship goes for longer than any fight lasts.
    let swept: Vec<Vec3> = (0..BATTLE_SAMPLES)
        .flat_map(|i| {
            let at = tick + i * SAMPLE_TICKS;
            [orbit.pose(at, 0.0).0, weave.pose(at, 0.0).0]
        })
        .collect();
    let near: Vec<Vec3> = swept.iter().step_by(7).copied().collect();
    asteroids::spawn_battlefield(commands, &near, BATTLE_ROCKS, |at, radius| {
        let keep_out = radius + SHIP_CLEARANCE;
        swept.iter().all(|p| p.distance_squared(at) > keep_out * keep_out)
    });
    let flying = |flight: Flight| {
        let frame = flight.frame(tick, Vec3::ZERO);
        (
            flight,
            RigidBody::Static,
            PreviousVelocity::default(),
            Position(frame.position),
            Rotation(frame.rotation),
            LinearVelocity(frame.linear_velocity),
            AngularVelocity(frame.angular_velocity),
        )
    };
    let home = commands
        .spawn((
            Name::new("Ship"),
            HomeShip,
            ShipHull::new(ship::HOME),
            flying(orbit),
        ))
        .id();
    let carrier = commands
        .spawn((
            Name::new("Carrier"),
            ShipHull::new(CARRIER),
            flying(weave),
            Combatant {
                target: Some(home),
                integrity: 1.0,
                shield: true,
                shielded_seconds: tuning.enemy_shielded_seconds,
                exposed_seconds: tuning.enemy_exposed_seconds,
            },
        ))
        .id();
    commands.entity(home).insert(Combatant {
        target: Some(carrier),
        integrity: 1.0,
        shield: true,
        shielded_seconds: tuning.home_survival_seconds,
        exposed_seconds: tuning.home_survival_seconds,
    });
    if stage.robots {
        crew_ship(commands, tuning, carrier, ShipHull::new(CARRIER).design(), &weave, tick);
    }
    crate::sim::pickups::stock_ship(commands, carrier, ShipHull::new(CARRIER).design(), tick);
    home
}

/// Puts robots at the posts of `design` aboard `ship` (flying `flight`):
/// walkers standing on its decks, flyers holding station about it.
fn crew_ship(commands: &mut Commands, tuning: &Tuning, ship: Entity, design: &ShipDesign, flight: &Flight, tick: u32) {
    let frame = flight.frame(tick, Vec3::ZERO);
    for (i, post) in design.posts.iter().enumerate() {
        let mode = match post.robot {
            Robot::Gunner => Mode::Booted {
                body: ship,
                feet: post.at + Vec3::Y * 0.01,
                up: Vec3::Y,
                velocity: Vec3::ZERO,
                orientation: post.rotation(),
                pitch: 0.0,
            },
            Robot::Swarmer => {
                let position = frame.point_to_world(post.at);
                Mode::Eva {
                    position,
                    velocity: frame.point_velocity(position),
                    orientation: frame.rotation * post.rotation(),
                    pitch: 0.0,
                }
            }
        };
        let home_post = match mode {
            Mode::Booted { feet, .. } => feet,
            Mode::Eva { .. } => post.at,
        };
        commands.spawn(bots::new_robot(post.robot, mode, ship, home_post, tuning, i as u32));
    }
}

/// Pressing the shield console drops that ship's shield.
fn drop_shields(
    tick: Res<SimTick>,
    players: Query<(Entity, &Player, Option<&Name>)>,
    mut ships: Query<&mut Combatant>,
    mut mission: Single<&mut Mission>,
    mut tally: ResMut<CrewTally>,
) {
    let pressed: Vec<_> = presses(tick.0, players.iter().map(|(e, p, _)| (e, p))).collect();
    for (who, body, action) in pressed {
        if action != ControlAction::DisableShields {
            continue;
        }
        if let Ok(mut ship) = ships.get_mut(body)
            && ship.shield
        {
            ship.shield = false;
            let name = players.get(who).ok().and_then(|(_, _, n)| n).map_or("?".to_string(), |n| n.to_string());
            mission.report.push(format!("Shield dropped by {name}. Remember that name."));
            mission.result("shield_dropped", 1.0);
            tally.credit(who, "dropped the carrier's shield");
        }
    }
}

/// Wrecks any ship worn down to nothing, and decides the mission.
fn settle_battle(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    bodies: Bodies,
    mut commands: Commands,
    mut mission: Single<&mut Mission>,
    mut ships: Query<(Entity, &mut Combatant, &ShipHull, Has<HomeShip>, Option<&mut Flight>)>,
    mut players: Query<(Entity, &mut Player, Has<Robot>)>,
) {
    let wrecked: Vec<(Entity, &'static ShipDesign, bool)> = ships
        .iter()
        .filter(|(_, c, ..)| c.integrity <= 0.0)
        .map(|(e, _, hull, home, _)| (e, hull.design(), home))
        .collect();
    for (ship, design, home) in wrecked {
        let Some(frame) = bodies.frame(ship) else {
            continue;
        };
        break_up(&mut commands, ship, design, &frame, tick.0, &tuning, &mut players);
        // Nobody shoots at wreckage, and it can't shake anyone.
        for (_, mut combatant, _, _, flight) in &mut ships {
            if combatant.target == Some(ship) {
                combatant.target = None;
                if let Some(mut flight) = flight {
                    flight.shaken = false;
                }
            }
        }
        if mission.phase == Phase::Underway {
            let shield_dropped = mission.report.iter().any(|l| l.starts_with("Shield dropped"));
            if home {
                mission.decide(Phase::Lost, tick.0, "SHIP LOST");
                if !shield_dropped {
                    mission.report.push("Nobody ever dropped the shield. Bold strategy.".into());
                }
            } else {
                mission.decide(Phase::Won, tick.0, "CARRIER DESTROYED");
                if !shield_dropped {
                    mission.report.push("Nobody dropped the shield; it fell on its own. Suspicious.".into());
                }
            }
        }
    }
}

/// The objective and markers for the HUD.
fn show(
    mission: Single<&mut Mission>,
    ships: Query<(Entity, &Combatant, &ShipHull, Has<HomeShip>)>,
    marked: MarkedBodies,
) {
    let mut mission = mission.into_inner();
    let enemy = ships.iter().find(|(.., home)| !home);
    let home = ships.iter().find(|(.., home)| *home).map(|(e, ..)| e);
    let shield_up = enemy.is_some_and(|(_, c, ..)| c.shield);
    let objective = match mission.phase {
        Phase::Won => "Carrier destroyed — return to the mission wall",
        Phase::Lost => "Hull breached — abandon ship",
        Phase::Underway if shield_up => "Board the carrier and disable its shield on the bridge",
        Phase::Underway => "Shield down — get clear!",
    };
    let mut markers = Vec::new();
    match (enemy, home) {
        (Some((carrier, _, hull, _)), _) if shield_up => markers.extend(
            marked
                .id(carrier)
                .map(|c| Marker::on(c, hull.design().marker("objective"), "Shield console", MarkerKind::Goal)),
        ),
        (_, Some(home)) if mission.phase != Phase::Lost => {
            markers.extend(marked.id(home).map(|h| Marker::on(h, Vec3::new(0.0, 1.5, 0.0), "Ship", MarkerKind::Exit)))
        }
        _ => {}
    }
    Mission::show(&mut mission, objective.into(), Vec::new(), markers);
}
