//! A co-op smoke test's host side (debug): `HULLBREAKERS_COOP_TOUR=<dir>`
//! on the host walks the first *remote* crew member (and, where a step
//! needs it, the host's own player) through each scenario's set pieces,
//! putting them where something can be tried (the locker and workbench, a
//! knocked-out breaker, a crate, a fight, the console, the mission wall,
//! the escort's cell and devices, the derelict's devices, a gate, a pod,
//! the zoo) and setting what the step needs (a death, guns, faults). The
//! tester drives the client's own input (press, fire, reel, the menus)
//! from outside, and follows along by `<dir>/step`, which holds the current
//! step's number and name, and lets the tour go on by writing the step's
//! name to `<dir>/ack`. `HULLBREAKERS_COOP_ONLY=a,b,...` runs only the
//! named steps. It's the authority that moves them, so each step is also a
//! hard correction for the client's prediction to take.

use bevy::prelude::*;

use super::LocalPlayer;
use crate::sim::bots::{Affix, Affixes, Robot};
use crate::sim::devices::Devices;
use crate::sim::mission::{Director, Mission, ScenarioKind};
use crate::sim::pickups::{Pickup, PickupKind};
use crate::sim::player::{Grapple, Mode, Player};
use crate::sim::progression::Profile;
use crate::sim::shots::Shoves;
use crate::sim::scenarios::boarding::CARRIER;
use crate::sim::scenarios::dead_ship::DERELICT;
use crate::sim::scenarios::escort::FREIGHTER;
use crate::sim::scenarios::grapple_run::COURSE;
use crate::sim::scenarios::recovery::Pod;
use crate::sim::ship::{HOME, ShipHull};
use crate::sim::tuning::Tuning;
use crate::sim::vitals::Vitals;

#[derive(Resource)]
pub struct CoopTour {
    dir: std::path::PathBuf,
    next: usize,
    /// Seconds into the current step.
    timer: f32,
    /// The current step's set-up is done.
    placed: bool,
}

pub fn coop_tour_from_env() -> Option<CoopTour> {
    let dir = std::env::var_os("HULLBREAKERS_COOP_TOUR")?;
    Some(CoopTour {
        dir: dir.into(),
        next: 0,
        timer: 0.0,
        placed: false,
    })
}

