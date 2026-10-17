//! Escort: a disabled freighter lies off our stern with a power cell in
//! its cargo bay, and robots aboard. Bring the cell home to our hold. It's
//! bulky: whoever carries it has both hands full (no guns, no grapple, no
//! switches), so the crew escorts them. The route is the puzzle
//! (`assets/ships/freighter`): the breach, where the deck is gone, until
//! someone on the far side runs the gangway out; the bay door, which a key
//! holds open for 3 s (or anyone standing in the doorway does); and the
//! 20 m gap to our ship, crossed on jetpacks. Alone: carry it, set it down
//! (drop, G) to fight or to work a switch, pick it up again.
//!
//! Won when the cell is in our hold, carried or set down there; lost if it
//! drifts more than [`ADRIFT_LOST`] from our ship. Whoever dies or leaves
//! holding it leaves it where they were, for anyone. Results: `cell_home`
//! (1 when won), `carriers` (how many of the crew carried it).

use std::collections::BTreeSet;

use bevy::prelude::*;

use super::{Scenario, Stage, after_banner};
use crate::sim::bodies::Bodies;
use crate::sim::bots::{self, Robot, Threat};
use crate::sim::design::ShipDesign;
use crate::sim::devices::Devices;
use crate::sim::frame::BodyFrame;
use crate::sim::items::{CargoKind, Item};
use crate::sim::mission::{CrewTally, MarkedBodies, Marker, MarkerKind, Mission, Phase, ScenarioKind, Staged};
use crate::sim::pickups::{self, PICKUP_HALF, Pickup};
use crate::sim::player::Player;
use crate::sim::ship::{self, HomeShip, ShipHull, in_hold};
use crate::sim::tuning::Tuning;
use crate::sim::{SimTick, TICK_HZ};

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::Escort,
    id: "escort",
    label: "Escort",
    build,
    arrive: None,
    systems: |app| super::judge(app, ScenarioKind::Escort, escort),
    return_after: after_banner,
    homecoming: true,
    ambience: crate::sim::ambience::Choice::named("escort"),
};

/// The freighter's design.
pub const FREIGHTER: &str = "freighter";
/// Where it lies: off our stern, turned about so its airlock faces our
/// cargo door across 33 m. No nearer: its grappler, standing in its
/// airlock, would reach into our hold and as far as where we come back
/// aboard after dying (the deck, 45 m off), yanking a lone player across to
/// the gunners again and again.
pub const FREIGHTER_AT: Vec3 = Vec3::new(0.0, 0.0, 66.0);
/// How dangerous its crew is (see [`Threat`]).
const THREAT: Threat = Threat(2);

/// The prize.
pub const CELL: Item = Item::Cargo(CargoKind::PowerCell);

/// How far from our ship (m) the cell may drift before it's lost.
pub const ADRIFT_LOST: f32 = 400.0;

fn build(stage: &mut Stage) -> Entity {
    let commands = &mut *stage.commands;
    let home = commands.spawn(ship::parked_home_ship()).id();
    let turn = Quat::from_rotation_y(std::f32::consts::PI);
    let freighter = commands.spawn(ship::site("Freighter", FREIGHTER, FREIGHTER_AT, turn)).id();
    let design: &ShipDesign = ShipHull::new(FREIGHTER).design();
    if stage.robots {
        let frame = BodyFrame {
            position: FREIGHTER_AT,
            rotation: turn,
            center_of_mass: FREIGHTER_AT,
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
        };
        bots::crew_ship(commands, stage.tuning, freighter, design, &frame, stage.tick, THREAT);
    }
    pickups::stock_ship(commands, freighter, design, stage.tick);
    commands.spawn((
        Name::new("Power cell"),
        Pickup::dropped((Some(freighter), design.marker("objective") + Vec3::Y * PICKUP_HALF.y, Vec3::ZERO), stage.tick, vec![CELL]),
    ));
    commands.spawn((
        Name::new("Escort"),
        Staged,
        Haul {
            freighter,
            carriers: BTreeSet::new(),
        },
    ));
    home
}

/// Who's carried the cell.
#[derive(Component)]
struct Haul {
    freighter: Entity,
    carriers: BTreeSet<Entity>,
}

/// Where the cell is.
enum Cell {
    Carried(Entity),
    /// Lying on a body (`at` in its coordinates) or adrift (in the world).
    Lying { body: Option<Entity>, at: Vec3, world: Vec3 },
    Gone,
}

