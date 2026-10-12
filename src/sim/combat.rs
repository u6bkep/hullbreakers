//! Ship-to-ship combat: laser turrets, shields, and hull integrity.
//!
//! Ship weapons are slow on purpose. A turret fires a long beam on a fixed
//! cycle; against a raised shield it barely scratches the hull. The fast way
//! to win is to drop the enemy's shield from inside.
//!
//! When each turret fires is a function of the tick alone, and where its beam
//! goes is a function of the two ships' poses and the target's shield, all of
//! which every peer has. So every peer knows every beam at every tick, and a
//! client can predict being shoved by one (see [`Beams`]). Only damage is the
//! authority's to apply.

use avian3d::prelude::*;
use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bodies::Bodies;
use super::ship::{ShipHull, Turret};
use super::{SimTick, TICK_HZ};

/// Turrets per ship.
pub const TURRETS: u8 = 2;
/// A turret's firing cycle: a beam this long (ticks), then a pause.
const CYCLE_TICKS: u32 = 6 * TICK_HZ as u32;
const BEAM_TICKS: u32 = 3 * TICK_HZ as u32;
/// Fraction of the time a turret's beam is on.
const DUTY: f32 = BEAM_TICKS as f32 / CYCLE_TICKS as f32;

/// When turret `index`'s current or last beam started, if it is firing or
/// has fired at least once by `tick`.
pub fn beam_start(index: u8, tick: u32) -> Option<u32> {
    // Turrets take turns: each starts a different part of the cycle.
    let offset = index as u32 * CYCLE_TICKS / TURRETS as u32;
    let shifted = tick.checked_sub(offset)?;
    Some(shifted - shifted % CYCLE_TICKS + offset)
}

/// Whether turret `index` is firing at `tick`.
pub fn beam_on(index: u8, tick: u32) -> bool {
    beam_start(index, tick).is_some_and(|start| tick - start < BEAM_TICKS)
}

/// A ship in a fight: what it shoots at, how intact it is, and whether its
/// shield is up.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[component(map_entities)]
pub struct Combatant {
    pub target: Option<Entity>,
    /// 1 is intact, 0 is destroyed.
    pub integrity: f32,
    pub shield: bool,
    /// Seconds of fire it takes to destroy this ship: through its shield,
    /// and without.
    pub shielded_seconds: f32,
    pub exposed_seconds: f32,
}

impl MapEntities for Combatant {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        if let Some(target) = &mut self.target {
            *target = mapper.get_mapped(*target);
        }
    }
}


/// One beam this tick, in world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Beam {
    pub shooter: Entity,
    pub target: Entity,
    pub from: Vec3,
    /// Where it stops: on the target's shield, or its hull.
    pub to: Vec3,
    /// Whether it hit the shield (rather than the hull).
    pub shielded: bool,
}

/// Every beam firing this tick. Rebuilt each tick, after the physics step,
/// on every peer that simulates players.
#[derive(Resource, Default, Debug)]
pub struct Beams(pub Vec<Beam>);

/// Where a ray from `origin` along `dir` (unit) first meets the ellipsoid
/// with `center` and `radii`, all in one frame; `None` if it misses or the
/// origin is inside.
fn ray_ellipsoid(origin: Vec3, dir: Vec3, center: Vec3, radii: Vec3) -> Option<f32> {
    // Scale space so the ellipsoid is a unit sphere.
    let o = (origin - center) / radii;
    let d = dir / radii;
    let (a, b, c) = (d.length_squared(), 2.0 * o.dot(d), o.length_squared() - 1.0);
    if c <= 0.0 {
        return None;
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a);
    (t >= 0.0).then_some(t)
}

