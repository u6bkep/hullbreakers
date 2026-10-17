//! A scene: the crew jumps in beside sites laid out from a seed (see
//! `sim::scene`), with something to do at each. One is the main objective;
//! the rest are sides, for the pay. Once the main is done, the jump window
//! opens: the crew has the scene's window (s) to be aboard, and may jump
//! early from the mission wall (its "Asteroid field" button). The scene is
//! won if the main is done when the ship jumps; who's aboard is the
//! debrief's business (`homecoming`).
//!
//! The encounters are generic over designs: a site's design says which it
//! hosts, and what the encounter refers to (a device, a cargo) is in the
//! design (see docs/scenes.md). This module builds the plan's world, reads
//! the world for each encounter's progress, and tells the crew what to do.
//!
//! Results: `main_done` (1 or 0), `sides` and `sides_done`, `aboard` (of
//! the crew, at the jump).

use std::collections::BTreeSet;

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::{Scenario, Stage, after_banner};
use crate::sim::asteroids::{FieldLayout, RockField};
use crate::sim::bodies::{Bodies, PreviousVelocity};
use crate::sim::bots::{self, Brain, Robot, Threat};
use crate::sim::design::{ShipDesign, design};
use crate::sim::devices::Devices;
use crate::sim::frame::BodyFrame;
use crate::sim::items::{CargoKind, Item, crate_loot, draw};
use crate::sim::mission::{CrewTally, Director, MarkedBodies, Marker, MarkerKind, Mission, Phase, ScenarioKind, Staged, clock, countdown};
use crate::sim::pickups::{self, CRATE_HALF, PICKUP_HALF, Pickup, PickupKind};
use crate::sim::player::Player;
use crate::sim::scene::{Encounter, ScenePlan, plan};
use crate::sim::scenarios::recovery::{Pod, PodKind, PodState, at_the_hold};
use crate::sim::ship::{self, HomeShip, ShipHull, in_hold};
use crate::sim::tuning::Tuning;
use crate::sim::{SimTick, TICK_HZ};

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::Scene,
    id: "scene",
    label: "Scene",
    build,
    arrive: None,
    systems: |app| super::judge(app, ScenarioKind::Scene, run_scene),
    return_after: after_banner,
    homecoming: true,
};

/// Survivors adrift round a wreck lie this far out (m), on this much air
/// (s), the nearest first.
const POD_REACH: (f32, f32) = (50.0, 140.0);
const POD_AIR: (f32, f32) = (150.0, 400.0);