#[allow(clippy::too_many_arguments)]
fn escort(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    bodies: Bodies,
    haul: Option<Single<&mut Haul>>,
    homes: Query<Entity, With<HomeShip>>,
    players: Query<(Entity, &Player, Option<&Name>), Without<Robot>>,
    robots: Query<&Player, With<Robot>>,
    cells: Query<&Pickup>,
    ships: Query<(&ShipHull, &Devices)>,
    marked: MarkedBodies,
    mission: Single<&mut Mission>,
    mut tally: ResMut<CrewTally>,
) {
    let Some(haul) = haul else { return };
    let mut haul = haul.into_inner();
    let mut mission = mission.into_inner();
    let Some(home) = homes.iter().next() else { return };
    let Some(ship) = bodies.frame(home) else { return };
    let name = |e: Entity| players.get(e).ok().and_then(|(_, _, n)| n).map_or("?".to_string(), |n| n.to_string());
    let carrier = players.iter().find(|(_, p, _)| p.loadout.inventory.carrying == Some(CargoKind::PowerCell));
    let cell = match carrier {
        Some((who, ..)) => Cell::Carried(who),
        None => cells
            .iter()
            .find(|p| p.items.contains(&CELL))
            .and_then(|p| Some(Cell::Lying { body: p.body, at: p.at, world: p.sim_pose(&bodies, tick.0)?.0 }))
            .unwrap_or(Cell::Gone),
    };
    if let Cell::Carried(who) = cell
        && haul.carriers.insert(who)
    {
        tally.credit(who, "carried the power cell");
    }
    // Home: in our hold, in someone's arms or set down there.
    let at_home = match cell {
        Cell::Carried(who) => players.get(who).is_ok_and(|(_, p, _)| {
            let own = p.mode.body().and_then(|b| bodies.frame(b));
            in_hold(ship.point_to_local(p.mode.world_kinematics(&tuning, own.as_ref()).0))
        }),
        Cell::Lying { world, .. } => in_hold(ship.point_to_local(world)),
        Cell::Gone => false,
    };
    // Lost: the cell gone from the world, or adrift past reach.
    let lost = match cell {
        Cell::Gone => true,
        Cell::Lying { body: None, world, .. } => world.distance(ship.position) > ADRIFT_LOST,
        _ => false,
    };
    if mission.phase == Phase::Underway && lost {
        mission.report.push("The power cell drifted out of reach.".into());
        mission.result("cell_home", 0.0);
        mission.decide(Phase::Lost, tick.0, "CELL LOST");
    }
    if mission.phase == Phase::Underway && at_home {
        let by: Vec<String> = haul.carriers.iter().map(|&e| name(e)).collect();
        let started = mission.started;
        mission.report.push(format!(
            "Power cell home at {}, carried by {}.",
            crate::sim::mission::clock(tick.0.wrapping_sub(started) as f32 / TICK_HZ as f32),
            if by.is_empty() { "nobody (it found its own way)".to_string() } else { by.join(", ") }
        ));
        let robots_left = robots.iter().filter(|r| r.mode.body() == Some(haul.freighter)).count();
        mission.report.push(format!("{robots_left} robots left aboard the freighter."));
        mission.result("cell_home", 1.0);
        mission.result("carriers", haul.carriers.len() as f32);
        mission.decide(Phase::Won, tick.0, "POWER RESTORED");
    }

    // What to do now, and where.
    let freighter_id = marked.id(haul.freighter);
    let design = ShipHull::new(FREIGHTER).design();
    let devices = ships.get(haul.freighter).ok().map(|(_, d)| d);
    let live = |id: &str| devices.is_some_and(|d| d.live(design, id));
    let device = |id: &str| freighter_id.map(|b| Marker::device(b, design, id, MarkerKind::Device));
    let hold = marked.id(home).map(|h| Marker::on(h, Vec3::new(0.0, 1.5, 9.0), "Hold", MarkerKind::Exit));
    let (objective, markers, status): (String, Vec<Option<Marker>>, String) = match (&cell, mission.phase) {
        (_, Phase::Won) => ("Power restored".into(), Vec::new(), "Power cell: home".into()),
        (_, Phase::Lost) => ("Escort failed".into(), Vec::new(), String::new()),
        (Cell::Carried(who), _) => (
            "Carry the power cell to our hold: hands full, so no guns, grapple or switches".into(),
            vec![hold, (!live("gangway")).then(|| device("gangway_lever")).flatten()],
            format!("Power cell: carried by {}", name(*who)),
        ),
        (Cell::Lying { body, at, world }, _) => {
            let marker = match body {
                Some(b) => marked.id(*b).map(|b| Marker::on(b, *at, "Power cell", MarkerKind::Goal)),
                None => Some(Marker::at(*world, "Power cell", MarkerKind::Goal)),
            };
            let in_bay = *body == Some(haul.freighter) && at.z < -14.0;
            let mut ms = vec![marker];
            if !live("gangway") {
                ms.push(device("gangway_lever"));
            }
            if in_bay && !live("bay_door") {
                ms.push(device("bay_key_hall"));
            }
            let objective = if haul.carriers.is_empty() {
                "Board the freighter and bring the power cell from its bay to our hold"
            } else {
                "The power cell is down: pick it up and get it home"
            };
            let place = match (body.is_some(), haul.carriers.is_empty() && in_bay) {
                (_, true) => "in the freighter's bay",
                (true, false) => "set down",
                (false, false) => "adrift",
            };
            (objective.into(), ms, format!("Power cell: {place}"))
        }
        (Cell::Gone, _) => ("The power cell is lost".into(), Vec::new(), String::new()),
    };
    let robots_left = robots.iter().filter(|r| r.mode.body() == Some(haul.freighter)).count();
    let mut lines = vec![status];
    if robots_left > 0 && mission.phase == Phase::Underway {
        lines.push(format!("Robots aboard: {robots_left}"));
    }
    if !live("gangway") && mission.phase == Phase::Underway {
        lines.push("Gangway: in (its lever is on the far side)".into());
    }
    let markers = markers.into_iter().flatten().collect();
    Mission::show(&mut mission, objective, lines.into_iter().filter(|l| !l.is_empty()).collect(), markers);
}
