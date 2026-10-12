//! The boarding action: our ship and an enemy carrier duel among rocks. The
//! carrier's guns are slow and its shield holds against ours; the fast way
//! to win is to cross over, fight through it to its bridge and drop the
//! shield, then get clear before it breaks apart. Lose our ship, and the
//! crew drifts a while, then it's back to the field.
//!
//! Meanwhile the carrier's hits knock our ship's power about (damage
//! control): a beam landing on us can trip the breaker or knock the main
//! bus junction nearest where it lands (`fault` devices in our ship's
//! design), so our turrets or our shield drop until someone goes aft and
//! puts it right, reading the power board on the bridge. Without our
//! shield the hull goes in `home_exposed_seconds`; without our turrets the
//! carrier never goes down, even with its shield off. The chance a hit
//! does damage is three times `hit_fault_chance` for each crew member
//! aboard our ship: the more of you at home, the more there is to do there,
//! and a ship left empty (a lone boarder away) isn't knocked about. Nor is
//! anything in the first `FAULT_GRACE_SECONDS`.
//!
//! The carrier's crew is sized to ours ([`carrier_crew`]): alone, a light
//! one; four players, the full complement.
//!
//! Results: `shield_dropped`, `faults` (hits that knocked something out)
//! and `repairs` (those put right).

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::{Scenario, Stage};
use crate::sim::bodies::{Bodies, PreviousVelocity};
use crate::sim::bots::{self, Gait, Robot};
use crate::sim::combat::{self, Beams, Combatant};
use crate::sim::devices::Devices;
use crate::sim::design::ShipDesign;
use crate::sim::flight::{Duel, Flight, Path};
use crate::sim::mission::{Staged, CrewTally, MarkedBodies, Marker, MarkerKind, Mission, Phase, ScenarioKind, break_up, presses};
use crate::sim::events::AuthorityLog;
use crate::sim::player::Player;
use crate::sim::ship::{self, ControlAction, HomeShip, ShipHull};
use crate::sim::tuning::Tuning;
use crate::sim::{SimTick, TICK_HZ, asteroids};

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::Boarding,
    id: "boarding",
    label: "Boarding action",
    build,
    arrive: None,
    systems: |app| super::judge(app, ScenarioKind::Boarding, (drop_shields, settle_battle, damage_control, show).chain()),
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
        exposed_seconds: tuning.home_exposed_seconds,
    });
    commands.spawn((Name::new("Damage control"), Staged, Damage::default()));
    if stage.robots {
        let design = ShipHull::new(CARRIER).design();
        let crew = carrier_crew(design, stage.players, tick);
        bots::man_posts(commands, tuning, carrier, design, &weave.frame(tick, Vec3::ZERO), crew);
    }
    crate::sim::pickups::stock_ship(commands, carrier, ShipHull::new(CARRIER).design(), tick);
    home
}

/// The carrier's crew for `players` boarders: how dangerous (see
/// [`bots::Threat`]) and how many of its posts are manned grow with the
/// crew, to the full complement at four. Alone, a boarder crossing on a
/// rope at the first pass met six swarmers and three hull turrets and was
/// dead in four seconds, 40 m short of the hangar.
pub fn carrier_crew(design: &ShipDesign, players: usize, tick: u32) -> Vec<bots::Posting> {
    let n = players.clamp(1, 4) - 1;
    let threat = bots::Threat([1, 2, 3, 3][n]);
    // Manned posts of each kind, by crew size (1 to 4 players).
    let manned = |gait: Gait| match gait {
        Gait::Walker => [4, 5, 6, 7][n],
        Gait::Flyer => [1, 3, 4, 6][n],
        Gait::Mount => [1, 2, 3, 3][n],
        _ => usize::MAX,
    };
    let entry = design.markers.get("entry").copied().unwrap_or(Vec3::ZERO);
    let crew = bots::compose_crew(design, threat, tick as u64);
    let mut kept = Vec::new();
    for gait in [Gait::Walker, Gait::Flyer, Gait::Mount, Gait::Crawler, Gait::Clinger] {
        let mut group: Vec<bots::Posting> = crew.iter().copied().filter(|p| p.robot.gait() == gait).collect();
        // From the way in to the deepest: keep posts spread along it, the
        // deepest always (the leader guards the bridge).
        group.sort_by(|a, b| a.post.at.distance(entry).total_cmp(&b.post.at.distance(entry)));
        let (have, want) = (group.len(), manned(gait).min(group.len()));
        if want == 0 {
            continue;
        }
        let picks: Vec<usize> = if want == 1 {
            vec![have - 1]
        } else {
            (0..want).map(|i| (i * (have - 1) + (want - 1) / 2) / (want - 1)).collect()
        };
        kept.extend(picks.into_iter().map(|i| group[i]));
    }
    kept
}

/// No hit knocks anything out of our ship in the first this many seconds:
/// a crew is still at its stations, or a lone boarder still crossing.
const FAULT_GRACE_SECONDS: f32 = 45.0;

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
    mut told: Query<&mut AuthorityLog>,
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
        break_up(&mut commands, ship, design, &frame, tick.0, &tuning, &mut players, &mut told);
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

/// What the carrier's hits have knocked out aboard our ship.
#[derive(Component, Default)]
struct Damage {
    /// Devices knocked out of their setting, by index, and when.
    knocked: Vec<(u16, u32)>,
    faults: u32,
    repairs: u32,
}

