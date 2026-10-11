//! Scripted flight: a body whose pose is a pure function of the tick.
//!
//! Ships in a fight fly set paths. Every peer evaluates the same [`Flight`]
//! at the same tick and gets the same pose, so a manoeuvring ship costs
//! nothing to network (its flight is sent once) and is exactly where the
//! authority has it, on every client, at every tick. Compare free bodies,
//! which are sent as [`super::motion::Motion`] whenever they're disturbed.
//!
//! A flying body is a static rigid body moved each tick (before the physics
//! step, so collision queries see it where it is) with the velocities of its
//! path, so whatever stands on it or measures against it sees it moving.

use std::f64::consts::TAU;

use avian3d::prelude::*;
use bevy::math::{DQuat, DVec3};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bodies::{BodyInertia, PreviousVelocity};
use super::combat;
use super::frame::BodyFrame;
use super::{SimTick, TICK_HZ};

/// A scripted flight, started on a tick.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Flight {
    pub start: u32,
    pub path: Path,
    /// Whether the ship shudders when the opposing turrets' beams hit it.
    pub shaken: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Path {
    /// The crew's ship in a duel: a wide, banked orbit with a gentle climb
    /// and dive.
    Orbit(Duel),
    /// Its opponent, weaving in close and back out around it.
    Weave(Duel),
}

/// The shape of a duel between two ships.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Duel {
    pub center: Vec3,
    /// Orbit radius (m) and time for one lap (s).
    pub radius: f32,
    pub lap: f32,
    /// Height of the orbit's climb and dive (m).
    pub bob: f32,
    /// Closest and farthest the opponent gets (m), and the time from one
    /// close pass to the next (s).
    pub near: f32,
    pub far: f32,
    pub pass: f32,
    /// Time for the opponent to circle once around the crew's ship (s).
    pub sweep: f32,
}

/// Banking: ships tilt into their turns as if their crews felt this much
/// gravity (m/s²) pulling toward the floor. Lower banks harder.
const BANK_GRAVITY: f64 = 4.0;
/// Step for differentiating a path (s).
const DIFF_STEP: f64 = 0.25;

impl Duel {
    fn orbit_position(&self, t: f64) -> DVec3 {
        let theta = TAU * t / self.lap as f64;
        let r = self.radius as f64;
        self.center.as_dvec3()
            + DVec3::new(r * theta.cos(), self.bob as f64 * (2.0 * theta).sin(), r * theta.sin())
    }

    fn weave_position(&self, t: f64) -> DVec3 {
        let (near, far) = (self.near as f64, self.far as f64);
        // Starts far out; the first close pass is half a pass in.
        let distance = (near + far) / 2.0 + (far - near) / 2.0 * (TAU * t / self.pass as f64).cos();
        let azimuth = TAU * t / self.sweep as f64;
        let elevation = 0.35 * (TAU * t / (self.sweep as f64 * 0.7)).sin();
        let direction = DVec3::new(
            azimuth.cos() * elevation.cos(),
            elevation.sin(),
            azimuth.sin() * elevation.cos(),
        );
        self.orbit_position(t) + direction * distance
    }
}

impl Path {
    fn position(&self, t: f64) -> DVec3 {
        match self {
            Path::Orbit(duel) => duel.orbit_position(t),
            Path::Weave(duel) => duel.weave_position(t),
        }
    }

    /// Pose at `t` seconds into the flight: nose along the velocity, banked
    /// into the turn.
    fn pose(&self, t: f64) -> (DVec3, DQuat) {
        let h = DIFF_STEP;
        let (before, here, after) = (self.position(t - h), self.position(t), self.position(t + h));
        let velocity = (after - before) / (2.0 * h);
        let acceleration = (after - 2.0 * here + before) / (h * h);
        let forward = velocity.normalize_or(DVec3::NEG_Z);
        let lateral = acceleration - forward * acceleration.dot(forward);
        let up = (DVec3::Y * BANK_GRAVITY + lateral).normalize();
        let right = forward.cross(up).normalize();
        let up = right.cross(forward);
        // Ship forward is -Z.
        let rotation = DQuat::from_mat3(&bevy::math::DMat3::from_cols(right, up, -forward));
        (here, rotation)
    }
}

/// How hard a hit shakes a ship: peak angle (rad), decay time (s), and
/// frequency (Hz). Gentle: the shake's acceleration at the ends of the
/// hull (angle × (2π·f)² × 15 m, about 2 m/s²) must stay well inside what
/// boots hold.
const SHAKE_ANGLE: f64 = 0.0015;
const SHAKE_DECAY: f64 = 0.4;
const SHAKE_HZ: f64 = 1.5;

impl Flight {
    fn seconds(&self, tick: u32, fraction: f32) -> f64 {
        (tick.wrapping_sub(self.start) as i32 as f64 + fraction as f64) / TICK_HZ
    }

    /// Where the body's origin is and how it's turned, at `tick` plus a
    /// fraction of one.
    pub fn pose(&self, tick: u32, fraction: f32) -> (Vec3, Quat) {
        let (position, mut rotation) = self.path.pose(self.seconds(tick, fraction));
        if self.shaken {
            rotation *= self.shake(tick, fraction);
        }
        (position.as_vec3(), rotation.as_quat())
    }

