//! Stress set-ups for profiling (see `profile` and `tools/stress`). Off
//! unless asked for; only a host (which runs the simulation) can set one up.
//!
//! `HULLBREAKERS_STRESS=firefight`: in the asteroid field, 10 Gunners on our
//! ship's roof and 20 Swarmers around it, all fighting the local player, who
//! stands on the roof firing the SMG without a break while turning slowly.
//! Nobody dies (health is topped up) and nobody runs dry, so the load holds
//! steady: tracers, sparks, bullets, robot brains and hits all at once.

use bevy::prelude::*;

use super::{LocalInput, LocalPlayer};
use crate::sim::bots::{Robot, new_robot};
use crate::sim::mission::{Director, Mission, ScenarioKind};
use crate::sim::player::{Mode, Player};
use crate::sim::ship::HomeShip;
use crate::sim::tuning::Tuning;
use crate::sim::vitals::Vitals;

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stress {
    Firefight,
}

pub fn stress_from_env() -> Option<Stress> {
    match std::env::var("HULLBREAKERS_STRESS").ok()?.as_str() {
        "firefight" => Some(Stress::Firefight),
        other => {
            warn!("unknown HULLBREAKERS_STRESS {other:?} (firefight)");
            None
        }
    }
}

pub struct StressPlugin(pub Stress);

impl Plugin for StressPlugin {
    fn build(&self, app: &mut App) {
        match self.0 {
            Stress::Firefight => {
                app.init_resource::<Firefight>()
                    .add_systems(Update, (set_up_firefight, sustain_firefight).chain())
                    .add_systems(PreUpdate, hold_fire.after(super::input::read_input));
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
        commands.spawn(new_robot(Robot::Gunner, mode, home, feet, &tuning, 1000 + i as u32));
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
        commands.spawn(new_robot(Robot::Swarmer, mode, home, post, &tuning, 2000 + i as u32));
    }
    info!("stress: firefight set up ({GUNNERS} Gunners, {SWARMERS} Swarmers)");
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
