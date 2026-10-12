//! Ships: hulls built to their designs (see [`super::design`]), the controls
//! and turrets fitted to them, and the crew ship's drift through the
//! asteroid field.
//!
//! In the field the world frame is chosen so the crew's ship is (on average)
//! at rest and the asteroid field streams past it. Physics is
//! Galilean-invariant, so this is identical to the ship flying through a
//! static field, but keeps every coordinate near the origin and keeps
//! relative velocities near the ship small for collision sweeps. Ships in a
//! fight fly scripted paths instead (see [`super::flight`]).

use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bodies::{BodyInertia, PreviousVelocity};
use super::design::{ShipDesign, design};
use super::mission::ScenarioKind;
use super::tuning::Tuning;
use super::{SimTick, TICK_HZ};

/// The crew's ship: where players come aboard, and where the mission wall is.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HomeShip;

/// A ship drifting with the field: a slow roll, a gentle yaw wobble, and an
/// optional forward lurch.
#[derive(Component, Default)]
pub struct Drift {
    /// Tick the current forward lurch started on.
    lurch_start: Option<u32>,
}

/// Orders for drifting ships, from whoever is allowed to give them. Consumed
/// by the authoritative simulation only: a ship is not something a client
/// predicts, so these never need replaying.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum ShipCommand {
    /// A one-second forward lurch.
    Lurch,
}

const LURCH_SECONDS: f32 = 1.0;
const WOBBLE_SECONDS: u32 = 20;

/// Ship-local forward axis.
pub const SHIP_FORWARD: Vec3 = Vec3::NEG_Z;

/// Something on a ship a player can press (by clicking it, within reach).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlAction {
    /// Leave whatever is going on and start this scenario.
    Scenario(ScenarioKind),
    /// Lurch the ship forward.
    Lurch,
    /// Drop this ship's shield.
    DisableShields,
    /// Take a full set of guns and ammo.
    Armory,
}

impl ControlAction {
    /// What the button says.
    pub fn label(self) -> &'static str {
        match self {
            ControlAction::Scenario(kind) => kind.button_label(),
            ControlAction::Lurch => "Lurch (DO NOT PRESS)",
            ControlAction::DisableShields => "Disable shields",
            ControlAction::Armory => "Weapons locker",
        }
    }
}

/// A button fitted to a ship: an action, and the box (centered on the
/// entity's transform, in ship coordinates) a player clicks on.
#[derive(Component, Clone, Copy, Debug)]
pub struct Control {
    pub action: ControlAction,
    pub half_size: Vec3,
}

/// A laser turret fitted to a ship (see [`super::combat`]).
#[derive(Component, Clone, Copy, Debug)]
pub struct Turret {
    /// Which of the ship's turrets: sets its place in the firing cycle.
    pub index: u8,
}

/// Visual-only hull piece; the collision shape is the ship's compound collider.
#[derive(Component, Clone, Copy, Debug)]
pub struct HullPart {
    pub size: Vec3,
}

/// The ship's structure: which design it's built to (see
/// [`super::design`]). Its geometry is built from the design wherever the
/// component appears (see [`build_hull`]), so replicating this is
/// replicating the hull.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShipHull(pub String);

impl ShipHull {
    pub fn new(id: &str) -> ShipHull {
        ShipHull(id.to_string())
    }

    pub fn design(&self) -> &'static ShipDesign {
        design(&self.0)
    }
}

/// The crew's ship's design.
pub const HOME: &str = "home";

/// The crew's ship, drifting with the field at the origin.
pub fn drifting_home_ship() -> impl Bundle {
    (
        Name::new("Ship"),
        HomeShip,
        Drift::default(),
        ShipHull::new(HOME),
        RigidBody::Kinematic,
        SleepingDisabled,
        PreviousVelocity::default(),
        Position::default(),
        Rotation::default(),
    )
}

/// Gives a new [`ShipHull`] its collider and visual parts.
///
/// The ship's collision is one compound collider on the body entity itself.
/// Avian only re-syncs *child* collider poses at the start of each physics
/// step, so child colliders would be queried a tick stale (v·dt off) by the
/// player controller, which runs after the step.
pub fn build_hull(add: On<Add, ShipHull>, hulls: Query<&ShipHull>, mut commands: Commands) {
    let ship = hulls.get(add.entity).expect("just added").design();
    let collider = Collider::compound(
        ship.boxes
            .iter()
            .map(|b| (b.center, b.rotation, Collider::cuboid(b.size.x, b.size.y, b.size.z)))
            .collect(),
    );
    commands
        .entity(add.entity)
        .insert((BodyInertia::immovable(&collider), collider, Visibility::default()))
        // A replicated hull may not have its pose yet.
        .insert_if_new(Transform::default())
        .with_children(|parent| {
            for (i, b) in ship.boxes.iter().enumerate() {
                parent.spawn((
                    Name::new(format!("Hull {i}")),
                    HullPart { size: b.size },
                    Transform::from_translation(b.center).with_rotation(b.rotation),
                    b.material,
                ));
            }
            for &(action, center, half_size) in &ship.controls {
                parent.spawn((
                    Name::new(action.label()),
                    Control { action, half_size },
                    Transform::from_translation(center),
                ));
            }
            for (index, &mount) in ship.turrets.iter().enumerate() {
                parent.spawn((
                    Name::new(format!("Turret {index}")),
                    Turret { index: index as u8 },
                    Transform::from_translation(mount),
                ));
            }
        });
}

/// Drives drifting ships: a slow roll, a gentle yaw wobble, and an
/// optional forward lurch limited by `ship_max_accel`.
pub fn drive_ship(
    time: Res<Time>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut orders: MessageReader<ShipCommand>,
    mut ships: Query<(
        &mut Drift,
        &Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    let dt = time.delta_secs();
    let lurch = orders.read().any(|o| *o == ShipCommand::Lurch);
    // Wobble phase from the tick, exact however long the game runs.
    let wobble_ticks = WOBBLE_SECONDS * TICK_HZ as u32;
    let phase = (tick.0 % wobble_ticks) as f32 / wobble_ticks as f32 * std::f32::consts::TAU;
    for (mut ship, rot, mut lin, mut ang) in &mut ships {
        if lurch {
            ship.lurch_start = Some(tick.0);
        }
        let wobble_freq = std::f32::consts::TAU / WOBBLE_SECONDS as f32;
        let yaw_rate = tuning.ship_yaw_wobble_deg.to_radians() * wobble_freq * phase.cos();
        let roll_axis = rot.0 * SHIP_FORWARD;
        ang.0 = roll_axis * tuning.ship_roll_rate_deg.to_radians() + rot.0 * Vec3::Y * yaw_rate;

        let lurching = ship.lurch_start.is_some_and(|start| {
            (tick.0.wrapping_sub(start) as f32 / TICK_HZ as f32) < LURCH_SECONDS
        });
        let target = if lurching {
            rot.0 * SHIP_FORWARD * tuning.lurch_delta_v
        } else {
            Vec3::ZERO
        };
        let max_dv = tuning.ship_max_accel * dt;
        let dv = (target - lin.0).clamp_length_max(max_dv);
        lin.0 += dv;
    }
}