fn build(stage: &mut Stage) -> Entity {
    let plan = plan(stage.scene);
    info!("building {}: {} sites, threat {}", plan.seed.name(), plan.sites.len(), plan.threat);
    let (commands, tick) = (&mut *stage.commands, stage.tick);
    let home = commands.spawn(ship::parked_home_ship()).id();
    // The crew's opposition: the scene's threat, eased for a small crew.
    let threat = Threat(((plan.threat as usize + stage.players) / 2).clamp(1, 3) as u8);
    let mut rng = ChaCha8Rng::seed_from_u64(plan.seed.seed ^ 0x5cee);
    let mut sites = Vec::new();
    let mut pods: Vec<Vec<Entity>> = Vec::new();
    for (i, site) in plan.sites.iter().enumerate() {
        let design: &ShipDesign = design(site.design);
        let mut entity = commands.spawn(ship::site(&site.label, site.design, site.at, site.turn));
        if i == plan.main {
            // The field is grown with the main site, wherever it goes.
            entity.insert(RockField {
                layout: FieldLayout::Scene(plan.seed),
                seed: plan.seed.seed,
                count: plan.seed.size.rocks() as u32,
                laid: tick,
            });
        }
        let entity = entity.id();
        sites.push(entity);
        if site.crewed && stage.robots {
            let frame = BodyFrame {
                position: site.at,
                rotation: site.turn,
                center_of_mass: site.at,
                linear_velocity: Vec3::ZERO,
                angular_velocity: Vec3::ZERO,
            };
            // A hull turret sees 320 m and shoots to 300: at a site that
            // near, it would fire onto our deck from the start, so mounts
            // are left unmanned unless the site is well out of their reach.
            // The close fight is the walkers', flyers' and crawlers'.
            let turret_reach = Robot::Turret.sight();
            let out_of_reach = site.at.length() > turret_reach + site.clearance + 50.0;
            let crew: Vec<bots::Posting> = bots::compose_crew(design, threat, tick.wrapping_add(i as u32) as u64)
                .into_iter()
                .filter(|p| out_of_reach || p.robot != Robot::Turret)
                .collect();
            bots::man_posts(commands, stage.tuning, entity, design, &frame, crew, None);
        }
        pickups::stock_ship(commands, entity, design, draw(plan.seed.seed as u32, i as u32));
        let mut adrift = Vec::new();
        match &site.encounter {
            Encounter::Salvage { cargo, released_by: None } => {
                commands.spawn((Name::new(cargo.name()), prize(entity, design, *cargo, tick)));
            }
            Encounter::Rescue { pods: n } => {
                for k in 0..*n {
                    let dir = crate::sim::asteroids::random_unit(&mut rng);
                    let out = rng.random_range(POD_REACH.0..POD_REACH.1);
                    let at = site.at + dir * out;
                    let air = POD_AIR.0 + (POD_AIR.1 - POD_AIR.0) * (k as f32 / (*n).max(2) as f32) + rng.random_range(0.0..20.0);
                    let drift = dir * rng.random_range(0.1..0.3);
                    let spin = crate::sim::asteroids::random_unit(&mut rng) * rng.random_range(0.1..0.3);
                    let pod = Pod {
                        kind: PodKind::Survivor,
                        index: (pods.iter().map(Vec::len).sum::<usize>() + k as usize) as u8,
                        oxygen_until: tick + (air * TICK_HZ as f32) as u32,
                        state: PodState::Adrift,
                    };
                    adrift.push(
                        commands
                            .spawn((
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
                            ))
                            .id(),
                    );
                }
            }
            _ => {}
        }
        pods.push(adrift);
    }
    for (k, drift) in plan.drifting.iter().enumerate() {
        commands.spawn((
            Name::new("Salvage crate"),
            Pickup {
                body: None,
                at: drift.at,
                velocity: drift.velocity,
                since: tick,
                items: crate_loot(draw(plan.seed.seed as u32 ^ 0xd41f, k as u32), drift.luck),
                kind: PickupKind::Crate,
                owner: None,
                owner_only: false,
            },
        ));
    }
    let n = plan.sites.len();
    commands.spawn((
        Name::new("Scene"),
        Staged,
        SceneState {
            plan,
            sites,
            pods,
            done: vec![None; n],
            released: vec![false; n],
            window_from: None,
            carriers: vec![BTreeSet::new(); n],
        },
    ));
    home
}

/// The cargo of a salvage, lying at its site's `objective`.
fn prize(site: Entity, design: &ShipDesign, cargo: CargoKind, tick: u32) -> Pickup {
    let half = if cargo.bulky() { CRATE_HALF } else { PICKUP_HALF };
    Pickup::dropped((Some(site), design.marker("objective") + Vec3::Y * half.y, Vec3::ZERO), tick, vec![Item::Cargo(cargo)])
}

/// How the scene stands.
#[derive(Component)]
struct SceneState {
    plan: ScenePlan,
    /// The sites' bodies, in the plan's order.
    sites: Vec<Entity>,
    /// Each site's survivors adrift (a rescue).
    pods: Vec<Vec<Entity>>,
    /// The tick each encounter was done on.
    done: Vec<Option<u32>>,
    /// Whether a salvage's cargo has been set out (released).
    released: Vec<bool>,
    /// When the main was done: the jump window opened.
    window_from: Option<u32>,
    /// Who's carried each salvage's cargo.
    carriers: Vec<BTreeSet<Entity>>,
}

/// Where a piece of cargo is.
enum Cargo {
    Carried(Entity),
    Lying { body: Option<Entity>, at: Vec3, world: Vec3 },
    /// Not set out yet, or gone.
    Nowhere,
}

fn carries(player: &Player, cargo: CargoKind) -> bool {
    let inventory = &player.loadout.inventory;
    inventory.carrying == Some(cargo) || inventory.pack.contains(&Some(Item::Cargo(cargo)))
}

