//! Bullets in flight, on the authority.
//!
//! A shot starts as a [`PlayerEvent::Fired`] entry in its shooter's log, made
//! by the shooter's (predicted) simulation. The authority picks those up on
//! the tick they were fired and flies them in straight lines: there is no
//! gravity, and a bullet keeps the velocity its shooter had. Each tick a
//! bullet sweeps the stretch it travels; the first thing it meets stops it.
//! Something with [`Vitals`] is hurt, and a free body is pushed by the
//! bullet's momentum. Teammates are passed through.
//!
//! Clients draw their own tracers from the same log entries; only the
//! authority decides what was hit.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::SimTick;
use super::bodies::Bodies;
use super::bots::{BodyTuning, Robot, WEAK_MULTIPLIER, body_rotation, tuning_of};
use super::tuning::Tuning;
use super::events::PlayerEvent;
use super::player::Player;
use super::vitals::{Damage, Vitals};

/// How deep past where a bullet meets a body it can still find the weak
/// spot (m): weak spots sit just inside the hull capsule, so a shot from
/// the far side doesn't reach through the body to one.
const WEAK_DEPTH: f32 = 0.25;

/// Seconds a bullet flies before it's forgotten.
pub const SHOT_LIFE: f32 = 2.0;

/// Which side someone is on. Shots pass through their own side. Players
/// without one are crew.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Team {
    #[default]
    Crew,
    Robots,
}

#[derive(Clone, Copy, Debug)]
pub struct Shot {
    pub shooter: Entity,
    pub team: Team,
    /// What it does to what it hits, and its mass (kg), for its push.
    pub damage: f32,
    pub mass: f32,
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
}

/// Every bullet in flight.
#[derive(Resource, Default, Debug)]
pub struct Shots(pub Vec<Shot>);

/// Authority: shots fired this tick start flying.
pub fn launch_shots(
    tick: Res<SimTick>,
    players: Query<(Entity, &Player, Option<&Team>)>,
    mut shots: ResMut<Shots>,
) {
    for (entity, player, team) in &players {
        for (at, event) in player.events.iter() {
            let PlayerEvent::Fired {
                damage,
                mass,
                origin,
                velocity,
                ..
            } = event
            else {
                continue;
            };
            if at == tick.0 {
                shots.0.push(Shot {
                    shooter: entity,
                    team: team.copied().unwrap_or_default(),
                    damage,
                    mass,
                    position: origin,
                    velocity,
                    age: 0.0,
                });
            }
        }
    }
}

/// Authority: moves every bullet one tick, and settles what it hits.
pub fn fly_shots(
    time: Res<Time>,
    spatial: SpatialQuery,
    mut shots: ResMut<Shots>,
    mut bodies: Bodies,
    teams: Query<Option<&Team>, With<Player>>,
    vitals: Query<(), With<Vitals>>,
    robots: Query<(&Robot, &Player, Option<&BodyTuning>)>,
    base: Res<Tuning>,
    mut damage: MessageWriter<Damage>,
) {
    let dt = time.delta_secs();
    shots.0.retain_mut(|shot| {
        let step = shot.velocity * dt;
        let (Ok(dir), length) = (Dir3::new(step), step.length()) else {
            return false;
        };
        let filter = SpatialQueryFilter::default().with_excluded_entities([shot.shooter]);
        let mut hits = spatial.ray_hits(shot.position, dir, length, 8, true, &filter);
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        let hit = hits.into_iter().find(|hit| match teams.get(hit.entity) {
            // A player: only the other side stops a bullet.
            Ok(team) => team.copied().unwrap_or_default() != shot.team,
            Err(_) => true,
        });
        let Some(hit) = hit else {
            shot.position += step;
            shot.age += dt;
            return shot.age < SHOT_LIFE;
        };
        let point = shot.position + dir * hit.distance;
        if vitals.contains(hit.entity) {
            // A robot's weak spot is a sphere (inside it, or behind its
            // armour); the bullet's line through the body finds it.
            let weak = robots.get(hit.entity).is_ok_and(|(kind, player, own)| {
                let frame = player.mode.body().and_then(|b| bodies.frame(b));
                let (center, _) = player.mode.world_kinematics(tuning_of(&base, own), frame.as_ref());
                let (local, radius) = kind.weak_spot();
                let spot = center + body_rotation(&player.mode, frame.as_ref()) * local;
                let along = (spot - point).dot(*dir).clamp(0.0, WEAK_DEPTH);
                (point + *dir * along).distance(spot) < radius
            });
            damage.write(Damage {
                target: hit.entity,
                amount: if weak { shot.damage * WEAK_MULTIPLIER } else { shot.damage },
                source: Some(shot.shooter),
                weak,
            });
        } else if let Some(body) = bodies.body_of_collider(hit.entity) {
            let relative = shot.velocity - bodies.frame(body).map_or(Vec3::ZERO, |f| f.point_velocity(point));
            bodies.apply_impulse(body, relative * shot.mass, point);
        }
        false
    });
}
