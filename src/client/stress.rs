//! Stress set-ups for profiling (see `profile` and `tools/stress`). Off
//! unless asked for; only a host (which runs the simulation) can set one up.
//!
//! `HULLBREAKERS_STRESS=firefight`: in the asteroid field, 10 Gunners on our
//! ship's roof and 20 Swarmers around it, all fighting the local player, who
//! stands on the roof firing the SMG without a break while turning slowly.
//! Nobody dies (health is topped up) and nobody runs dry, so the load holds
//! steady: tracers, sparks, bullets, robot brains and hits all at once.
//!
//! `=carrier` (start in the boarding action): the player aboard the carrier,
//! on its lower deck, firing the same way, with its crew coming at them:
//! the fight inside, its robots, lights and effects.
//! `=derelict` (start in the dead ship): the derelict's power routed to its
//! vault as a crew would have (its rooms lit), the player in its junction.
//! `=aboard` (start in the boarding action, for playtesting the fight
//! inside): the player armed in the carrier's hangar, as if just in through
//! its door, and nothing else done: they and the crew can die, the player
//! drives.

use bevy::prelude::*;

use super::{LocalInput, LocalPlayer};
use crate::sim::bots::{Affixes, Robot, new_robot};
use crate::sim::mission::{Director, Mission, ScenarioKind};
use crate::sim::player::{Mode, Player};
use crate::sim::ship::HomeShip;
use crate::sim::tuning::Tuning;
use crate::sim::vitals::Vitals;

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stress {
    Firefight,
    Carrier,
    Derelict,
    Aboard,
}

pub fn stress_from_env() -> Option<Stress> {
    match std::env::var("HULLBREAKERS_STRESS").ok()?.as_str() {
        "firefight" => Some(Stress::Firefight),
        "carrier" => Some(Stress::Carrier),
        "derelict" => Some(Stress::Derelict),
        "aboard" => Some(Stress::Aboard),
        other => {
            warn!("unknown HULLBREAKERS_STRESS {other:?} (firefight, carrier, derelict, aboard)");
            None
        }
    }
}

pub struct StressPlugin(pub Stress);

impl Plugin for StressPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0);
        match self.0 {
            Stress::Firefight => {
                app.init_resource::<Firefight>()
                    .add_systems(Update, (set_up_firefight, sustain_firefight).chain())
                    .add_systems(PreUpdate, hold_fire.after(super::input::read_input));
            }
            Stress::Carrier => {
                app.init_resource::<Firefight>()
                    .add_systems(Update, (board_carrier, sustain_firefight).chain())
                    .add_systems(PreUpdate, hold_fire.after(super::input::read_input));
            }
            Stress::Derelict => {
                app.init_resource::<Firefight>().add_systems(Update, light_derelict);
            }
            Stress::Aboard => {
                app.init_resource::<Firefight>().add_systems(Update, board_carrier);
            }
        }
    }
}

const GUNNERS: usize = 10;
const SWARMERS: usize = 20;
/// The top of our ship's roof (ship coordinates), as the screenshot tour has it.
const ROOF: f32 = 3.31;
/// How fast the player turns while firing (rad/s): a full circle in ~16 s.
const SWEEP: f32 = 0.4;

#[derive(Resource, Default)]
struct Firefight {
    /// Seconds since the local player appeared.
    waited: f32,
    started: bool,
}

