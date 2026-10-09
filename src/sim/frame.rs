//! Reference-frame math.
//!
//! Every rigid body (ship, asteroid) defines a local frame. A mag-booted player
//! is simulated in the coordinates of the body it stands on; a free-flying
//! player is simulated in world (inertial) coordinates. All conversions between
//! the two go through [`BodyFrame`], so momentum is carried correctly across
//! every transition.

use bevy::prelude::*;

/// Kinematic snapshot of a rigid body at one simulation tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyFrame {
    /// Body origin in world space (Avian `Position`).
    pub position: Vec3,
    /// Body orientation (Avian `Rotation`).
    pub rotation: Quat,
    /// World-space center of mass. Linear velocity is measured here.
    pub center_of_mass: Vec3,
    /// Linear velocity of the center of mass.
    pub linear_velocity: Vec3,
    /// World-space angular velocity.
    pub angular_velocity: Vec3,
}

impl BodyFrame {
    pub const IDENTITY: Self = Self {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        center_of_mass: Vec3::ZERO,
        linear_velocity: Vec3::ZERO,
        angular_velocity: Vec3::ZERO,
    };

    pub fn point_to_world(&self, local: Vec3) -> Vec3 {
        self.position + self.rotation * local
    }

    pub fn point_to_local(&self, world: Vec3) -> Vec3 {
        self.rotation.inverse() * (world - self.position)
    }

    pub fn dir_to_world(&self, local: Vec3) -> Vec3 {
        self.rotation * local
    }

    pub fn dir_to_local(&self, world: Vec3) -> Vec3 {
        self.rotation.inverse() * world
    }

    /// World-space velocity of the material point of the body at `world_point`.
    pub fn point_velocity(&self, world_point: Vec3) -> Vec3 {
        self.linear_velocity
            + self
                .angular_velocity
                .cross(world_point - self.center_of_mass)
    }

    /// World velocity of something at `local_point` moving at `local_velocity`
    /// relative to the body (both in body coordinates).
    pub fn velocity_to_world(&self, local_point: Vec3, local_velocity: Vec3) -> Vec3 {
        self.point_velocity(self.point_to_world(local_point)) + self.dir_to_world(local_velocity)
    }

    /// Inverse of [`Self::velocity_to_world`].
    pub fn velocity_to_local(&self, world_point: Vec3, world_velocity: Vec3) -> Vec3 {
        self.dir_to_local(world_velocity - self.point_velocity(world_point))
    }

    /// World-space acceleration a material point at `world_point` undergoes,
    /// given this frame and the frame one tick earlier. This is the
    /// acceleration a boot must supply to keep a player stuck to the surface:
    /// a_cm + α × r + ω × (ω × r).
    pub fn point_acceleration(&self, previous: &BodyFrame, dt: f32, world_point: Vec3) -> Vec3 {
        let r = world_point - self.center_of_mass;
        let a_cm = (self.linear_velocity - previous.linear_velocity) / dt;
        let alpha = (self.angular_velocity - previous.angular_velocity) / dt;
        let w = self.angular_velocity;
        a_cm + alpha.cross(r) + w.cross(w.cross(r))
    }
}

/// Mass properties needed to resolve impulses against a body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyMass {
    pub inverse_mass: f32,
    /// World-space inverse inertia tensor.
    pub inverse_inertia: Mat3,
}

impl BodyMass {
    /// Kinematic or static: unmovable by impulses.
    pub const INFINITE: Self = Self {
        inverse_mass: 0.0,
        inverse_inertia: Mat3::ZERO,
    };

    pub fn is_infinite(&self) -> bool {
        self.inverse_mass == 0.0
    }

    /// Inverse of the effective mass felt by an impulse along `dir` applied at
    /// `world_point`: 1/m + (r × n) · I⁻¹ (r × n).
    pub fn inverse_effective_mass(&self, frame: &BodyFrame, world_point: Vec3, dir: Vec3) -> f32 {
        let rn = (world_point - frame.center_of_mass).cross(dir);
        self.inverse_mass + rn.dot(self.inverse_inertia * rn)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spinning_frame() -> BodyFrame {
        BodyFrame {
            position: Vec3::new(10.0, -3.0, 5.0),
            rotation: Quat::from_euler(EulerRot::XYZ, 0.3, 1.1, -0.4),
            center_of_mass: Vec3::new(10.5, -3.0, 4.0),
            linear_velocity: Vec3::new(2.0, 0.5, -15.0),
            angular_velocity: Vec3::new(0.1, -0.4, 0.25),
        }
    }

    #[test]
    fn point_round_trip() {
        let f = spinning_frame();
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert!(f.point_to_local(f.point_to_world(p)).abs_diff_eq(p, 1e-4));
    }

    #[test]
    fn velocity_round_trip_carries_momentum() {
        // A booted player standing still on a spinning, moving body must
        // leave it with exactly the velocity of the surface point under it.
        let f = spinning_frame();
        let feet = Vec3::new(0.0, 4.0, -2.0);
        let world_v = f.velocity_to_world(feet, Vec3::ZERO);
        let expected = f.point_velocity(f.point_to_world(feet));
        assert!(world_v.abs_diff_eq(expected, 1e-5));
        // And relative velocity survives the round trip.
        let v_local = Vec3::new(0.3, 3.0, -1.0);
        let w = f.velocity_to_world(feet, v_local);
        let back = f.velocity_to_local(f.point_to_world(feet), w);
        assert!(back.abs_diff_eq(v_local, 1e-4));
    }

    #[test]
    fn centripetal_acceleration_points_inward() {
        let mut f = BodyFrame::IDENTITY;
        f.angular_velocity = Vec3::Y * 2.0;
        let a = f.point_acceleration(&f, 1.0 / 60.0, Vec3::X * 3.0);
        // |a| = ω²r = 12, directed toward the axis.
        assert!(a.abs_diff_eq(Vec3::NEG_X * 12.0, 1e-4));
    }

    #[test]
    fn effective_mass_off_center_is_lighter() {
        let f = BodyFrame::IDENTITY;
        let m = BodyMass {
            inverse_mass: 0.5,
            inverse_inertia: Mat3::from_diagonal(Vec3::splat(0.25)),
        };
        let center = m.inverse_effective_mass(&f, Vec3::ZERO, Vec3::X);
        let lever = m.inverse_effective_mass(&f, Vec3::Y * 2.0, Vec3::X);
        assert!((center - 0.5).abs() < 1e-6);
        assert!((lever - (0.5 + 4.0 * 0.25)).abs() < 1e-6);
    }
}
