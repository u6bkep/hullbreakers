//! How robots (limpets among them) reach a remote client.
//!
//! A client never simulates a robot: it draws it, hears it, and aims at
//! it. So rather than the robot's whole [`Player`] (its loadout, input
//! cursor and the rest, about 170 bytes at the send rate for every robot
//! that moves), the authority sends what presentation reads:
//!
//! - [`RobotView`]: its pose (the mode), its rope, its boots and its beams,
//!   quantised on the wire (millimetres, centimetres a second, 1/32767 of a
//!   unit), interpolated like a player. It's rewritten only when it's moved
//!   by more than the quantisation, so an idle robot costs nothing.
//! - [`RobotLog`]: the recent entries of its event log (shots, landings,
//!   ropes), rewritten only when there's a new one.
//!
//! On a remote client, the robot's `Player` is built from these
//! ([`present_robots`]), so everything that draws or hears players reads
//! robots as before. The host shares the authority's world and reads the
//! real one.

use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::sim::bots::Robot;
use crate::sim::events::EventLog;
use crate::sim::motion::rotation_error;
use crate::sim::player::{Grapple, Mode, Player};
use crate::sim::weapons::{Beam, Loadout};

/// What a remote client needs of a robot to draw it: see the module docs.
/// In memory it holds what the wire carries (the authority stores the
/// quantised value), so every peer has the same.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "WireView", from = "WireView")]
#[component(map_entities)]
pub struct RobotView {
    pub mode: Mode,
    pub grapple: Option<Grapple>,
    pub boots_on: bool,
    pub beams: [Option<Beam>; 2],
}

impl MapEntities for RobotView {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        if let Mode::Booted { body, .. } = &mut self.mode {
            *body = mapper.get_mapped(*body);
        }
        if let Some(grapple) = &mut self.grapple {
            grapple.target = mapper.get_mapped(grapple.target);
        }
    }
}

impl RobotView {
    /// The view of `player`, as the wire would carry it.
    pub fn of(player: &Player) -> Self {
        RobotView {
            mode: player.mode,
            grapple: player.grapple,
            boots_on: player.boots_on,
            beams: [player.loadout.hands[0].beam, player.loadout.hands[1].beam],
        }
        .quantised()
    }

    fn quantised(self) -> Self {
        WireView::from(self).into()
    }

    /// Whether `self` is `other` to within what the wire carries (and
    /// what anyone could see): a robot jittering in the last bits (boots
    /// re-seating on a turning rock) isn't re-sent.
    pub fn near(&self, other: &RobotView) -> bool {
        near_mode(&self.mode, &other.mode)
            && self.boots_on == other.boots_on
            && self.beams == other.beams
            && match (self.grapple, other.grapple) {
                (None, None) => true,
                (Some(a), Some(b)) => {
                    a.target == b.target
                        && a.anchor.distance(b.anchor) < AT
                        && (a.length - b.length).abs() < 0.01
                        && (a.tension - b.tension).abs() <= 1.0f32.max(0.05 * b.tension.abs())
                }
                _ => false,
            }
    }
}

/// What two modes may differ by and still be the same on the wire.
const AT: f32 = 2e-3;
const SPEED: f32 = 2e-2;
const TURN: f32 = 1e-3;

fn near_mode(a: &Mode, b: &Mode) -> bool {
    let turned = |p: Quat, q: Quat| rotation_error(p, q) < TURN;
    match (*a, *b) {
        (
            Mode::Booted { body: b0, feet: f0, up: u0, velocity: v0, orientation: o0, pitch: p0 },
            Mode::Booted { body: b1, feet: f1, up: u1, velocity: v1, orientation: o1, pitch: p1 },
        ) => {
            b0 == b1
                && f0.distance(f1) < AT
                && u0.angle_between(u1) < TURN
                && v0.distance(v1) < SPEED
                && turned(o0, o1)
                && (p0 - p1).abs() < TURN
        }
        (
            Mode::Eva { position: x0, velocity: v0, orientation: o0, pitch: p0 },
            Mode::Eva { position: x1, velocity: v1, orientation: o1, pitch: p1 },
        ) => x0.distance(x1) < AT && v0.distance(v1) < SPEED && turned(o0, o1) && (p0 - p1).abs() < TURN,
        _ => false,
    }
}

/// A robot's recent events, for a remote client to show (see the module
/// docs). The log's tick is the tick it was written on.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[component(map_entities)]
pub struct RobotLog(pub EventLog);

impl MapEntities for RobotLog {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        self.0.map_entities(mapper);
    }
}

/// How far back a [`RobotLog`] reaches (ticks): further than presentation
/// shows a late event (`client::events`), so a lost update's entries come
/// in the next.
pub const ROBOT_LOG_TICKS: u32 = 30;