/// Once our player is aboard in the field, arms them, puts them on the
/// roof, and brings the robots.
#[allow(clippy::too_many_arguments)]
fn set_up_firefight(
    time: Res<Time>,
    mut state: ResMut<Firefight>,
    director: Option<Res<Director>>,
    mission: Option<Single<&Mission>>,
    home: Option<Single<Entity, With<HomeShip>>>,
    mut player: Option<Single<&mut Player, With<LocalPlayer>>>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    if state.started {
        return;
    }
    // Only the authority simulates robots.
    if director.is_none() {
        return;
    }
    let (Some(mission), Some(home), Some(player)) = (mission, home, player.as_mut()) else {
        return;
    };
    if mission.kind != ScenarioKind::Field {
        return;
    }
    state.waited += time.delta_secs();
    if state.waited < 1.0 {
        return;
    }
    state.started = true;
    let home = *home;
    player.loadout.restock();
    // Stance 1: SMG in the main hand, grapple in the other.
    player.loadout.current = 0;
    player.boots_on = true;
    player.mode = Mode::Booted {
        body: home,
        feet: Vec3::new(0.0, ROOF, 12.0),
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    player.grapple = None;
    for i in 0..GUNNERS {
        let feet = Vec3::new(if i % 2 == 0 { -2.5 } else { 2.5 }, ROOF, -14.0 + 3.0 * (i / 2) as f32);
        let mode = Mode::Booted {
            body: home,
            feet,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
            // Facing aft, toward the player.
            orientation: Quat::from_rotation_y(std::f32::consts::PI),
            pitch: 0.0,
        };
        commands.spawn(new_robot(Robot::Gunner, Affixes::NONE, mode, home, feet, &tuning, 1000 + i as u32));
    }
    // The ship is at rest in the field (see `ship::drifting_home_ship`), so
    // its frame is close enough to the world's here.
    for i in 0..SWARMERS {
        let angle = i as f32 / SWARMERS as f32 * std::f32::consts::TAU;
        let post = Vec3::new(30.0 * angle.cos(), -8.0 + 4.0 * (i % 5) as f32, 30.0 * angle.sin());
        let mode = Mode::Eva {
            position: post,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
        commands.spawn(new_robot(Robot::Swarmer, Affixes::NONE, mode, home, post, &tuning, 2000 + i as u32));
    }
    info!("stress: firefight set up ({GUNNERS} Gunners, {SWARMERS} Swarmers)");
}

/// Once the boarding action is under way, puts the armed player on the
/// carrier's lower deck.
fn board_carrier(
    stress: Res<Stress>,
    time: Res<Time>,
    mut state: ResMut<Firefight>,
    director: Option<Res<Director>>,
    mission: Option<Single<&Mission>>,
    ships: Query<(Entity, &crate::sim::ship::ShipHull)>,
    mut player: Option<Single<&mut Player, With<LocalPlayer>>>,
) {
    if state.started || director.is_none() {
        return;
    }
    let (Some(mission), Some(player)) = (mission, player.as_mut()) else { return };
    if mission.kind != ScenarioKind::Boarding {
        return;
    }
    let Some((carrier, _)) = ships.iter().find(|(_, h)| h.0 == crate::sim::scenarios::boarding::CARRIER) else { return };
    state.waited += time.delta_secs();
    if state.waited < 1.0 {
        return;
    }
    state.started = true;
    player.loadout.restock();
    player.loadout.current = 0;
    player.boots_on = true;
    player.grapple = None;
    player.reference_body = Some(carrier);
    player.mode = Mode::Booted {
        body: carrier,
        // In the hangar, just in through the door (a playtest), or the
        // lower deck (the fight inside, for profiling).
        feet: if *stress == Stress::Aboard { Vec3::new(0.0, 0.01, 26.0) } else { Vec3::new(0.0, 0.01, 12.0) },
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    info!("stress: aboard the carrier");
}

/// Once in the dead ship, routes its power (its rooms lit) and puts the
/// player in its junction.
fn light_derelict(
    mut state: ResMut<Firefight>,
    time: Res<Time>,
    director: Option<Res<Director>>,
    mut ships: Query<(Entity, &crate::sim::ship::ShipHull, &mut crate::sim::devices::Devices)>,
    mut player: Option<Single<&mut Player, With<LocalPlayer>>>,
) {
    if state.started || director.is_none() {
        return;
    }
    let Some(player) = player.as_mut() else { return };
    let Some((derelict, hull, mut devices)) = ships.iter_mut().find(|(_, h, _)| h.0 == crate::sim::scenarios::dead_ship::DERELICT) else {
        return;
    };
    state.waited += time.delta_secs();
    if state.waited < 1.0 {
        return;
    }
    state.started = true;
    super::debug_shots::power_up(hull, &mut devices);
    player.boots_on = true;
    player.reference_body = Some(derelict);
    player.mode = Mode::Booted {
        body: derelict,
        feet: Vec3::new(4.6, 0.01, 1.0),
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
        pitch: 0.0,
    };
    info!("stress: in the lit derelict");
}

/// Keeps everyone alive and the player's guns loaded, so the load holds.
fn sustain_firefight(
    state: Res<Firefight>,
    mut vitals: Query<&mut Vitals>,
    mut player: Option<Single<&mut Player, With<LocalPlayer>>>,
) {
    if !state.started {
        return;
    }
    for mut v in &mut vitals {
        v.health = 1.0e6;
    }
    if let Some(player) = player.as_mut() {
        for kind in crate::sim::items::AmmoKind::ALL {
            *player.loadout.inventory.ammo_mut(kind) = kind.cap();
        }
    }
}

/// The player's input: fire held, turning slowly.
fn hold_fire(time: Res<Time>, state: Res<Firefight>, mut local: ResMut<LocalInput>) {
    if !state.started {
        return;
    }
    local.0.main.held = true;
    local.0.add_look(Vec2::new(SWEEP * time.delta_secs(), 0.0));
}
