//! Access to rigid bodies as reference frames, and impulse application.

use avian3d::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use super::frame::{BodyFrame, BodyMass};

/// Velocities of a body at the start of the current tick, before the physics
/// step. Used to derive frame accelerations for mag-boot adhesion.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PreviousVelocity {
    pub linear: Vec3,
    pub angular: Vec3,
}

pub fn record_previous_velocity(
    mut q: Query<(&LinearVelocity, &AngularVelocity, &mut PreviousVelocity)>,
) {
    for (lin, ang, mut prev) in &mut q {
        prev.linear = lin.0;
        prev.angular = ang.0;
    }
}

type BodyData = (
    &'static RigidBody,
    &'static Position,
    &'static Rotation,
    &'static mut LinearVelocity,
    &'static mut AngularVelocity,
    &'static ComputedMass,
    &'static ComputedAngularInertia,
    &'static ComputedCenterOfMass,
    Option<&'static PreviousVelocity>,
);

#[derive(SystemParam)]
pub struct Bodies<'w, 's> {
    bodies: Query<'w, 's, BodyData>,
    collider_of: Query<'w, 's, &'static ColliderOf>,
    shapes: Query<'w, 's, &'static Collider>,
}

impl Bodies<'_, '_> {
    /// The rigid body a collider belongs to.
    pub fn body_of_collider(&self, collider: Entity) -> Option<Entity> {
        self.collider_of.get(collider).ok().map(|c| c.body)
    }

    pub fn frame(&self, body: Entity) -> Option<BodyFrame> {
        let (_, pos, rot, lin, ang, _, _, com, _) = self.bodies.get(body).ok()?;
        Some(BodyFrame {
            position: pos.0,
            rotation: rot.0,
            center_of_mass: pos.0 + rot.0 * com.0,
            linear_velocity: lin.0,
            angular_velocity: ang.0,
        })
    }

    /// The frame as it was at the start of this tick (pose is current; only
    /// velocities differ). Enough for finite-difference accelerations.
    pub fn previous_frame(&self, body: Entity) -> Option<BodyFrame> {
        let mut frame = self.frame(body)?;
        let (.., prev) = self.bodies.get(body).ok()?;
        if let Some(prev) = prev {
            frame.linear_velocity = prev.linear;
            frame.angular_velocity = prev.angular;
        }
        Some(frame)
    }

    /// The velocity "holding still relative to `body`" means at `position`.
    ///
    /// Far from the body that is its center-of-mass velocity: a tumbling rock
    /// rotates in front of you rather than dragging you around in an orbit.
    /// Close in, it blends to the velocity of the nearest surface point, so
    /// you can touch down on a moving face. The blend runs from `far` (pure
    /// center of mass) to `near` (pure surface), measured from the surface.
    pub fn matched_velocity(
        &self,
        body: Entity,
        position: Vec3,
        near: f32,
        far: f32,
    ) -> Option<Vec3> {
        let frame = self.frame(body)?;
        let Ok(shape) = self.shapes.get(body) else {
            return Some(frame.linear_velocity);
        };
        let (surface, inside) = shape.project_point(frame.position, frame.rotation, position, true);
        let distance = if inside {
            0.0
        } else {
            surface.distance(position)
        };
        let k = ((distance - near) / (far - near).max(1e-3)).clamp(0.0, 1.0);
        let center_weight = k * k * (3.0 - 2.0 * k);
        Some(
            frame
                .point_velocity(surface)
                .lerp(frame.linear_velocity, center_weight),
        )
    }

    pub fn mass(&self, body: Entity) -> Option<BodyMass> {
        let (rb, _, rot, _, _, mass, inertia, _, _) = self.bodies.get(body).ok()?;
        if !rb.is_dynamic() {
            return Some(BodyMass::INFINITE);
        }
        Some(BodyMass {
            inverse_mass: mass.inverse(),
            inverse_inertia: inertia.rotated(rot.0).inverse().to_mat3(),
        })
    }

    /// Applies a world-space impulse at a world-space point. No-op for
    /// non-dynamic bodies.
    pub fn apply_impulse(&mut self, body: Entity, impulse: Vec3, world_point: Vec3) {
        let Some(mass) = self.mass(body) else { return };
        if mass.is_infinite() {
            return;
        }
        let Some(frame) = self.frame(body) else {
            return;
        };
        let Ok((_, _, _, mut lin, mut ang, ..)) = self.bodies.get_mut(body) else {
            return;
        };
        lin.0 += impulse * mass.inverse_mass;
        ang.0 += mass.inverse_inertia * (world_point - frame.center_of_mass).cross(impulse);
    }
}
