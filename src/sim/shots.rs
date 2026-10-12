//! Shots, on the authority.
//!
//! A shot starts as a [`PlayerEvent::Fired`] entry in its shooter's log, made
//! by the shooter's (predicted) simulation. The authority picks those up on
//! the tick they were fired. What happens then is the gun's family (see
//! `docs/plan-weapons.md`):
//!
//! - **Bullets** fly in straight lines: there is no gravity, and a bullet
//!   keeps the velocity its shooter had. Each tick it sweeps the stretch it
//!   travels; the first thing it meets stops it (a sniper's goes through one
//!   body first). It pushes what it hits by its momentum.
//! - **Plasma bolts** fly the same way, slowly, and burst where they hit:
//!   everything with health the blast can see (a clear line from the burst:
//!   no harm through a hull) within its radius is hurt and shoved, less with
//!   distance.
//! - **Arcs** are instant and short: the first thing in the crosshair, then
//!   a jump to the nearest other target within reach of it in clear sight,
//!   and on, so many times. Hulls and decks stop a jump; ropes aren't in the
//!   way of anything. An arc doesn't find weak spots.
//! - **Laser** beams are instant: a tick of beam hits the first thing in the
//!   way.
//!
//! Something with [`Vitals`] is hurt; a free body is pushed; a player (or
//! robot) is shoved as by a blast (floating, it drifts off; on boots, only a
//! hard shove knocks it off the deck). Teammates are passed through. Clients
//! draw their own tracers from the same log entries; only the authority
//! decides what was hit.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::SimTick;
use super::bodies::Bodies;
use super::bots::{BodyTuning, Robot, WEAK_MULTIPLIER, body_rotation, tuning_of};
use super::events::PlayerEvent;
use super::player::Player;
use super::tuning::Tuning;
use super::vitals::{Damage, Vitals};
use super::weapons::ShotKind;
use super::Layer;

/// How deep past where a bullet meets a body it can still find the weak
/// spot (m): weak spots sit just inside the hull capsule, so a shot from
/// the far side doesn't reach through the body to one.
const WEAK_DEPTH: f32 = 0.25;

/// Seconds a bullet flies before it's forgotten.
pub const SHOT_LIFE: f32 = 2.0;
/// A plasma bolt is slow: it flies longer.
pub const BOLT_LIFE: f32 = 6.0;

/// How far an arc jumps from one target to the next (m), and what each
/// jump keeps of the damage.
pub const ARC_JUMP: f32 = 7.0;
pub const ARC_FALLOFF: f32 = 0.6;

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
    /// Where it was fired from: what its victim is told.
    pub origin: Vec3,
    pub kind: ShotKind,
    /// A body it has gone through already (a piercing round).
    pub through: Option<Entity>,
}

/// Every shot in flight.
#[derive(Resource, Default, Debug)]
pub struct Shots(pub Vec<Shot>);

/// Shoves this tick for players (and robots) floating free: who, and the
/// impulse (N s). Applied after the shots are settled.
#[derive(Resource, Default, Debug)]
pub struct Shoves(pub Vec<(Entity, Vec3)>);

/// Authority: shots fired this tick start.
pub fn launch_shots(tick: Res<SimTick>, players: Query<(Entity, &Player, Option<&Team>)>, mut shots: ResMut<Shots>) {
    for (entity, player, team) in &players {
        for (at, event) in player.events.iter() {
            let PlayerEvent::Fired {
                damage,
                mass,
                origin,
                velocity,
                kind,
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
                    origin,
                    kind,
                    through: None,
                });
            }
        }
    }
}

/// Something that can be hurt: where its middle is, and its side.
struct Target {
    entity: Entity,
    center: Vec3,
    team: Team,
}

