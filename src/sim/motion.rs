//! Free flight, described once and evaluated anywhere.
//!
//! A body in free flight (no thrust, no contact) moves its center of mass in
//! a straight line and spins at a constant rate (bodies whose spin we want
//! predictable have isotropic inertia; see [`super::bodies::dynamic_mass`]).
//! [`Motion`] records that flight at one tick, and [`Motion::at`] gives the
//! body's pose at any other: earlier or later, between ticks for rendering.
//! A network sends a body's motion only when it changes, rather than its pose
//! every tick, and a client sees the body where it is *now*, not where it
//! was a round trip ago.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::TICK_HZ;
use super::frame::BodyFrame;

#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Motion {
    /// The tick this state was sampled on.
    pub tick: u32,
    /// World-space center of mass.
    pub center_of_mass: Vec3,
    pub rotation: Quat,
    /// Velocity of the center of mass.
    pub linear: Vec3,
    pub angular: Vec3,
}

/// A body's pose and velocities at some time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: Quat,
}

impl Motion {
    /// Records the flight of a body in `frame` at `tick`.
    pub fn sample(tick: u32, frame: &BodyFrame) -> Self {
        Self {
            tick,
            center_of_mass: frame.center_of_mass,
            rotation: frame.rotation,
            linear: frame.linear_velocity,
            angular: frame.angular_velocity,
        }
    }

    /// The pose at `tick` plus a fraction of a tick, for a body whose center
    /// of mass is `local_com` in body coordinates.
    pub fn at(&self, tick: u32, fraction: f32, local_com: Vec3) -> Pose {
        let ticks = tick.wrapping_sub(self.tick) as i32 as f32 + fraction;
        let t = ticks / TICK_HZ as f32;
        let rotation = (Quat::from_scaled_axis(self.angular * t) * self.rotation).normalize();
        Pose {
            position: self.center_of_mass + self.linear * t - rotation * local_com,
            rotation,
        }
    }

    /// The body's frame at `tick`.
    pub fn frame_at(&self, tick: u32, local_com: Vec3) -> BodyFrame {
        let pose = self.at(tick, 0.0, local_com);
        BodyFrame {
            position: pose.position,
            rotation: pose.rotation,
            center_of_mass: pose.position + pose.rotation * local_com,
            linear_velocity: self.linear,
            angular_velocity: self.angular,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spins_about_the_center_of_mass() {
        let com = Vec3::new(1.0, 0.0, 0.0);
        let motion = Motion {
            tick: 100,
            center_of_mass: Vec3::new(1.0, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            linear: Vec3::ZERO,
            // Half a turn per second about Y.
            angular: Vec3::Y * std::f32::consts::PI,
        };
        let pose = motion.at(100 + TICK_HZ as u32, 0.0, com);
        // The center of mass stays put; the origin swings around it.
        assert!((pose.position + pose.rotation * com - Vec3::X).length() < 1e-4);
        assert!((pose.position - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4);
    }

    #[test]
    fn evaluates_backwards_and_between_ticks() {
        let motion = Motion {
            tick: 10,
            center_of_mass: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            linear: Vec3::X * TICK_HZ as f32,
            angular: Vec3::ZERO,
        };
        assert!((motion.at(9, 0.0, Vec3::ZERO).position.x + 1.0).abs() < 1e-4);
        assert!((motion.at(10, 0.5, Vec3::ZERO).position.x - 0.5).abs() < 1e-4);
    }
}
