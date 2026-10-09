//! Headless simulation: everything that would run on a server.
//!
//! Nothing here touches rendering, windows, or input devices. Input arrives as
//! [`player::PlayerCommand`] components; the client layer reads state back out.

pub mod asteroids;
pub mod bodies;
pub mod frame;
pub mod player;
pub mod rope;
pub mod ship;
pub mod tuning;

use avian3d::prelude::*;
use bevy::prelude::*;

pub const TICK_HZ: f64 = 60.0;

/// Visual material class of a collider. The sim only tags; the client decides
/// what it looks like.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SimMaterial {
    Hull,
    Trim,
    Rock,
}

/// Fixed-tick ordering within `FixedUpdate`, before the physics step.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SimSystems {
    /// Snapshot velocities so frame accelerations can be measured across
    /// everything that changes them this tick (scripted drive and the solver).
    RecordVelocities,
    /// Script-driven bodies set their velocities and the field is maintained.
    Drive,
}

pub struct SimPlugin {
    /// Spawn the asteroid field. Tests turn it off for a quiet ship.
    pub asteroids: bool,
}

impl Default for SimPlugin {
    fn default() -> Self {
        Self { asteroids: true }
    }
}

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<tuning::Tuning>() {
            let tuning = tuning::Tuning::load_or_default(std::path::Path::new(tuning::TUNING_PATH));
            app.insert_resource(tuning);
        }
        if self.asteroids {
            app.add_systems(
                Startup,
                asteroids::spawn_field
                    .after(ship::spawn_ship)
                    .before(player::spawn_player),
            )
            .add_systems(
                FixedUpdate,
                (
                    asteroids::apply_field_speed,
                    asteroids::recycle_field.after(update_pins),
                )
                    .in_set(SimSystems::Drive),
            );
        }
        app.insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
            .insert_resource(Gravity(Vec3::ZERO))
            .add_systems(Startup, (ship::spawn_ship, player::spawn_player).chain())
            .configure_sets(
                FixedUpdate,
                (SimSystems::RecordVelocities, SimSystems::Drive).chain(),
            )
            .add_systems(
                FixedUpdate,
                (
                    (ship::drive_ship, update_pins).in_set(SimSystems::Drive),
                    bodies::record_previous_velocity.in_set(SimSystems::RecordVelocities),
                ),
            )
            .add_systems(
                FixedPostUpdate,
                player::player_tick.after(PhysicsSystems::Last),
            );
    }
}

/// Keeps rocks the player is standing on or grappled to from being recycled.
fn update_pins(
    mut commands: Commands,
    players: Query<&player::Player>,
    pinned: Query<Entity, With<asteroids::Pinned>>,
    rocks: Query<(), With<asteroids::Asteroid>>,
) {
    let wanted: Vec<Entity> = players
        .iter()
        .flat_map(|p| [p.mode.body(), p.grapple.map(|g| g.body)])
        .flatten()
        .filter(|e| rocks.contains(*e))
        .collect();
    for e in &pinned {
        if !wanted.contains(&e) {
            commands.entity(e).remove::<asteroids::Pinned>();
        }
    }
    for e in wanted {
        if !pinned.contains(e) {
            commands.entity(e).insert(asteroids::Pinned);
        }
    }
}