/// What the judge found of one encounter this tick.
struct Progress {
    done: bool,
    /// Its line on the board.
    line: String,
    markers: Vec<Marker>,
}

#[allow(clippy::too_many_arguments)]
fn run_scene(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    bodies: Bodies,
    director: Res<Director>,
    mut commands: Commands,
    state: Option<Single<&mut SceneState>>,
    ships: Query<(&ShipHull, &Devices)>,
    homes: Query<Entity, With<HomeShip>>,
    players: Query<(Entity, &Player, Option<&Name>), Without<Robot>>,
    robots: Query<&Brain, With<Robot>>,
    pickups: Query<&Pickup>,
    mut pods: Query<(Entity, &mut Pod, &Position)>,
    marked: MarkedBodies,
    mission: Single<&mut Mission>,
    mut tally: ResMut<CrewTally>,
) {
    let Some(state) = state else { return };
    let mut state = state.into_inner();
    let mut mission = mission.into_inner();
    let Some(home) = homes.iter().next() else { return };
    let Some(ship) = bodies.frame(home) else { return };
    if state.plan.sites.is_empty() {
        Mission::show(&mut mission, "Nothing here: no design offers a scene".into(), vec![state.plan.seed.name()], vec![]);
        return;
    }
    let name = |e: Entity| players.get(e).ok().and_then(|(_, _, n)| n).map_or("?".to_string(), |n| n.to_string());
    let started = mission.started;
    let since = |t: u32| clock(t.wrapping_sub(started) as f32 / TICK_HZ as f32);
    let underway = mission.phase == Phase::Underway;
    let crew: Vec<Entity> = players.iter().map(|(e, ..)| e).collect();
    let aboard = players.iter().filter(|(_, p, _)| p.mode.body() == Some(home)).count();

    let mut progress: Vec<Progress> = Vec::new();
    for i in 0..state.plan.sites.len() {
        let site = state.sites[i];
        let site_id = marked.id(site);
        let design: &ShipDesign = design(state.plan.sites[i].design);
        let label = state.plan.sites[i].label.clone();
        let devices = ships.get(site).ok().map(|(_, d)| d);
        let live = |id: &str| devices.is_some_and(|d| d.live(design, id));
        let entry = site_id.map(|b| Marker::on(b, design.marker("entry") + Vec3::Y * 1.2, &label, MarkerKind::Goal));
        let device = |id: &str, kind| site_id.map(|b| Marker::device(b, design, id, kind));
        let hold = marked.id(home).map(|h| Marker::on(h, Vec3::new(0.0, 1.5, 9.0), "Hold", MarkerKind::Exit));
        let already = state.done[i].is_some();
        let p = match state.plan.sites[i].encounter.clone() {
            Encounter::Salvage { cargo, released_by } => {
                // Set out once released.
                if let Some(id) = &released_by
                    && !state.released[i]
                    && live(id)
                {
                    state.released[i] = true;
                    commands.spawn((Name::new(cargo.name()), prize(site, design, cargo, tick.0)));
                    mission.report.push(format!("{} released at {}.", cargo.name(), since(tick.0)));
                }
                let item = Item::Cargo(cargo);
                let carrier = players.iter().find(|(_, p, _)| carries(p, cargo));
                let where_ = match carrier {
                    Some((who, ..)) => Cargo::Carried(who),
                    None => pickups
                        .iter()
                        .find(|p| p.items.contains(&item))
                        .and_then(|p| Some(Cargo::Lying { body: p.body, at: p.at, world: p.sim_pose(&bodies, tick.0)?.0 }))
                        .unwrap_or(Cargo::Nowhere),
                };
                if let Cargo::Carried(who) = where_
                    && state.carriers[i].insert(who)
                {
                    tally.credit(who, format!("carried the {}", cargo.name().to_lowercase()));
                }
                let at_home = match where_ {
                    Cargo::Carried(who) => players.get(who).is_ok_and(|(_, p, _)| {
                        let own = p.mode.body().and_then(|b| bodies.frame(b));
                        p.mode.body() == Some(home) || in_hold(ship.point_to_local(p.mode.world_kinematics(&tuning, own.as_ref()).0))
                    }),
                    Cargo::Lying { world, .. } => in_hold(ship.point_to_local(world)),
                    Cargo::Nowhere => false,
                };
                let (place, markers): (String, Vec<Option<Marker>>) = match where_ {
                    _ if at_home => ("home".into(), vec![]),
                    Cargo::Carried(who) => (format!("carried by {}", name(who)), vec![hold]),
                    Cargo::Lying { body: Some(b), at, .. } => (
                        if b == site { format!("in the {}", label.to_lowercase()) } else { "set down".into() },
                        vec![marked.id(b).map(|b| Marker::on(b, at, cargo.name(), MarkerKind::Goal))],
                    ),
                    Cargo::Lying { body: None, world, .. } => ("adrift".into(), vec![Some(Marker::at(world, cargo.name(), MarkerKind::Goal))]),
                    Cargo::Nowhere => match &released_by {
                        Some(id) if !state.released[i] => (
                            format!("not yet released ({}, {})", design.devices[design.device(id)].label.to_lowercase(), label),
                            vec![entry, device(id, MarkerKind::Device)],
                        ),
                        _ => ("lost".into(), vec![]),
                    },
                };
                Progress {
                    // Judged as it stands: carried back off the ship, it's
                    // not home (the jump checks again).
                    done: at_home,
                    line: format!("{}: {place}", cargo.name()),
                    markers: markers.into_iter().flatten().collect(),
                }
            }
            Encounter::Restore { device: id } => {
                let on = live(&id);
                let dlabel = design.devices[design.device(&id)].label.clone();
                Progress {
                    done: already || on,
                    line: format!("{label} {}: {}", dlabel.to_lowercase(), if on || already { "live" } else { "dead" }),
                    markers: if on || already { vec![] } else { [entry, device(&id, MarkerKind::Device)].into_iter().flatten().collect() },
                }
            }
            Encounter::Clear => {
                let left = robots.iter().filter(|b| b.home == site).count();
                Progress {
                    done: already || left == 0,
                    line: format!("{label}: {} robots aboard", left),
                    markers: if left == 0 { vec![] } else { entry.into_iter().collect() },
                }
            }
            Encounter::Rescue { pods: n } => {
                let mine = state.pods[i].clone();
                let mut adrift = Vec::new();
                let mut home_count = 0u32;
                let mut lost = 0u32;
                for &e in &mine {
                    let Ok((_, pod, at)) = pods.get(e) else {
                        home_count += 1;
                        continue;
                    };
                    match pod.state {
                        PodState::Lost => lost += 1,
                        PodState::Adrift => adrift.push((e, *pod, at.0)),
                    }
                }
                for (e, pod, at) in &adrift {
                    if !underway {
                        break;
                    }
                    let towing: Vec<Entity> = players.iter().filter(|(_, p, _)| p.grapple.is_some_and(|g| g.target == *e)).map(|(w, ..)| w).collect();
                    if at_the_hold(ship.point_to_local(*at)) {
                        let by: Vec<String> = towing.iter().map(|&w| name(w)).collect();
                        mission.report.push(format!("{} home at {}, by {}.", pod.name(), since(tick.0), if by.is_empty() { "nobody's rope".into() } else { by.join(" and ") }));
                        for w in towing {
                            tally.credit(w, format!("brought {} home", pod.name()));
                        }
                        commands.entity(*e).despawn();
                        home_count += 1;
                    } else if tick.0 >= pod.oxygen_until {
                        if let Ok((_, mut p, _)) = pods.get_mut(*e) {
                            p.state = PodState::Lost;
                        }
                        mission.report.push(format!("{} ran out of air at {}.", pod.name(), since(tick.0)));
                        lost += 1;
                    }
                }
                let out = n as u32 - home_count - lost;
                let markers: Vec<Marker> = adrift
                    .iter()
                    .filter(|(_, p, _)| p.pinging(tick.0))
                    .filter_map(|(e, p, _)| Some(Marker::on(marked.id(*e)?, Vec3::ZERO, &p.name(), MarkerKind::Goal)))
                    .chain(players.iter().any(|(_, p, _)| p.grapple.is_some_and(|g| mine.contains(&g.target))).then_some(hold).flatten())
                    .collect();
                Progress {
                    // Everyone accounted for, and someone saved: all lost is
                    // a rescue that never happened.
                    done: already || (out == 0 && home_count > 0),
                    line: format!("{label}: {home_count} of {n} home, {out} adrift{}", if lost > 0 { format!(", {lost} lost") } else { String::new() }),
                    markers,
                }
            }
        };
        if p.done && state.done[i].is_none() && underway {
            state.done[i] = Some(tick.0);
            // A side's first completion is its report; a salvage carried
            // back off is judged again at the jump (`progress`).
            let what = state.plan.sites[i].encounter.label(&label);
            let role = if i == state.plan.main { "Main objective" } else { "Side" };
            mission.report.push(format!("{role} done at {}: {what}.", since(tick.0)));
        }
        progress.push(p);
    }

    // The jump window: opens when the main is first done, and stays open.
    let main_done = progress[state.plan.main].done;
    if main_done && state.window_from.is_none() {
        state.window_from = Some(tick.0);
    }
    let sides = state.plan.sites.len().saturating_sub(1);
    let sides_done = progress.iter().enumerate().filter(|(i, p)| *i != state.plan.main && p.done).count();
    let window_left = state.window_from.map(|from| state.plan.window - tick.0.wrapping_sub(from) as f32 / TICK_HZ as f32);
    // Jumping: the window's closed, or the crew pressed the wall (any
    // button: leaving is leaving).
    let jumped_early = director.switching().is_some();
    if underway && (window_left.is_some_and(|left| left <= 0.0) || jumped_early) {
        mission.result("main_done", main_done as u32 as f32);
        mission.result("sides", sides as f32);
        mission.result("sides_done", sides_done as f32);
        mission.result("aboard", aboard as f32);
        if main_done {
            mission.report.push(format!("Jumped with {aboard} of {} aboard; {sides_done} of {sides} sides done.", crew.len()));
            mission.decide(Phase::Won, tick.0, "JUMP COMPLETE");
        } else {
            mission.report.push("Jumped without the main objective.".into());
            mission.decide(Phase::Lost, tick.0, "SCENE ABANDONED");
        }
    }

    // What to do now, and where.
    let main = &progress[state.plan.main];
    let (objective, markers): (String, Vec<Marker>) = match mission.phase {
        Phase::Won => ("Jump complete".into(), vec![]),
        Phase::Lost => ("Scene abandoned".into(), vec![]),
        Phase::Underway if !main_done => (
            format!("Main: {}", state.plan.sites[state.plan.main].encounter.label(&state.plan.sites[state.plan.main].label)),
            main.markers.iter().cloned().chain(progress.iter().enumerate().filter(|(i, _)| *i != state.plan.main).flat_map(|(_, p)| p.markers.iter().cloned().map(|mut m| {
                if m.kind == MarkerKind::Goal {
                    m.kind = MarkerKind::Waypoint;
                }
                m
            }))).collect(),
        ),
        Phase::Underway => {
            let ship_marker = marked.id(home).map(|h| Marker::on(h, Vec3::new(0.0, 1.5, 8.0), "Ship", MarkerKind::Exit));
            let objective = if sides_done < sides {
                "Jump window open: finish the sides or get everyone aboard (the wall's field button jumps)".to_string()
            } else {
                "Jump window open: everyone aboard, then jump from the wall".to_string()
            };
            (objective, progress.iter().enumerate().filter(|(i, _)| *i != state.plan.main).flat_map(|(_, p)| p.markers.iter().cloned()).chain(ship_marker).collect())
        }
    };
    let mut status = vec![state.plan.seed.name()];
    for (i, p) in progress.iter().enumerate() {
        let tag = if i == state.plan.main { "MAIN" } else { "side" };
        let tick_ = if p.done { " ✓" } else { "" };
        status.push(format!("{tag}: {}{tick_}", p.line));
    }
    if let Some(left) = window_left
        && underway
    {
        status.push(format!("Jump in {}  aboard {aboard} of {}", countdown(left), crew.len()));
    }
    Mission::show(&mut mission, objective, status, markers);
}
