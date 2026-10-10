//! The player ship: a kinematic greybox hull on a scripted flight path.
//!
//! The world frame is chosen so the ship is (on average) at rest and the
//! asteroid field streams past it. Physics is Galilean-invariant, so this is
//! identical to the ship flying through a static field, but keeps every
//! coordinate near the origin and keeps relative velocities near the ship small
//! for collision sweeps.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::bodies::PreviousVelocity;
use super::tuning::Tuning;
use super::{SimMaterial, SimTick, TICK_HZ};

#[derive(Component, Default)]
pub struct Ship {
    /// Tick the current forward lurch started on.
    lurch_start: Option<u32>,
}

/// Orders for the ship, from whoever is allowed to give them. Consumed by the
/// authoritative simulation only: the ship is not something a client
/// predicts, so these never need replaying.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum ShipCommand {
    /// Debug: a one-second forward lurch.
    Lurch,
}

const LURCH_SECONDS: f32 = 1.0;
const WOBBLE_SECONDS: u32 = 20;

/// Ship-local forward axis.
pub const SHIP_FORWARD: Vec3 = Vec3::NEG_Z;

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
fn hull_layout() -> Vec<HullBox> {
    let t = 0.3;
    let v = Vec3::new;
    vec![
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
    ]
}

/// Visual-only hull piece; the collision shape is the ship's compound collider.
#[derive(Component, Clone, Copy, Debug)]
pub struct HullPart {
    pub size: Vec3,
}

/// The ship's collision is one compound collider on the body entity itself.
/// Avian only re-syncs *child* collider poses at the start of each physics
/// step, so child colliders would be queried a tick stale (v·dt off) by the
/// player controller, which runs after the step.
pub fn spawn_ship(mut commands: Commands) {
    let layout = hull_layout();
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
        .spawn((
            Name::new("Ship"),
            Ship::default(),
            RigidBody::Kinematic,
            collider,
            SleepingDisabled,
            PreviousVelocity::default(),
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|ship| {
            for (i, b) in layout.into_iter().enumerate() {
                ship.spawn((
                    Name::new(format!("Hull {i}")),
                    HullPart { size: b.size },
                    Transform::from_translation(b.center),
                    b.material,
                ));
            }
        });
}

/// Drives the kinematic ship: a slow roll, a gentle yaw wobble, and an
/// optional forward lurch limited by `ship_max_accel`.
pub fn drive_ship(
    time: Res<Time>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut orders: MessageReader<ShipCommand>,
    mut ships: Query<(
        &mut Ship,
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