/// Authority, after players move: each robot's view and log, rewritten
/// only when they've changed by more than the wire would show.
pub fn publish_robots(
    mut robots: Query<(Entity, &Player, Option<&mut RobotView>, Option<&mut RobotLog>), With<Robot>>,
    mut commands: Commands,
) {
    for (entity, player, view, log) in &mut robots {
        let (Some(mut view), Some(mut log)) = (view, log) else {
            commands.entity(entity).insert((RobotView::of(player), RobotLog(player.events.since(ROBOT_LOG_TICKS))));
            continue;
        };
        let now = RobotView::of(player);
        if !now.near(&view) {
            *view = now;
        }
        if player.events.last() != log.0.last() {
            log.0 = player.events.since(ROBOT_LOG_TICKS);
        }
    }
}

/// A remote client, as the interpolation is written: each robot's
/// `Player`, from its view and log, for presentation (and aiming) to read.
/// What a client doesn't get (the loadout but for its beams, the input
/// cursor) stays at rest.
pub fn present_robots(
    mut robots: Query<(Entity, &RobotView, Option<&RobotLog>, Option<&mut Player>), With<Robot>>,
    mut commands: Commands,
) {
    for (entity, view, log, player) in &mut robots {
        let Some(mut player) = player else {
            let mut loadout = Loadout::default();
            for (hand, beam) in loadout.hands.iter_mut().zip(view.beams) {
                hand.beam = beam;
            }
            commands.entity(entity).insert(Player {
                mode: view.mode,
                boots_on: view.boots_on,
                flight_assist: false,
                grapple: view.grapple,
                reference_body: view.mode.body(),
                detach_timer: 0.0,
                loadout,
                input: default(),
                events: log.map(|l| l.0.clone()).unwrap_or_default(),
            });
            continue;
        };
        let player = &mut *player;
        player.mode = view.mode;
        player.boots_on = view.boots_on;
        player.grapple = view.grapple;
        player.reference_body = view.mode.body();
        for (hand, beam) in player.loadout.hands.iter_mut().zip(view.beams) {
            hand.beam = beam;
        }
        if let Some(log) = log
            && log.0 != player.events
        {
            player.events = log.0.clone();
        }
    }
}

/// Interpolates a robot's view as a player's is (`interpolate_mode`); the
/// rest is taken from the nearer end.
pub fn interpolate_view(start: RobotView, end: RobotView, t: f32) -> RobotView {
    let mode = super::interpolate_mode(start.mode, end.mode, t);
    let mut view = if t < 0.5 { start } else { end };
    view.mode = mode;
    view
}

/// The nearer end: what can't be blended.
pub fn snap<C>(start: C, end: C, t: f32) -> C {
    if t < 0.5 { start } else { end }
}

// The wire.

const MM: f32 = 1000.0;
const CM_S: f32 = 100.0;
const UNIT: f32 = 32767.0;
const PITCH: f32 = 10000.0;

fn q3(v: Vec3, scale: f32) -> [i32; 3] {
    // Float to int casts saturate.
    [(v.x * scale).round() as i32, (v.y * scale).round() as i32, (v.z * scale).round() as i32]
}

fn d3(v: [i32; 3], scale: f32) -> Vec3 {
    Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32) / scale
}

fn qunit(v: Vec3) -> [i16; 3] {
    [(v.x * UNIT).round() as i16, (v.y * UNIT).round() as i16, (v.z * UNIT).round() as i16]
}

fn dunit(v: [i16; 3]) -> Vec3 {
    Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32).normalize_or(Vec3::Y)
}

fn qquat(q: Quat) -> [i16; 4] {
    let q = q.normalize();
    [(q.x * UNIT).round() as i16, (q.y * UNIT).round() as i16, (q.z * UNIT).round() as i16, (q.w * UNIT).round() as i16]
}

fn dquat(q: [i16; 4]) -> Quat {
    let q = Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32);
    if q.length_squared() > 0.0 { q.normalize() } else { Quat::IDENTITY }
}

fn qpitch(p: f32) -> i16 {
    (p * PITCH).round() as i16
}

fn dpitch(p: i16) -> f32 {
    p as f32 / PITCH
}

#[derive(Clone, Serialize, Deserialize)]
enum WireMode {
    Eva {
        position: [i32; 3],
        velocity: [i32; 3],
        orientation: [i16; 4],
        pitch: i16,
    },
    Booted {
        body: Entity,
        feet: [i32; 3],
        up: [i16; 3],
        velocity: [i32; 3],
        orientation: [i16; 4],
        pitch: i16,
    },
}

