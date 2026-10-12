//! A volatile robot going up: everything near it, robots too, is hurt and
//! shoved, less the further off it is, unless a wall is in the way. Loose
//! bodies (rocks, wreckage) are pushed. A volatile robot it kills goes up
//! the tick after, so blasts chain.
//!
//! The shove is an impulse, so it's mass that decides how far it throws
//! something: a crew member is flung, a Gunner staggers. On boots, a shove
//! that would lift you off the deck or slide you hard knocks you off it.
//! (The authority shoves a client's player; the client learns of it as a
//! correction of what it predicted.)

use avian3d::prelude::*;
use bevy::prelude::*;

use super::{BodyTuning, Robot, tuning_of};
use crate::sim::Layer;
use crate::sim::bodies::Bodies;
use crate::sim::player::Player;
use crate::sim::tuning::Tuning;
use crate::sim::vitals::{Damage, Vitals};

/// A blast: where, how far it reaches (m), the damage and impulse (N·s) it
/// does at its heart (falling to nothing at its edge), who set it off, and
/// whose kill it was.
#[derive(Message, Clone, Copy, Debug)]
pub struct Detonation {
    pub at: Vec3,
    pub radius: f32,
    pub damage: f32,
    pub impulse: f32,
    /// The robot that went up (gone by the time the blast is resolved).
    pub robot: Entity,
    pub credit: Option<Entity>,
}

/// A volatile robot's blast, by its kind: the bigger the robot, the more it
/// carries.
pub fn blast_of(kind: Robot, at: Vec3, robot: Entity, credit: Option<Entity>) -> Detonation {
    let (radius, damage, impulse) = match kind {
        Robot::Gunner => (6.0, 70.0, 700.0),
        Robot::Swarmer => (4.5, 45.0, 400.0),
        Robot::Grappler => (7.0, 80.0, 900.0),
        Robot::Turret => (7.0, 80.0, 900.0),
        Robot::Crawler => (4.0, 40.0, 350.0),
        // No affixes, so never volatile; for completeness.
        Robot::Limpet => (2.5, 20.0, 150.0),
    };
    Detonation {
        at,
        radius,
        damage,
        impulse,
        robot,
        credit,
    }
}

/// How much of a blast reaches `d` metres from its heart.
pub fn falloff(d: f32, radius: f32) -> f32 {
    (1.0 - d / radius).clamp(0.0, 1.0)
}

/// Authority, after deaths are settled: this tick's blasts hurt and shove
/// what's near them.
pub fn detonate(
    base: Res<Tuning>,
    spatial: SpatialQuery,
    mut blasts: MessageReader<Detonation>,
    mut bodies: Bodies,
    mut players: Query<(Entity, &mut Player, Option<&BodyTuning>, Has<Robot>), With<Vitals>>,
    mut damage: MessageWriter<Damage>,
) {
    for blast in blasts.read() {
        // Nothing behind a wall is touched. (Players' capsules aren't
        // walls, so a crowd doesn't shelter itself.)
        let sheltered = |to: Vec3| {
            let Ok(dir) = Dir3::new(to - blast.at) else { return false };
            let d = blast.at.distance(to);
            spatial
                .cast_ray(blast.at, dir, d, true, &SpatialQueryFilter::from_mask(Layer::World))
                .is_some_and(|hit| hit.distance < d - 0.3)
        };
        for (entity, mut player, own, robot) in &mut players {
            if entity == blast.robot {
                continue;
            }
            let tuning = tuning_of(&base, own);
            let frame = player.mode.body().and_then(|b| bodies.frame(b));
            let (center, _) = player.mode.world_kinematics(tuning, frame.as_ref());
            let d = center.distance(blast.at);
            let k = falloff(d, blast.radius);
            if k <= 0.0 || sheltered(center) {
                continue;
            }
            // The kill (and a chain's) is its killer's; the crew it hurts
            // aren't their hits.
            damage.write(Damage {
                target: entity,
                amount: blast.damage * k,
                source: if robot { blast.credit } else { None },
                weak: false,
                // The victim's arcs point at the blast's centre.
                from: Some(blast.at),
            });
            let out = (center - blast.at).normalize_or(Vec3::Y);
            player.shove(out * blast.impulse * k / tuning.player_mass, frame.as_ref(), tuning);
        }
        // Loose bodies in reach are pushed from where they're nearest it.
        let reach = Collider::sphere(blast.radius);
        let filter = SpatialQueryFilter::from_mask(Layer::World);
        let mut pushed: Vec<Entity> = Vec::new();
        for collider in spatial.shape_intersections(&reach, blast.at, Quat::IDENTITY, &filter) {
            let Some(body) = bodies.body_of_collider(collider) else { continue };
            if pushed.contains(&body) {
                continue;
            }
            pushed.push(body);
            let Some(frame) = bodies.frame(body) else { continue };
            let d = frame.center_of_mass.distance(blast.at);
            let k = falloff(d, blast.radius).max(0.2);
            let out = (frame.center_of_mass - blast.at).normalize_or(Vec3::Y);
            bodies.apply_impulse(body, out * blast.impulse * k, frame.center_of_mass);
        }
    }
}