    /// The shudder from recent beam hits: each one a decaying wobble, about
    /// an axis that depends on which turret fired.
    fn shake(&self, tick: u32, fraction: f32) -> DQuat {
        let mut shake = DQuat::IDENTITY;
        for turret in 0..combat::TURRETS {
            let Some(hit) = combat::beam_start(turret, tick) else {
                continue;
            };
            let since = (tick.wrapping_sub(hit) as f64 + fraction as f64) / TICK_HZ;
            if since > 10.0 * SHAKE_DECAY {
                continue;
            }
            // Swelling from rest to its peak at SHAKE_DECAY, then dying
            // away: starting at full swing would jolt the ends of a long
            // hull hard enough to tear boots off.
            let x = since / SHAKE_DECAY;
            let angle = SHAKE_ANGLE * x * (1.0 - x).exp() * (TAU * SHAKE_HZ * since).sin();
            let axis = if turret % 2 == 0 { DVec3::X } else { DVec3::Z };
            shake *= DQuat::from_axis_angle(axis, angle);
        }
        shake
    }

    /// The body's frame at `tick`: pose, and velocities across the tick
    /// that brought it there, measured at its center of mass.
    pub fn frame(&self, tick: u32, local_com: Vec3) -> BodyFrame {
        let (position, rotation) = self.pose(tick, 0.0);
        let (was_position, was_rotation) = self.pose(tick.wrapping_sub(1), 0.0);
        let center_of_mass = position + rotation * local_com;
        let was_center = was_position + was_rotation * local_com;
        let mut turn = (rotation * was_rotation.inverse()).normalize();
        // The short way round: q and −q are the same turn, but −q's axis
        // and angle read as nearly a whole revolution the other way.
        if turn.w < 0.0 {
            turn = -turn;
        }
        BodyFrame {
            position,
            rotation,
            center_of_mass,
            linear_velocity: (center_of_mass - was_center) * TICK_HZ as f32,
            angular_velocity: turn.to_scaled_axis() * TICK_HZ as f32,
        }
    }
}

/// Every peer, each tick: poses each flying body for this tick, before the
/// physics step. Its velocities last tick are known exactly too, and set
/// with it: recorded from the body, they'd be wrong on its first tick (a
/// new ship read as lurching to full speed, throwing off whoever stands on
/// it).
pub fn fly(
    tick: Res<SimTick>,
    mut bodies: Query<(
        &Flight,
        &BodyInertia,
        &mut Position,
        &mut Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
        Option<&mut PreviousVelocity>,
    )>,
) {
    for (flight, inertia, mut pos, mut rot, mut lin, mut ang, previous) in &mut bodies {
        if let Some(mut previous) = previous {
            let was = flight.frame(tick.0.wrapping_sub(1), inertia.center_of_mass);
            previous.linear = was.linear_velocity;
            previous.angular = was.angular_velocity;
        }
        let frame = flight.frame(tick.0, inertia.center_of_mass);
        pos.0 = frame.position;
        rot.0 = frame.rotation;
        lin.0 = frame.linear_velocity;
        ang.0 = frame.angular_velocity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn duel() -> Duel {
        Duel {
            center: Vec3::ZERO,
            radius: 250.0,
            lap: 100.0,
            bob: 15.0,
            near: 40.0,
            far: 200.0,
            pass: 30.0,
            sweep: 90.0,
        }
    }

    #[test]
    fn velocities_carry_the_body_from_tick_to_tick() {
        let flight = Flight {
            start: 1000,
            path: Path::Weave(duel()),
            shaken: true,
        };
        let com = Vec3::new(0.0, 1.0, 2.0);
        for tick in [1000, 1500, 4321] {
            let now = flight.frame(tick, com);
            let next = flight.frame(tick + 1, com);
            let dt = 1.0 / TICK_HZ as f32;
            let moved = now.center_of_mass + next.linear_velocity * dt;
            assert!(moved.distance(next.center_of_mass) < 1e-3);
            let turned = Quat::from_scaled_axis(next.angular_velocity * dt) * now.rotation;
            assert!(super::super::motion::rotation_error(turned, next.rotation) < 1e-5);
        }
    }

    #[test]
    fn the_opponent_passes_close_and_slowly() {
        let duel = duel();
        let (home, raider) = (Path::Orbit(duel), Path::Weave(duel));
        let at = |t: f64| home.position(t).distance(raider.position(t));
        assert!((at(0.0) - 200.0).abs() < 1e-6);
        let pass = duel.pass as f64 / 2.0;
        assert!((at(pass) - 40.0).abs() < 1e-6);
        // At the close pass the ships drift past each other, not race.
        let h = 0.01;
        let relative = |t: f64| raider.position(t) - home.position(t);
        let speed = (relative(pass + h) - relative(pass - h)).length() / (2.0 * h);
        assert!(speed < 6.0, "passing at {speed} m/s");
    }

    #[test]
    fn the_ends_of_a_long_ship_ride_smoothly() {
        // A carrier's weave, hit by beams throughout (shaken), sampled for
        // five minutes at its bow, 25 m from the middle: what mag-boots
        // there must hold against.
        let duel = Duel {
            pass: 60.0,
            sweep: 240.0,
            near: 100.0,
            far: 400.0,
            radius: 300.0,
            lap: 120.0,
            ..duel()
        };
        let flight = Flight {
            start: 0,
            path: Path::Weave(duel),
            shaken: true,
        };
        let bow = Vec3::new(0.0, 3.3, -25.0);
        let dt = 1.0 / TICK_HZ as f32;
        let worst = (1..60 * 300)
            .map(|tick| {
                let (a, b) = (flight.frame(tick, Vec3::ZERO), flight.frame(tick + 1, Vec3::ZERO));
                let before = a.point_velocity(a.point_to_world(bow));
                let after = b.point_velocity(b.point_to_world(bow));
                (after - before).length() / dt
            })
            .fold(0.0, f32::max);
        // Within what the crew's boots hold, with a margin.
        let grip = crate::sim::tuning::Tuning::default().adhesion_accel;
        assert!(worst < 0.9 * grip, "the bow feels up to {worst} m/s² (boots hold {grip})");
    }
}
