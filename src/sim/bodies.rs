//! Access to rigid bodies as reference frames, and impulse application.
//!
//! A body is anything with a pose, velocities, and [`BodyInertia`]. It need
//! not be simulated here: on a network client, bodies are posed from their
//! replicated motion, and `Bodies` treats them exactly the same.

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

/// A body's mass properties, derived from its collider the same way on every
/// peer (so a network client has them without simulating the body).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BodyInertia {
    /// Zero for bodies impulses can't move (kinematic, static).
    pub inverse_mass: f32,
    /// Inverse of the (isotropic) angular inertia; zero when immovable.
    pub inverse_inertia: f32,
    /// In body coordinates.
    pub center_of_mass: Vec3,
    /// How far the body extends from its center of mass (m).
    pub reach: f32,
}

impl BodyInertia {
    /// A body only scripts move (the ship). It still turns about its center
    /// of mass, as Avian computes it from the colliders.
    pub fn immovable(collider: &Collider) -> Self {
        let center_of_mass = collider.mass_properties(1.0).center_of_mass;
        Self {
            inverse_mass: 0.0,
            inverse_inertia: 0.0,
            center_of_mass,
            reach: reach(collider, center_of_mass),
        }
    }

    fn dynamic(collider: &Collider, density: f32) -> Self {
        let props = collider.mass_properties(density);
        let inertia = props.principal_angular_inertia.element_sum() / 3.0;
        Self {
            inverse_mass: 1.0 / props.mass,
            inverse_inertia: 1.0 / inertia,
            center_of_mass: props.center_of_mass,
            reach: reach(collider, props.center_of_mass),
        }
    }
}

/// A bound on the distance from `center` to any point of the collider.
fn reach(collider: &Collider, center: Vec3) -> f32 {
    let aabb = collider.aabb(Vec3::ZERO, Quat::IDENTITY);
    (aabb.max - aabb.min).length() / 2.0 + ((aabb.max + aabb.min) / 2.0 - center).length()
}

/// Mass properties for a dynamic body: [`BodyInertia`], and the same values
/// imposed on Avian. Inertia is made isotropic (equal about every axis, the
/// mean of the collider's principal moments). An isotropic body's spin is
/// torque-free, so a tumbling rock keeps a constant angular velocity until
/// something touches it, which makes its motion exactly predictable.
pub fn dynamic_mass(collider: &Collider, density: f32) -> impl Bundle {
    let inertia = BodyInertia::dynamic(collider, density);
    (
        inertia,
        Mass(1.0 / inertia.inverse_mass),
        AngularInertia {
            principal: Vec3::splat(1.0 / inertia.inverse_inertia),
            local_frame: Quat::IDENTITY,
        },
        CenterOfMass(inertia.center_of_mass),
        NoAutoMass,
        NoAutoAngularInertia,
        NoAutoCenterOfMass,
    )
}

type BodyData = (
    &'static Position,
    &'static Rotation,
    &'static mut LinearVelocity,
    &'static mut AngularVelocity,
    &'static BodyInertia,
    Option<&'static PreviousVelocity>,
);

#[derive(SystemParam)]
pub struct Bodies<'w, 's> {
    bodies: Query<'w, 's, BodyData>,
    collider_of: Query<'w, 's, &'static ColliderOf>,
    shapes: Query<'w, 's, &'static Collider>,
    vanished: Option<Res<'w, VanishedBodies>>,
}

/// Bodies that have just gone (despawned: a rock recycled, a grazer
/// killed, a pod brought home, a ship wrecked) and how they were posed and
/// moving as they went, for a little while, on every peer. Whatever was on
/// them or measured against them (boots, positions in their frame) still
/// has a frame to leave from; see [`Bodies::vanished`].
#[derive(Resource, Default)]
pub struct VanishedBodies(Vec<(Entity, u32, BodyFrame)>);

/// How long (ticks) a vanished body's last frame is kept.
const VANISHED_TICKS: u32 = 120;

/// Notes a body's last frame as it goes.
pub fn note_vanished(
    remove: On<Remove, BodyInertia>,
    tick: Option<Res<super::SimTick>>,
    bodies: Query<(&Position, &Rotation, Option<&LinearVelocity>, Option<&AngularVelocity>, &BodyInertia)>,
    mut vanished: ResMut<VanishedBodies>,
) {
    let Ok((pos, rot, lin, ang, inertia)) = bodies.get(remove.entity) else { return };
    let tick = tick.map_or(0, |t| t.0);
    vanished.0.push((
        remove.entity,
        tick,
        BodyFrame {
            position: pos.0,
            rotation: rot.0,
            center_of_mass: pos.0 + rot.0 * inertia.center_of_mass,
            linear_velocity: lin.map_or(Vec3::ZERO, |v| v.0),
            angular_velocity: ang.map_or(Vec3::ZERO, |v| v.0),
        },
    ));
}

/// Forgets vanished bodies after [`VANISHED_TICKS`].
pub fn forget_vanished(tick: Res<super::SimTick>, mut vanished: ResMut<VanishedBodies>) {
    vanished.0.retain(|(_, at, _)| tick.0.wrapping_sub(*at) < VANISHED_TICKS);
}

impl Bodies<'_, '_> {
    /// The rigid body a collider belongs to.
    pub fn body_of_collider(&self, collider: Entity) -> Option<Entity> {
        self.collider_of.get(collider).ok().map(|c| c.body)
    }

    /// The last frame of a body that has just gone (see [`VanishedBodies`]),
    /// if `body` is one.
    pub fn vanished(&self, body: Entity) -> Option<BodyFrame> {
        if self.bodies.contains(body) {
            return None;
        }
        self.vanished.as_ref()?.0.iter().rev().find(|(e, ..)| *e == body).map(|(_, _, f)| *f)
    }

    /// Where a body is and how it moves; for one that has just gone, as it
    /// went (so positions in its frame stay where they were).
    pub fn frame(&self, body: Entity) -> Option<BodyFrame> {
        let Ok((pos, rot, lin, ang, inertia, _)) = self.bodies.get(body) else {
            return self.vanished(body);
        };
        Some(BodyFrame {
            position: pos.0,
            rotation: rot.0,
            center_of_mass: pos.0 + rot.0 * inertia.center_of_mass,
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
        let (.., inertia, _) = self.bodies.get(body).ok()?;
        Some(BodyMass {
            inverse_mass: inertia.inverse_mass,
            // Isotropic: the same in every orientation.
            inverse_inertia: Mat3::from_diagonal(Vec3::splat(inertia.inverse_inertia)),
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
        let Ok((_, _, mut lin, mut ang, ..)) = self.bodies.get_mut(body) else {
            return;
        };
        lin.0 += impulse * mass.inverse_mass;
        ang.0 += mass.inverse_inertia * (world_point - frame.center_of_mass).cross(impulse);
    }
}