/// Authority: moves every flown shot one tick, settles the instant ones,
/// and settles what they all hit.
#[allow(clippy::too_many_arguments)]
pub fn fly_shots(
    time: Res<Time>,
    spatial: SpatialQuery,
    mut shots: ResMut<Shots>,
    mut shoves: ResMut<Shoves>,
    mut bodies: Bodies,
    teams: Query<Option<&Team>, With<Player>>,
    vitals: Query<(Entity, Option<&Player>, Option<&BodyTuning>, Option<&Team>), With<Vitals>>,
    robots: Query<(&Robot, &Player, Option<&BodyTuning>)>,
    base: Res<Tuning>,
    mut damage: MessageWriter<Damage>,
) {
    let dt = time.delta_secs();
    let targets: Vec<Target> = vitals
        .iter()
        .filter_map(|(entity, player, own, team)| {
            let player = player?;
            let frame = player.mode.body().and_then(|b| bodies.frame(b));
            let (center, _) = player.mode.world_kinematics(tuning_of(&base, own), frame.as_ref());
            Some(Target {
                entity,
                center,
                team: team.copied().unwrap_or_default(),
            })
        })
        .collect();
    // A clear line between two points, as far as hulls and rocks go.
    let clear = |from: Vec3, to: Vec3| {
        let d = to - from;
        let Ok(dir) = Dir3::new(d) else { return true };
        let world = SpatialQueryFilter::from_mask(Layer::World);
        spatial.cast_ray(from, dir, d.length(), true, &world).is_none()
    };
    let mut hurts: Vec<Damage> = Vec::new();
    // Who's pushed: at what velocity, by what mass, where.
    let mut pushes: Vec<(Entity, Vec3, f32, Vec3)> = Vec::new();
    let all = std::mem::take(&mut shots.0);
    for mut shot in all {
        let instant = matches!(shot.kind, ShotKind::Arc { .. } | ShotKind::Laser { .. });
        let (dir, length) = if instant {
            let range = match shot.kind {
                ShotKind::Arc { range, .. } | ShotKind::Laser { range } => range,
                _ => 0.0,
            };
            (Dir3::new(shot.velocity).ok(), range)
        } else {
            let step = shot.velocity * dt;
            (Dir3::new(step).ok(), step.length())
        };
        let Some(dir) = dir else { continue };
        let mut excluded = vec![shot.shooter];
        excluded.extend(shot.through);
        let filter = SpatialQueryFilter::default().with_excluded_entities(excluded);
        let mut hits = spatial.ray_hits(shot.position, dir, length, 8, true, &filter);
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        let hit = hits.into_iter().find(|hit| match teams.get(hit.entity) {
            // A player: only the other side stops a shot.
            Ok(team) => team.copied().unwrap_or_default() != shot.team,
            Err(_) => true,
        });
        let Some(hit) = hit else {
            if instant {
                continue;
            }
            shot.position += dir * length;
            shot.age += dt;
            let life = if matches!(shot.kind, ShotKind::Plasma { .. }) { BOLT_LIFE } else { SHOT_LIFE };
            if shot.age < life {
                shots.0.push(shot);
            }
            continue;
        };
        let point = shot.position + dir * hit.distance;
        let hurt = |target: Entity, amount: f32, weak: bool| Damage {
            target,
            amount,
            source: Some(shot.shooter),
            weak,
            from: Some(shot.origin),
        };
        let living = targets.iter().any(|t| t.entity == hit.entity);
        match shot.kind {
            ShotKind::Bullet { pierce } => {
                if living {
                    // A robot's weak spot is a sphere (inside it, or behind
                    // its armour); the bullet's line through the body finds it.
                    let weak = robots.get(hit.entity).is_ok_and(|(kind, player, own)| {
                        let frame = player.mode.body().and_then(|b| bodies.frame(b));
                        let (center, _) = player.mode.world_kinematics(tuning_of(&base, own), frame.as_ref());
                        let (local, radius) = kind.weak_spot();
                        let spot = center + body_rotation(&player.mode, frame.as_ref()) * local;
                        let along = (spot - point).dot(*dir).clamp(0.0, WEAK_DEPTH);
                        (point + *dir * along).distance(spot) < radius
                    });
                    hurts.push(hurt(hit.entity, if weak { shot.damage * WEAK_MULTIPLIER } else { shot.damage }, weak));
                    pushes.push((hit.entity, shot.velocity, shot.mass, point));
                    // Through it, if it pierces, on from here.
                    if pierce > 0 && shot.through.is_none() {
                        shot.through = Some(hit.entity);
                        shot.kind = ShotKind::Bullet { pierce: pierce - 1 };
                        shot.position = point;
                        shot.age += dt;
                        shots.0.push(shot);
                    }
                } else {
                    pushes.push((hit.entity, shot.velocity, shot.mass, point));
                }
            }
            ShotKind::Plasma { splash } => {
                // The burst: what it struck takes the whole of it, what's
                // near and in sight of it less; loose bodies in reach are
                // pushed off from it.
                let burst = point - *dir * 0.05;
                let reach = Collider::sphere(splash);
                let world = SpatialQueryFilter::from_mask(Layer::World);
                let mut pushed: Vec<Entity> = Vec::new();
                for collider in spatial.shape_intersections(&reach, burst, Quat::IDENTITY, &world) {
                    let Some(body) = bodies.body_of_collider(collider) else { continue };
                    let Some(frame) = bodies.frame(body) else { continue };
                    let direct = collider == hit.entity;
                    // In sight: the first thing toward its middle is itself.
                    let seen = || {
                        let d = frame.center_of_mass - burst;
                        let Ok(toward) = Dir3::new(d) else { return true };
                        spatial
                            .cast_ray(burst, toward, d.length(), true, &world)
                            .is_none_or(|h| bodies.body_of_collider(h.entity) == Some(body))
                    };
                    if pushed.contains(&body) || !direct && !seen() {
                        continue;
                    }
                    pushed.push(body);
                    let d = frame.center_of_mass.distance(burst);
                    let near = if direct { 1.0 } else { (1.0 - d / splash).max(0.25) };
                    let away = if direct { *dir } else { (frame.center_of_mass - burst).normalize_or(*dir) };
                    pushes.push((collider, away * shot.velocity.length(), shot.mass * near, if direct { point } else { frame.center_of_mass }));
                }
                for t in &targets {
                    if t.team == shot.team {
                        continue;
                    }
                    let d = t.center.distance(burst);
                    let direct = t.entity == hit.entity;
                    if !direct && (d > splash || !clear(burst, t.center)) {
                        continue;
                    }
                    let near = if direct { 1.0 } else { (1.0 - d / splash).max(0.25) };
                    hurts.push(hurt(t.entity, shot.damage * near, false));
                    let away = (t.center - burst).normalize_or(*dir);
                    pushes.push((t.entity, away * shot.velocity.length(), shot.mass * near, t.center));
                }
            }
            ShotKind::Arc { chain, .. } => {
                if living {
                    let mut struck = vec![hit.entity];
                    let mut at = targets.iter().find(|t| t.entity == hit.entity).map_or(point, |t| t.center);
                    let mut amount = shot.damage;
                    hurts.push(hurt(hit.entity, amount, false));
                    for _ in 0..chain {
                        amount *= ARC_FALLOFF;
                        let next = targets
                            .iter()
                            .filter(|t| t.team != shot.team && !struck.contains(&t.entity))
                            .filter(|t| t.center.distance(at) <= ARC_JUMP && clear(at, t.center))
                            .min_by(|a, b| a.center.distance(at).total_cmp(&b.center.distance(at)));
                        let Some(next) = next else { break };
                        hurts.push(hurt(next.entity, amount, false));
                        struck.push(next.entity);
                        at = next.center;
                    }
                }
            }
            ShotKind::Laser { .. } => {
                if living {
                    hurts.push(hurt(hit.entity, shot.damage, false));
                }
            }
        }
    }
    for d in hurts {
        damage.write(d);
    }
    // Pushes: a free body by its own response; a player or robot floating
    // free is shoved (one on its boots holds).
    for (entity, velocity, mass, point) in pushes {
        if mass <= 0.0 {
            continue;
        }
        if targets.iter().any(|t| t.entity == entity) {
            shoves.0.push((entity, velocity * mass));
        } else if let Some(body) = bodies.body_of_collider(entity) {
            // Relative to the body: a bolt chasing a fleeing rock pushes less.
            let relative = velocity - bodies.frame(body).map_or(Vec3::ZERO, |f| f.point_velocity(point));
            bodies.apply_impulse(body, relative * mass, point);
        }
    }
}

/// Authority, after the shots: players and robots take their shoves, as
/// from a blast (see [`Player::shove`]: floating, it's velocity; on boots, a
/// hard enough one knocks them off the deck). A client learns of its own by
/// correction: being shot is the authority's to know.
pub fn apply_shoves(
    base: Res<Tuning>,
    bodies: Bodies,
    mut shoves: ResMut<Shoves>,
    mut players: Query<(&mut Player, Option<&BodyTuning>)>,
) {
    for (entity, impulse) in shoves.0.drain(..) {
        let Ok((mut player, own)) = players.get_mut(entity) else { continue };
        let tuning = tuning_of(&base, own);
        let frame = player.mode.body().and_then(|b| bodies.frame(b));
        player.shove(impulse / tuning.player_mass, frame.as_ref(), tuning);
    }
}