/// Our ship's power, under fire: new hits from the carrier may knock out
/// the nearest `fault` device; whatever's put back is a repair; our shield
/// holds only while its emitter has power.
#[allow(clippy::too_many_arguments)]
fn damage_control(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    beams: Res<Beams>,
    bodies: Bodies,
    damage: Option<Single<&mut Damage>>,
    mut homes: Query<(Entity, &ShipHull, &mut Devices, &mut Combatant), With<HomeShip>>,
    players: Query<(Entity, &Player, Option<&Name>), Without<Robot>>,
    mission: Single<&mut Mission>,
    mut tally: ResMut<CrewTally>,
) {
    let (Some(damage), Ok((home, hull, mut devices, mut ours))) = (damage, homes.single_mut()) else {
        return;
    };
    let mut damage = damage.into_inner();
    let mut mission = mission.into_inner();
    let design = hull.design();
    let started = mission.started;
    let since = |t: u32| crate::sim::mission::clock(t.wrapping_sub(started) as f32 / TICK_HZ as f32);
    // A hit just begun: a beam on us, from a turret whose beam starts this
    // tick (turrets take turns, so there's often one beam on us throughout).
    let starting = (0..combat::TURRETS).any(|i| combat::beam_start(i, tick.0) == Some(tick.0));
    let new_hit = beams.0.iter().find(|b| b.target == home && starting).map(|b| b.to);
    if let (Some(at), Some(frame), Phase::Underway) = (new_hit, bodies.frame(home), mission.phase) {
        let mut rng = ChaCha8Rng::seed_from_u64(tick.0 as u64 ^ 0xda4a_6e);
        // More of the crew at home, more goes wrong there (the other
        // front): the base chance, plus twice it for each of them aboard.
        // Nobody at home, nothing goes wrong there that matters: a lone
        // boarder's ship holds while they're away (it's only time).
        let aboard = players.iter().filter(|(_, p, _)| p.mode.body() == Some(home)).count() as f32;
        let chance = (tuning.hit_fault_chance * 3.0 * aboard).min(0.9);
        let grace = tick.0.wrapping_sub(started) < (FAULT_GRACE_SECONDS * TICK_HZ as f32) as u32;
        if !grace && rng.random::<f32>() < chance {
            let local = frame.point_to_local(at);
            let nearest = design
                .devices
                .iter()
                .enumerate()
                .filter(|(i, d)| d.fault && !d.displaced(&devices.0[*i]))
                .min_by(|a, b| a.1.at.distance(local).total_cmp(&b.1.at.distance(local)));
            if let Some((i, device)) = nearest
                && device.knock(&mut devices.0[i], rng.random())
            {
                damage.knocked.push((i as u16, tick.0));
                damage.faults += 1;
                mission.report.push(format!("A hit at {} knocked out the {}.", since(tick.0), device.label.to_lowercase()));
                mission.result("faults", damage.faults as f32);
            }
        }
    }
    // Put right: back in its starting setting.
    let pressed: Vec<_> = presses(tick.0, players.iter().map(|(e, p, _)| (e, p))).collect();
    let knocked = std::mem::take(&mut damage.knocked);
    for (i, at) in knocked {
        let device = &design.devices[i as usize];
        if device.displaced(&devices.0[i as usize]) {
            damage.knocked.push((i, at));
            continue;
        }
        damage.repairs += 1;
        mission.result("repairs", damage.repairs as f32);
        let fixers: Vec<Entity> = pressed
            .iter()
            .filter(|(_, body, action)| *body == home && *action == ControlAction::Device(i))
            .map(|(who, ..)| *who)
            .collect();
        for who in &fixers {
            tally.credit(*who, format!("reset the {}", device.label.to_lowercase()));
        }
        let by = fixers.iter().filter_map(|w| players.get(*w).ok()).filter_map(|(_, _, n)| n).map(|n| n.to_string()).collect::<Vec<_>>();
        if !by.is_empty() {
            mission.report.push(format!("{} reset the {} at {}.", by.join(" and "), device.label.to_lowercase(), since(tick.0)));
        }
    }
    // Our shield is its emitter.
    let shield = devices.live(design, "shield") || design.device_index("shield").is_none();
    if ours.shield != shield {
        ours.shield = shield;
    }
}

/// The objective and markers for the HUD.
fn show(
    mission: Single<&mut Mission>,
    ships: Query<(Entity, &Combatant, &ShipHull, Has<HomeShip>)>,
    damage: Option<Single<&Damage>>,
    tick: Res<SimTick>,
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
    // Damage aboard: on the board, and where to fix it.
    let mut status = Vec::new();
    if let (Some(damage), Some(home)) = (damage, home)
        && mission.phase == Phase::Underway
    {
        let design = ShipHull::new(ship::HOME).design();
        for &(i, at) in &damage.knocked {
            let device = &design.devices[i as usize];
            let ago = tick.0.wrapping_sub(at) / TICK_HZ as u32;
            status.push(format!("DAMAGE: {} knocked out {ago} s ago", device.label));
            if let Some(h) = marked.id(home) {
                let mut marker = Marker::device(h, design, &device.id, MarkerKind::Device);
                marker.label = format!("Reset: {}", device.label);
                markers.push(marker);
            }
        }
    }
    Mission::show(&mut mission, objective.into(), status, markers);
}
