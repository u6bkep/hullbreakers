//! Ships: greybox hulls, the controls and turrets fitted to them, and the
//! crew ship's drift through the asteroid field.
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
use super::mission::ScenarioKind;
use super::tuning::Tuning;
use super::{SimMaterial, SimTick, TICK_HZ};

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

/// Which hull a ship has. Its geometry, controls and turrets all follow
/// from this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HullKind {
    /// The crew's ship.
    #[default]
    Home,
    /// The enemy in a boarding action: the same frame, hostile paint, and a
    /// shield console on its bridge.
    Raider,
}

/// One structural box of the hull: center and full size, in ship coordinates.
struct HullBox {
    center: Vec3,
    size: Vec3,
    material: SimMaterial,
}

const fn hull(center: Vec3, size: Vec3) -> HullBox {
    HullBox {
        center,
        size,
        material: SimMaterial::Hull,
    }
}

const fn trim(center: Vec3, size: Vec3) -> HullBox {
    HullBox {
        center,
        size,
        material: SimMaterial::Trim,
    }
}

/// Greybox layout. Interior deck surface is y = 0, ceiling y = 3, interior
/// half-width 3.7, hull runs z = -15 (bow) to z = +15 (stern, open cargo door).
fn hull_layout(kind: HullKind) -> Vec<HullBox> {
    let t = 0.3;
    let v = Vec3::new;
    let mut boxes = vec![
        // Floor, ceiling, side walls.
        hull(v(0.0, -t / 2.0, 0.0), v(8.0, t, 30.0)),
        hull(v(0.0, 3.0 + t / 2.0, 0.0), v(8.0, t, 30.0)),
        hull(v(-3.85, 1.5, 0.0), v(t, 3.0, 30.0)),
        hull(v(3.85, 1.5, 0.0), v(t, 3.0, 30.0)),
        // Bow wall with a window slot between y = 1.2 and y = 2.0.
        hull(v(0.0, 0.6, -14.85), v(7.4, 1.2, t)),
        hull(v(0.0, 2.5, -14.85), v(7.4, 1.0, t)),
        // Stern: open cargo door, 3 m wide by 2.4 m high.
        hull(v(-2.6, 1.5, 14.85), v(2.2, 3.0, t)),
        hull(v(2.6, 1.5, 14.85), v(2.2, 3.0, t)),
        hull(v(0.0, 2.7, 14.85), v(3.0, 0.6, t)),
        // Mid bulkhead with a doorway 1.2 m wide by 2.2 m high.
        hull(v(-2.15, 1.5, 0.0), v(3.1, 3.0, t)),
        hull(v(2.15, 1.5, 0.0), v(3.1, 3.0, t)),
        hull(v(0.0, 2.6, 0.0), v(1.2, 0.8, t)),
        // Cargo crates in the aft hold.
        trim(v(-2.6, 0.6, 9.0), v(1.2, 1.2, 1.2)),
        trim(v(-2.6, 0.5, 10.4), v(1.0, 1.0, 1.4)),
        trim(v(2.4, 0.9, 6.0), v(1.6, 1.8, 1.6)),
        // Bridge console.
        trim(v(0.0, 0.5, -13.0), v(4.0, 1.0, 0.8)),
        // Dorsal spine: a raised walkway along the top.
        trim(v(0.0, 3.3 + 0.4, 2.0), v(2.0, 0.8, 18.0)),
        // Antenna mast.
        trim(v(0.0, 3.3 + 2.0, -9.0), v(0.3, 4.0, 0.3)),
        // Engine nacelles on pylons.
        hull(v(-5.4, 1.5, 11.0), v(1.6, 1.6, 8.0)),
        hull(v(5.4, 1.5, 11.0), v(1.6, 1.6, 8.0)),
        trim(v(-4.3, 1.5, 11.0), v(0.6, 0.4, 3.0)),
        trim(v(4.3, 1.5, 11.0), v(0.6, 0.4, 3.0)),
    ];
    // Turret bases, where the turrets below are mounted.
    for mount in TURRET_MOUNTS {
        boxes.push(trim(mount - Vec3::Y * mount.y.signum() * 0.25, v(1.0, 0.5, 1.0)));
    }
    match kind {
        HullKind::Home => {
            // The mission wall: a panel on the port side of the bridge.
            boxes.push(trim(v(-3.65, 1.5, -7.0), v(0.1, 1.4, 4.4)));
        }
        HullKind::Raider => {
            for b in &mut boxes {
                if b.material == SimMaterial::Hull {
                    b.material = SimMaterial::HostileHull;
                }
            }
        }
    }
    boxes
}

