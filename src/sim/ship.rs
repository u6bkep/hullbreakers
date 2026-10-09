//! The player ship: a kinematic greybox hull on a scripted flight path.
//!
//! The world frame is chosen so the ship is (on average) at rest and the
//! asteroid field streams past it. Physics is Galilean-invariant, so this is
//! identical to the ship flying through a static field, but keeps every
//! coordinate near the origin and keeps relative velocities near the ship small
//! for collision sweeps.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::SimMaterial;
use super::bodies::PreviousVelocity;
use super::tuning::Tuning;

#[derive(Component, Default)]
pub struct Ship {
    /// Seconds of forward lurch remaining.
    pub lurch_remaining: f32,
    elapsed: f32,
}

impl Ship {
    pub fn lurch(&mut self) {
        self.lurch_remaining = 1.0;
    }
}

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

pub fn spawn_ship(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Ship"),
            Ship::default(),
            RigidBody::Kinematic,
            SleepingDisabled,
            PreviousVelocity::default(),
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|ship| {
            for (i, b) in hull_layout().into_iter().enumerate() {
                ship.spawn((
                    Name::new(format!("Hull {i}")),
                    Collider::cuboid(b.size.x, b.size.y, b.size.z),
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
    tuning: Res<Tuning>,
    mut ships: Query<(
        &mut Ship,
        &Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    let dt = time.delta_secs();
    for (mut ship, rot, mut lin, mut ang) in &mut ships {
        ship.elapsed += dt;
        let wobble_freq = std::f32::consts::TAU / 20.0;
        let yaw_rate = tuning.ship_yaw_wobble_deg.to_radians()
            * wobble_freq
            * (wobble_freq * ship.elapsed).cos();
        let roll_axis = rot.0 * SHIP_FORWARD;
        ang.0 = roll_axis * tuning.ship_roll_rate_deg.to_radians() + rot.0 * Vec3::Y * yaw_rate;

        let target = if ship.lurch_remaining > 0.0 {
            ship.lurch_remaining -= dt;
            rot.0 * SHIP_FORWARD * tuning.lurch_delta_v
        } else {
            Vec3::ZERO
        };
        let max_dv = tuning.ship_max_accel * dt;
        let dv = (target - lin.0).clamp_length_max(max_dv);
        lin.0 += dv;
    }
}