/// Aims every firing turret at its ship's target and finds where the beam
/// stops.
pub fn update_beams(
    tick: Res<SimTick>,
    bodies: Bodies,
    ships: Query<(&Combatant, &Collider, &ShipHull)>,
    turrets: Query<(&Turret, &Transform, &ChildOf)>,
    mut beams: ResMut<Beams>,
) {
    beams.0.clear();
    for (turret, mount, ship) in &turrets {
        let shooter = ship.parent();
        if !beam_on(turret.index, tick.0) {
            continue;
        }
        let Ok((combatant, ..)) = ships.get(shooter) else {
            continue;
        };
        let Some(target) = combatant.target else {
            continue;
        };
        let (Some(own), Some(theirs), Ok((target_state, hull, kind))) =
            (bodies.frame(shooter), bodies.frame(target), ships.get(target))
        else {
            continue;
        };
        let from = own.point_to_world(mount.translation);
        // At the middle of their hull.
        let aim = theirs.point_to_world(Vec3::new(0.0, 1.5, 0.0));
        let Ok(dir) = Dir3::new(aim - from) else {
            continue;
        };
        let reach = from.distance(aim) + 50.0;
        let (distance, shielded) = if target_state.shield {
            let local_origin = theirs.point_to_local(from);
            let local_dir = theirs.dir_to_local(*dir);
            let shield = kind.design().shield;
            let (center, radii) = (shield.center, shield.radii);
            match ray_ellipsoid(local_origin, local_dir, center, radii) {
                Some(t) => (t, true),
                None => (from.distance(aim), false),
            }
        } else {
            let hit = hull.cast_ray(theirs.position, theirs.rotation, from, *dir, reach, true);
            (hit.map_or(from.distance(aim), |(t, _)| t), false)
        };
        beams.0.push(Beam {
            shooter,
            target,
            from,
            to: from + *dir * distance,
            shielded,
        });
    }
}

/// Authority: beams wear down what they hit.
pub fn apply_damage(beams: Res<Beams>, time: Res<Time>, mut ships: Query<&mut Combatant>) {
    let dt = time.delta_secs();
    for beam in &beams.0 {
        let Ok(mut target) = ships.get_mut(beam.target) else {
            continue;
        };
        // Seconds-to-destroy count time under fire from all of a ship's
        // attacker's turrets taking turns.
        let seconds = if beam.shielded {
            target.shielded_seconds
        } else {
            target.exposed_seconds
        };
        let rate = 1.0 / (seconds * DUTY * TURRETS as f32);
        target.integrity = (target.integrity - rate * dt).max(0.0);
    }
}

/// A player closer than this to a beam's line (m) is caught in it.
pub const BEAM_RADIUS: f32 = 0.9;

/// Which way a beam shoves a player at `position`, if one is crossing them:
/// out of the beam and along it. Unit length, summed over beams.
pub fn beam_shove(beams: &Beams, position: Vec3) -> Option<Vec3> {
    let mut shove = Vec3::ZERO;
    for beam in &beams.0 {
        let span = beam.to - beam.from;
        let along = (position - beam.from).dot(span) / span.length_squared();
        if !(0.0..=1.0).contains(&along) {
            continue;
        }
        let off = position - (beam.from + span * along);
        if off.length() > BEAM_RADIUS {
            continue;
        }
        shove += off.normalize_or(span.any_orthonormal_vector()) + span.normalize();
    }
    shove.try_normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turrets_take_turns() {
        let half = CYCLE_TICKS / 2;
        assert!(beam_on(0, 0));
        assert!(!beam_on(1, 0));
        assert!(!beam_on(0, half));
        assert!(beam_on(1, half));
        assert_eq!(beam_start(0, CYCLE_TICKS + 5), Some(CYCLE_TICKS));
        assert_eq!(beam_start(1, half + 5), Some(half));
    }

    #[test]
    fn beams_stop_at_the_shield() {
        let t = ray_ellipsoid(Vec3::new(0.0, 0.0, -100.0), Vec3::Z, Vec3::ZERO, Vec3::new(2.0, 2.0, 10.0));
        assert!((t.unwrap() - 90.0).abs() < 1e-4);
        assert!(ray_ellipsoid(Vec3::new(5.0, 0.0, -100.0), Vec3::Z, Vec3::ZERO, Vec3::splat(2.0)).is_none());
    }
}