/// Where a step puts someone.
#[derive(Clone, Copy)]
enum Place {
    /// Leave them be.
    Stay,
    /// Booted on `ship`'s deck.
    Booted { ship: &'static str, feet: Vec3, forward: Vec3, pitch: f32 },
    /// In EVA at `offset` from `ship`, looking at `look_at`, drifting.
    Eva { ship: &'static str, offset: Vec3, look_at: Vec3, drift: Vec3 },
    /// Facing the kit the client left, from a step away.
    Kit,
    /// In EVA by the first pod, roped to it.
    Pod,
    /// In EVA a few metres in front of the gallery's `kind` with `affix`,
    /// facing it.
    Robot { kind: Robot, affix: Affix },
    /// Booted on `ship`'s deck (at height `deck`), `back` metres out from
    /// its device `id`, facing it.
    Device { ship: &'static str, id: &'static str, back: f32, deck: f32 },
    /// In front of the first of our ship's damage-control devices that's
    /// been knocked off its setting (or stay, if none is).
    Knocked,
}

/// What else a step does.
#[derive(Clone, Copy, PartialEq)]
enum Do {
    Nothing,
    Arm,
    Kill,
    /// Kills the client and the host where they stand.
    KillBoth,
    /// Blows the client and the host off the deck (an authority's shove,
    /// straight up).
    Blast,
    /// Logs the client's profile as the host holds it.
    LogProfile,
    /// Sets the chance a carrier hit knocks out our damage-control devices.
    Faults(f32),
}

struct Step {
    name: &'static str,
    scenario: ScenarioKind,
    place: Place,
    /// Where the host's own player goes (to look, or to try something).
    host: Place,
    action: Do,
    seconds: f32,
}

fn upper() -> f32 {
    ShipHull::new(CARRIER).design().marker("upper_deck").y
}

fn steps() -> Vec<Step> {
    use ScenarioKind::*;
    let booted = |ship, feet, forward, pitch| Place::Booted { ship, feet, forward, pitch };
    let step = |name, scenario, place, action, seconds| Step { name, scenario, place, host: Place::Stay, action, seconds };
    let device = |ship, id, back| Place::Device { ship, id, back, deck: 0.0 };
    let up = upper();
    let mut steps = vec![
        step("field_profile", Field, Place::Stay, Do::LogProfile, 6.0),
        // The armory and workbench in the field (no one shooting).
        step("field_locker", Field, booted(HOME, Vec3::new(2.4, 0.01, 9.0), Vec3::X, -0.05), Do::Nothing, 6.0),
        Step {
            // The host stands where it can see the client's hands.
            host: booted(HOME, Vec3::new(0.6, 0.01, 13.4), Vec3::new(1.0, 0.0, -1.0), -0.1),
            ..step("field_workbench", Field, booted(HOME, Vec3::new(2.4, 0.01, 11.6), Vec3::X, -0.16), Do::Nothing, 6.0)
        },
        // A scenario switch keeps what you carry.
        step("boarding_arrive", Boarding, Place::Stay, Do::LogProfile, 4.0),
        // Damage control: every carrier hit knocks something out a while.
        step("boarding_faults", Boarding, booted(HOME, Vec3::new(0.0, 0.01, 6.0), Vec3::NEG_Z, 0.0), Do::Faults(1.0), 10.0),
        step("dc_board", Boarding, device(HOME, "board", 1.8), Do::Faults(0.0), 4.0),
        step("dc_fix", Boarding, Place::Knocked, Do::Nothing, 4.0),
        step("dc_fix_2", Boarding, Place::Knocked, Do::Nothing, 4.0),
        step("dc_board_after", Boarding, device(HOME, "board", 1.8), Do::Nothing, 4.0),
        // A death with the guns on: the host tries the case, then its owner.
        step("boarding_die", Boarding, booted(HOME, Vec3::new(0.0, 0.01, 8.0), Vec3::NEG_Z, 0.0), Do::Kill, 5.0),
        Step { host: Place::Kit, ..step("boarding_host_tries", Boarding, Place::Stay, Do::Nothing, 5.0) },
        step("boarding_kit", Boarding, Place::Kit, Do::Nothing, 6.0),
        step("boarding_crate", Boarding, booted(CARRIER, Vec3::new(3.4, up + 0.01, -27.4), Vec3::new(1.0, 0.0, -1.0), -0.7), Do::Nothing, 6.0),
        step("boarding_fight", Boarding, booted(CARRIER, Vec3::new(0.0, up + 0.01, 10.0), Vec3::NEG_Z, 0.0), Do::Arm, 10.0),
        // On the carrier's roof, aft, where a hull crawler walks.
        Step {
            // The host too, a little way off: does it come for either?
            host: booted(CARRIER, Vec3::new(2.0, 6.61, 10.0), Vec3::Z, -0.1),
            ..step("boarding_crawler", Boarding, booted(CARRIER, Vec3::new(0.0, 6.61, 12.0), Vec3::Z, -0.1), Do::Nothing, 12.0)
        },
        step("boarding_console", Boarding, booted(CARRIER, Vec3::new(0.0, up + 0.01, -25.6), Vec3::NEG_Z, -0.38), Do::Nothing, 22.0),
        // Home, and away by the mission wall: the pay.
        step("boarding_wall", Boarding, booted(HOME, Vec3::new(-2.2, 0.01, -8.4), Vec3::NEG_X, 0.08), Do::Nothing, 5.0),
        step("field_debrief", Field, Place::Stay, Do::LogProfile, 8.0),
        // Escort: the bay key, the gangway, the cell, home with it.
        step("escort_key", Escort, device(FREIGHTER, "bay_key_bay", 1.0), Do::Nothing, 5.0),
        step("escort_gangway", Escort, device(FREIGHTER, "gangway_lever", 1.0), Do::Nothing, 5.0),
        // Looking down at it from close: then a set-down lands under the
        // crosshair too.
        step("escort_cell", Escort, booted(FREIGHTER, Vec3::new(0.0, 0.01, -16.9), Vec3::NEG_Z, -1.1), Do::Nothing, 8.0),
        step("escort_home", Escort, booted(HOME, Vec3::new(0.0, 0.01, 9.0), Vec3::NEG_Z, 0.0), Do::Nothing, 8.0),
        step("derelict_reactor", DeadShip, booted(DERELICT, Vec3::new(2.4, 0.01, 10.0), Vec3::X, -0.35), Do::Nothing, 6.0),
        step("derelict_junction", DeadShip, booted(DERELICT, Vec3::new(5.7, 0.01, 1.0), Vec3::X, -0.25), Do::Nothing, 6.0),
        step(
            "run_gate",
            GrappleRun,
            Place::Eva {
                ship: COURSE,
                offset: Vec3::new(0.286, 0.103, 0.953) * -8.0,
                look_at: Vec3::ZERO,
                drift: Vec3::new(0.286, 0.103, 0.953) * 5.0,
            },
            Do::Nothing,
            8.0,
        ),
        step("recovery_tow", Recovery, Place::Pod, Do::Nothing, 10.0),
        step("gallery_shielded", Gallery, Place::Robot { kind: Robot::Gunner, affix: Affix::Shielded }, Do::Arm, 6.0),
        step("gallery_armoured", Gallery, Place::Robot { kind: Robot::Gunner, affix: Affix::Armoured }, Do::Arm, 6.0),
        step("gallery_volatile", Gallery, Place::Robot { kind: Robot::Gunner, affix: Affix::Volatile }, Do::Arm, 6.0),
        // What only the authority decides, on both: a death where they
        // stand (the field's start put them on their deck spots), then a
        // blast off the deck.
        step("field_die", Field, Place::Stay, Do::KillBoth, 6.0),
        step("field_blast", Field, Place::Stay, Do::Blast, 6.0),
    ];
    if let Ok(only) = std::env::var("HULLBREAKERS_COOP_ONLY")
        && !only.is_empty()
    {
        let only: Vec<&str> = only.split(',').collect();
        steps.retain(|s| only.contains(&s.name));
    }
    steps
}

/// Seconds after a scenario switch before the step's set-up, so the world
/// has arrived on the client.
const ARRIVE_SECONDS: f32 = 4.0;

#[allow(clippy::too_many_arguments)]
pub fn run_coop_tour(
    time: Res<Time>,
    mut tour: ResMut<CoopTour>,
    mut director: ResMut<Director>,
    mut tuning: ResMut<Tuning>,
    missions: Query<&Mission>,
    mut crew: Query<(Entity, &mut Player, &mut Vitals, Option<&Profile>), (Without<LocalPlayer>, Without<Robot>)>,
    places: Places,
    mut host: Option<Single<(Entity, &mut Player, &mut Vitals), With<LocalPlayer>>>,
    mut shoves: ResMut<Shoves>,
    mut died_at: Local<Option<Vec3>>,
) {
    let steps = steps();
    let Some(step) = steps.get(tour.next) else {
        return;
    };
    let Ok(mission) = missions.single() else { return };
    // Wait for the client.
    let Some((client, mut player, mut vitals, profile)) = crew.iter_mut().next() else {
        return;
    };
    // Switch scenario, unless the step is the one that leaves it (the
    // mission wall does it).
    if mission.kind != step.scenario && !tour.placed {
        director.start(step.scenario);
        tour.timer = -ARRIVE_SECONDS;
        return;
    }
    tour.timer += time.delta_secs();
    if tour.timer < 0.0 {
        return;
    }
    if !tour.placed {
        tour.placed = true;
        put(step.place, &mut player, &places, *died_at);
        if let Some(host) = host.as_mut() {
            put(step.host, &mut host.1, &places, *died_at);
        }
        match step.action {
            Do::Nothing => {}
            Do::Arm => {
                player.loadout.restock();
                player.loadout.current = 0;
            }
            Do::Kill => {
                // Where they fell, to find their kit by.
                let frame = player.mode.body().and_then(|b| places.ships.get(b).ok().map(|(_, t, _)| *t));
                *died_at = Some(match (player.mode, frame) {
                    (Mode::Booted { feet, .. }, Some(t)) => t.transform_point(feet),
                    (Mode::Eva { position, .. }, _) => position,
                    _ => Vec3::ZERO,
                });
                vitals.health = 0.0;
                vitals.since_hurt = 0.0;
            }
            Do::KillBoth => {
                // Hurt just now, or the suit heals them first.
                for v in [Some(&mut *vitals), host.as_mut().map(|h| &mut *h.2)].into_iter().flatten() {
                    v.health = 0.0;
                    v.since_hurt = 0.0;
                }
            }
            Do::Blast => {
                let mass = tuning.player_mass;
                for (who, mode) in [Some((client, player.mode)), host.as_ref().map(|h| (h.0, h.1.mode))].into_iter().flatten() {
                    let up = mode.body().and_then(|b| places.ships.get(b).ok()).map_or(Vec3::Y, |(_, t, _)| t.rotation * Vec3::Y);
                    shoves.0.push((who, up * 12.0 * mass));
                }
            }
            // At the step's end (once the upload has had time to land).
            Do::LogProfile => {}
            Do::Faults(chance) => tuning.hit_fault_chance = chance,
        }
        let _ = std::fs::write(tour.dir.join("step"), format!("{} {}\n", tour.next, step.name));
        info!("coop tour: {}", step.name);
    }
    // Damage control as the host has it, while it matters.
    if step.name.starts_with("dc_") || step.name == "boarding_faults" {
        if let Some((_, devices)) = places.devices.iter().find(|(h, _)| h.0 == HOME)
            && (tour.timer * 2.0).floor() != ((tour.timer - time.delta_secs()) * 2.0).floor()
        {
            let design = ShipHull::new(HOME).design();
            let states: Vec<String> = design
                .devices
                .iter()
                .zip(&devices.0)
                .filter(|(d, _)| d.fault)
                .map(|(d, s)| format!("{}={}{}", d.id, s.set, if s.set == d.start { "" } else { " (OUT)" }))
                .collect();
            info!("coop tour: damage control on the host: {}", states.join(", "));
        }
    }
    // On to the next once the step's time is up and the tester is done
    // with it (it writes the step's name to `<dir>/ack`).
    let acked = std::fs::read_to_string(tour.dir.join("ack")).is_ok_and(|a| a.trim() == step.name);
    if tour.timer >= step.seconds && acked {
        if step.action == Do::LogProfile {
            match profile {
                Some(p) => info!(
                    "coop tour: the client's profile on the host: salvage {}, xp {}, missions {}, stash {:?}, carrying {:?}",
                    p.salvage,
                    p.xp,
                    p.missions,
                    p.stash.iter().map(|w| w.name()).collect::<Vec<_>>(),
                    player.loadout.inventory.weapons.iter().flatten().map(|w| w.name()).collect::<Vec<_>>()
                ),
                None => info!("coop tour: the client has no profile on the host"),
            }
        }
        tour.next += 1;
        tour.timer = 0.0;
        tour.placed = false;
        if tour.next == steps.len() {
            let _ = std::fs::write(tour.dir.join("step"), format!("{} done\n", tour.next));
        }
    }
}

/// What placing someone needs to find.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Places<'w, 's> {
    ships: Query<'w, 's, (Entity, &'static Transform, &'static ShipHull)>,
    pickups: Query<'w, 's, (&'static Pickup, &'static Transform)>,
    pods: Query<'w, 's, (Entity, &'static Pod, &'static Transform)>,
    robots: Query<'w, 's, (&'static Robot, &'static Affixes, &'static Player), Without<LocalPlayer>>,
    devices: Query<'w, 's, (&'static ShipHull, &'static Devices)>,
}

/// Puts `player` at `place`.
fn put(place: Place, player: &mut Player, places: &Places, died_at: Option<Vec3>) {
    let (ships, pickups, pods, robots) = (&places.ships, &places.pickups, &places.pods, &places.robots);
    let ship = |id: &str| ships.iter().find(|(.., h)| h.0 == id).map(|(e, t, _)| (e, *t));
    match place {
        Place::Stay => {}
        Place::Booted { ship: id, feet, forward, pitch } => {
            if let Some((body, _)) = ship(id) {
                let up = Vec3::Y;
                let right = forward.cross(up).normalize();
                let forward = up.cross(right);
                player.mode = Mode::Booted {
                    body,
                    feet,
                    up,
                    velocity: Vec3::ZERO,
                    orientation: Quat::from_mat3(&Mat3::from_cols(right, up, -forward)),
                    pitch,
                };
                player.grapple = None;
            }
        }
        Place::Eva { ship: id, offset, look_at, drift } => {
            if let Some((_, t)) = ship(id) {
                let position = t.transform_point(offset);
                player.mode = Mode::Eva {
                    position,
                    velocity: t.rotation * drift,
                    orientation: Transform::from_translation(position).looking_at(t.transform_point(look_at), t.up()).rotation,
                    pitch: 0.0,
                };
                player.grapple = None;
                player.flight_assist = drift == Vec3::ZERO;
            }
        }
        Place::Kit => {
            let kit = died_at.and_then(|at| {
                pickups
                    .iter()
                    .filter(|(p, _)| p.kind == PickupKind::Dropped)
                    .min_by(|a, b| a.1.translation.distance(at).total_cmp(&b.1.translation.distance(at)))
            });
            match kit {
                // On a deck: a step aft of it, looking down at it.
                Some((case, _)) if case.body.is_some() => {
                    player.mode = Mode::Booted {
                        body: case.body.unwrap(),
                        feet: case.at - Vec3::Y * 0.19 + Vec3::Z * 1.3,
                        up: Vec3::Y,
                        velocity: Vec3::ZERO,
                        orientation: Quat::IDENTITY,
                        pitch: -0.75,
                    };
                }
                // Adrift: beside it, drifting with it.
                Some((case, t)) => {
                    let position = t.translation + Vec3::new(0.0, 0.3, 1.6);
                    player.mode = Mode::Eva {
                        position,
                        velocity: case.velocity,
                        orientation: Transform::from_translation(position).looking_at(t.translation, Vec3::Y).rotation,
                        pitch: 0.0,
                    };
                    player.flight_assist = false;
                }
                None => {}
            }
        }
        Place::Device { ship: id, id: device, back, deck } => {
            if let Some((body, _, hull)) = ships.iter().find(|(.., h)| h.0 == id) {
                let design = hull.design();
                let d = &design.devices[design.device(device)];
                let out = Vec3::new(d.face.x, 0.0, d.face.z).normalize_or(Vec3::Z);
                let feet = Vec3::new(d.at.x, deck + 0.01, d.at.z) + out * back;
                let eye = deck + 1.62;
                let pitch = (d.at.y - eye).atan2(back);
                let forward = -out;
                let right = forward.cross(Vec3::Y).normalize();
                player.mode = Mode::Booted {
                    body,
                    feet,
                    up: Vec3::Y,
                    velocity: Vec3::ZERO,
                    orientation: Quat::from_mat3(&Mat3::from_cols(right, Vec3::Y, -forward)),
                    pitch,
                };
                player.grapple = None;
            }
        }
        Place::Knocked => {
            let found = places.devices.iter().find(|(h, _)| h.0 == HOME).and_then(|(hull, devices)| {
                let design = hull.design();
                design.devices.iter().zip(&devices.0).find(|(d, s)| d.fault && s.set != d.start).map(|(d, _)| d.id.clone())
            });
            if let Some(id) = found {
                let id: &'static str = Box::leak(id.into_boxed_str());
                put(Place::Device { ship: HOME, id, back: 1.1, deck: 0.0 }, player, places, died_at);
            }
        }
        Place::Robot { kind, affix } => {
            // The zoo's robots float in EVA.
            let found = robots.iter().find_map(|(k, a, p)| match p.mode {
                Mode::Eva { position, .. } if *k == kind && a.has(affix) => Some(position),
                _ => None,
            });
            if let Some(target) = found {
                let position = target + Vec3::new(0.0, 0.0, 6.0);
                player.mode = Mode::Eva {
                    position,
                    velocity: Vec3::ZERO,
                    orientation: Transform::from_translation(position).looking_at(target, Vec3::Y).rotation,
                    pitch: 0.0,
                };
                player.grapple = None;
                player.flight_assist = true;
            }
        }
        Place::Pod => {
            if let Some((pod, _, t)) = pods.iter().min_by_key(|(_, p, _)| p.index) {
                let position = t.translation + Vec3::new(-6.0, 2.0, 4.0);
                player.mode = Mode::Eva {
                    position,
                    velocity: Vec3::ZERO,
                    orientation: Transform::from_translation(position).looking_at(t.translation, Vec3::Y).rotation,
                    pitch: 0.0,
                };
                player.grapple = Some(Grapple {
                    target: pod,
                    anchor: Vec3::ZERO,
                    length: position.distance(t.translation),
                    tension: 0.0,
                });
            }
        }
    }
}