#[derive(Clone, Serialize, Deserialize)]
struct WireGrapple {
    target: Entity,
    anchor: [i32; 3],
    /// Millimetres.
    length: i32,
    /// Newtons.
    tension: i32,
}

#[derive(Clone, Serialize, Deserialize)]
struct WireView {
    mode: WireMode,
    grapple: Option<WireGrapple>,
    boots_on: bool,
    beams: [Option<Beam>; 2],
}

impl From<RobotView> for WireView {
    fn from(v: RobotView) -> Self {
        let mode = match v.mode {
            Mode::Eva { position, velocity, orientation, pitch } => WireMode::Eva {
                position: q3(position, MM),
                velocity: q3(velocity, CM_S),
                orientation: qquat(orientation),
                pitch: qpitch(pitch),
            },
            Mode::Booted { body, feet, up, velocity, orientation, pitch } => WireMode::Booted {
                body,
                feet: q3(feet, MM),
                up: qunit(up),
                velocity: q3(velocity, CM_S),
                orientation: qquat(orientation),
                pitch: qpitch(pitch),
            },
        };
        WireView {
            mode,
            grapple: v.grapple.map(|g| WireGrapple {
                target: g.target,
                anchor: q3(g.anchor, MM),
                length: (g.length * MM).round() as i32,
                tension: g.tension.round() as i32,
            }),
            boots_on: v.boots_on,
            beams: v.beams,
        }
    }
}

impl From<WireView> for RobotView {
    fn from(w: WireView) -> Self {
        let mode = match w.mode {
            WireMode::Eva { position, velocity, orientation, pitch } => Mode::Eva {
                position: d3(position, MM),
                velocity: d3(velocity, CM_S),
                orientation: dquat(orientation),
                pitch: dpitch(pitch),
            },
            WireMode::Booted { body, feet, up, velocity, orientation, pitch } => Mode::Booted {
                body,
                feet: d3(feet, MM),
                up: dunit(up),
                velocity: d3(velocity, CM_S),
                orientation: dquat(orientation),
                pitch: dpitch(pitch),
            },
        };
        RobotView {
            mode,
            grapple: w.grapple.map(|g| Grapple {
                target: g.target,
                anchor: d3(g.anchor, MM),
                length: g.length as f32 / MM,
                tension: g.tension as f32,
            }),
            boots_on: w.boots_on,
            beams: w.beams,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn booted(feet: Vec3) -> RobotView {
        RobotView {
            mode: Mode::Booted {
                body: Entity::from_raw_u32(7).unwrap(),
                feet,
                up: Vec3::new(0.3, 0.9, 0.1).normalize(),
                velocity: Vec3::new(1.234, -0.5, 0.0),
                orientation: Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.1),
                pitch: -0.4,
            },
            grapple: Some(Grapple {
                target: Entity::from_raw_u32(9).unwrap(),
                anchor: Vec3::new(-1.5, 0.25, 3.0),
                length: 10.5,
                tension: 42.4,
            }),
            boots_on: true,
            beams: [None, None],
        }
    }

    #[test]
    fn the_wire_keeps_a_pose_to_within_what_shows() {
        let v = booted(Vec3::new(12.3456, -80.0, 0.0004));
        let q = v.clone().quantised();
        assert!(q.near(&v), "{q:?} vs {v:?}");
        // And quantising again changes nothing: the authority stores what
        // the client gets.
        assert_eq!(q.clone().quantised(), q);
    }

    #[test]
    fn a_jitter_below_a_millimetre_is_not_a_change_but_a_step_is() {
        let v = booted(Vec3::new(1.0, 2.0, 3.0)).quantised();
        let mut player_side = booted(Vec3::new(1.0003, 2.0, 3.0));
        assert!(player_side.clone().quantised().near(&v));
        player_side = booted(Vec3::new(1.01, 2.0, 3.0));
        assert!(!player_side.quantised().near(&v));
    }

    #[test]
    fn a_view_is_smaller_on_the_wire_than_a_player() {
        let v = booted(Vec3::new(12.3456, -80.0, 0.5));
        let view = postcard::to_allocvec(&v).unwrap().len();
        let player = Player {
            mode: v.mode,
            boots_on: true,
            flight_assist: true,
            grapple: v.grapple,
            reference_body: v.mode.body(),
            detach_timer: 0.0,
            loadout: Loadout::default(),
            input: default(),
            events: EventLog::default(),
        };
        let whole = postcard::to_allocvec(&player).unwrap().len();
        eprintln!("on the wire: a robot's view {view} B, its player {whole} B");
        assert!(view * 2 < whole, "view {view} B, player {whole} B");
    }
}