/// The boxes a hull is built from: center, full size, and paint.
pub fn hull_pieces(kind: HullKind) -> Vec<(Vec3, Vec3, SimMaterial)> {
    hull_layout(kind)
        .into_iter()
        .map(|b| (b.center, b.size, b.material))
        .collect()
}

/// Where turrets sit, in ship coordinates: one on the dorsal spine, one
/// under the keel. A turret's beam leaves from its mount.
pub const TURRET_MOUNTS: [Vec3; 2] = [Vec3::new(0.0, 4.6, -4.0), Vec3::new(0.0, -0.8, -6.0)];

/// Something on a ship a player can press (by clicking it, within reach).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlAction {
    /// Leave whatever is going on and start this scenario.
    Scenario(ScenarioKind),
    /// Lurch the ship forward.
    Lurch,
    /// Drop this ship's shield.
    DisableShields,
}

impl ControlAction {
    /// What the button says.
    pub fn label(self) -> &'static str {
        match self {
            ControlAction::Scenario(kind) => kind.button_label(),
            ControlAction::Lurch => "Lurch (DO NOT PRESS)",
            ControlAction::DisableShields => "Disable shields",
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

/// The controls fitted to each kind of hull: (action, center, half size).
fn controls(kind: HullKind) -> Vec<(ControlAction, Vec3, Vec3)> {
    let v = Vec3::new;
    match kind {
        HullKind::Home => {
            // Buttons on the mission wall, sticking out of the panel.
            let button = v(0.06, 0.28, 0.4);
            let mut buttons: Vec<_> = ScenarioKind::ALL
                .iter()
                .enumerate()
                .map(|(i, kind)| {
                    (
                        ControlAction::Scenario(*kind),
                        v(-3.54, 1.75, -8.4 + 1.1 * i as f32),
                        button,
                    )
                })
                .collect();
            // Lower, and apart from the rest.
            buttons.push((ControlAction::Lurch, v(-3.54, 1.05, -5.4), v(0.06, 0.18, 0.25)));
            buttons
        }
        // A big button on top of the bridge console.
        HullKind::Raider => vec![(ControlAction::DisableShields, v(0.0, 1.06, -13.0), v(0.35, 0.06, 0.3))],
    }
}

/// Visual-only hull piece; the collision shape is the ship's compound collider.
#[derive(Component, Clone, Copy, Debug)]
pub struct HullPart {
    pub size: Vec3,
}

/// The ship's structure. Its geometry is built from the layout wherever the
/// component appears (see [`build_hull`]), so replicating this is replicating
/// the hull.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShipHull(pub HullKind);

/// The crew's ship, drifting with the field at the origin.
pub fn drifting_home_ship() -> impl Bundle {
    (
        Name::new("Ship"),
        HomeShip,
        Drift::default(),
        ShipHull(HullKind::Home),
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
    let kind = hulls.get(add.entity).expect("just added").0;
    let layout = hull_layout(kind);
    let collider = Collider::compound(
        layout
            .iter()
            .map(|b| {
                (
                    b.center,
                    Quat::IDENTITY,
                    Collider::cuboid(b.size.x, b.size.y, b.size.z),
                )
            })
            .collect(),
    );
    commands
        .entity(add.entity)
        .insert((BodyInertia::immovable(&collider), collider, Visibility::default()))
        // A replicated hull may not have its pose yet.
        .insert_if_new(Transform::default())
        .with_children(|ship| {
            for (i, b) in layout.into_iter().enumerate() {
                ship.spawn((
                    Name::new(format!("Hull {i}")),
                    HullPart { size: b.size },
                    Transform::from_translation(b.center),
                    b.material,
                ));
            }
            for (action, center, half_size) in controls(kind) {
                ship.spawn((
                    Name::new(action.label()),
                    Control { action, half_size },
                    Transform::from_translation(center),
                ));
            }
            for (index, mount) in TURRET_MOUNTS.into_iter().enumerate() {
                ship.spawn((
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
