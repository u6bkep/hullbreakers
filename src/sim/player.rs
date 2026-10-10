//! Player movement: mag boots, EVA jetpack, and grapple.
//!
//! The player is not an Avian rigid body. It is a kinematic capsule driven by
//! Avian's spatial queries, in one of two modes:
//!
//! - **Booted**: simulated in the local frame of the body underfoot. Walking on
//!   a rolling ship is as stable as walking on the ground, because in this
//!   frame it is. The body's acceleration at the feet is checked against boot
//!   adhesion and shear; exceeding them rips the player off or makes them skid.
//! - **Eva**: simulated in world (inertial) coordinates with a jetpack.
//!
//! Every transition goes through [`BodyFrame`] so world momentum is continuous.

use std::time::Duration;

use avian3d::character_controller::move_and_slide::DepenetrationConfig;
use avian3d::prelude::*;
use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::SimTick;
use super::bodies::Bodies;
use super::events::{Detach, EventLog, PlayerEvent};
use super::frame::BodyFrame;
use super::input::{InputCursor, PlayerInput, TickInput};
use super::rope::{RopeEnd, solve_rope};
use super::tuning::Tuning;

pub const PITCH_LIMIT: f32 = 1.55;
const SKIN: f32 = 0.01;
/// How far below the surface the edge-wrap ray starts.
const WRAP_DEPTH: f32 = 0.08;
/// How quickly the EVA body turns to follow the head's pitch (1/s).
const BODY_FOLLOW_RATE: f32 = 3.0;
/// Fraction of the capsule (from the bottom) that counts as "feet".
const FEET_REGION: f32 = 0.3;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Mode {
    /// World-space. `position` is the capsule center; `orientation` is the
    /// body (capsule axis = local Y); the view is `orientation * pitch`.
    Eva {
        position: Vec3,
        velocity: Vec3,
        orientation: Quat,
        pitch: f32,
    },
    /// Body-local. `feet` is the bottom of the capsule, `up` the surface
    /// normal, `velocity` relative to the body. `orientation` keeps its Y axis
    /// equal to `up`.
    Booted {
        body: Entity,
        feet: Vec3,
        up: Vec3,
        velocity: Vec3,
        orientation: Quat,
        pitch: f32,
    },
}

impl Mode {
    pub fn body(&self) -> Option<Entity> {
        match self {
            Mode::Booted { body, .. } => Some(*body),
            Mode::Eva { .. } => None,
        }
    }

    /// Eye position and view rotation, in the frame the mode lives in
    /// (body-local for Booted, world for Eva).
    pub fn view(&self, tuning: &Tuning) -> (Vec3, Quat) {
        match *self {
            Mode::Eva {
                position,
                orientation,
                pitch,
                ..
            } => (
                position
                    + orientation * Vec3::Y * (tuning.eye_height - tuning.capsule_height / 2.0),
                orientation * Quat::from_rotation_x(pitch),
            ),
            Mode::Booted {
                feet,
                up,
                orientation,
                pitch,
                ..
            } => (
                feet + up * tuning.eye_height,
                orientation * Quat::from_rotation_x(pitch),
            ),
        }
    }

    /// Eye position and view rotation in world space.
    pub fn world_view(&self, tuning: &Tuning, frame: Option<&BodyFrame>) -> (Vec3, Quat) {
        let (eye, rot) = self.view(tuning);
        match (self, frame) {
            (Mode::Booted { .. }, Some(f)) => (f.point_to_world(eye), f.rotation * rot),
            _ => (eye, rot),
        }
    }

    /// World-space body center and velocity. `frame` must be the frame of
    /// the body underfoot when booted.
    pub fn world_kinematics(&self, tuning: &Tuning, frame: Option<&BodyFrame>) -> (Vec3, Vec3) {
        match (*self, frame) {
            (
                Mode::Booted {
                    feet, up, velocity, ..
                },
                Some(f),
            ) => {
                let center = feet + up * (tuning.capsule_height / 2.0);
                (
                    f.point_to_world(center),
                    f.velocity_to_world(center, velocity),
                )
            }
            (Mode::Booted { feet, velocity, .. }, None) => (feet, velocity),
            (
                Mode::Eva {
                    position, velocity, ..
                },
                _,
            ) => (position, velocity),
        }
    }

    /// Applies mouse look and roll. Used by the simulation each tick and by
    /// the camera to apply not-yet-simulated look for zero-latency aiming.
    pub fn apply_look(&mut self, look: Vec2, roll_angle: f32) {
        match self {
            Mode::Booted {
                up,
                orientation,
                pitch,
                ..
            } => {
                *orientation = (Quat::from_axis_angle(*up, -look.x) * *orientation).normalize();
                *pitch = (*pitch - look.y).clamp(-PITCH_LIMIT, PITCH_LIMIT);
            }
            Mode::Eva {
                orientation, pitch, ..
            } => {
                *orientation *= Quat::from_rotation_y(-look.x);
                let unclamped = *pitch - look.y;
                *pitch = unclamped.clamp(-PITCH_LIMIT, PITCH_LIMIT);
                // Looking past straight up/down tumbles the body instead.
                *orientation *= Quat::from_rotation_x(unclamped - *pitch);
                let view_axis = *orientation * Quat::from_rotation_x(*pitch) * Vec3::NEG_Z;
                *orientation =
                    (Quat::from_axis_angle(view_axis, roll_angle) * *orientation).normalize();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grapple {
    pub body: Entity,
    /// Anchor point in the body's local coordinates.
    pub anchor: Vec3,
    pub length: f32,
    /// Last solved rope tension in newtons, for display.
    pub tension: f32,
}

/// Everything about a player that the simulation carries from tick to tick.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[component(map_entities)]
pub struct Player {
    pub mode: Mode,
    pub boots_on: bool,
    /// Flight assist: movement input commands a velocity relative to the
    /// reference body instead of a raw thrust, and releasing the keys brakes.
    pub flight_assist: bool,
    pub grapple: Option<Grapple>,
    /// The body velocities are reported (and braked) relative to.
    pub reference_body: Option<Entity>,
    pub detach_timer: f32,
    /// How much of [`PlayerInput`] has been applied.
    pub input: InputCursor,
    pub events: EventLog,
}

impl MapEntities for Player {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        if let Mode::Booted { body, .. } = &mut self.mode {
            *body = mapper.get_mapped(*body);
        }
        if let Some(grapple) = &mut self.grapple {
            grapple.body = mapper.get_mapped(grapple.body);
        }
        if let Some(body) = &mut self.reference_body {
            *body = mapper.get_mapped(*body);
        }
        self.events.map_entities(mapper);
    }
}

impl Player {
    fn emit(&mut self, event: PlayerEvent) {
        self.events.push(event);
    }
}

/// Per-tick outputs derived from the simulation, for presentation. Not state:
/// recomputed every tick.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlayerTelemetry {
    /// Jetpack output as a fraction of base thrust (boost > 1).
    pub thrust: f32,
}

/// The mode at the start of the most recent tick, for render interpolation.
#[derive(Component, Clone, Copy, Debug)]
pub struct PreviousMode(pub Mode);

/// Spawns the one local player of a game without a network.
pub fn spawn_player(mut commands: Commands, ship: Single<Entity, With<super::ship::Ship>>) {
    commands.spawn(new_player(*ship, 0));
}

/// A player standing on `ship`'s deck. Players are spread across the deck by
/// `index` so they don't spawn inside each other.
pub fn new_player(ship: Entity, index: u32) -> impl Bundle {
    let column = [0.0, 1.2, -1.2, 2.4][index as usize % 4];
    let row = (index / 4) as f32;
    let mode = Mode::Booted {
        body: ship,
        feet: Vec3::new(column, SKIN, 5.0 + row * 1.5),
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    (
        Name::new(format!("Player {}", index + 1)),
        Player {
            mode,
            boots_on: true,
            flight_assist: true,
            grapple: None,
            reference_body: Some(ship),
            detach_timer: 0.0,
            input: InputCursor::default(),
            events: EventLog::default(),
        },
        PreviousMode(mode),
        PlayerInput::default(),
        PlayerTelemetry::default(),
    )
}

fn capsule(t: &Tuning) -> Collider {
    Collider::capsule(
        t.capsule_radius,
        (t.capsule_height - 2.0 * t.capsule_radius).max(0.01),
    )
}

/// Rotation whose -Z is `forward` and Y is `up` (forward need not be
/// perpendicular to up).
fn basis(forward: Vec3, up: Vec3) -> Quat {
    let right = forward.cross(up).normalize();
    let forward = up.cross(right);
    Quat::from_mat3(&Mat3::from_cols(right, up, -forward)).normalize()
}

/// Splits a view rotation into an upright body orientation (Y = `up`) and a
/// pitch, preserving the view direction where possible.
fn upright_from_view(view: Quat, up: Vec3) -> (Quat, f32) {
    let forward = view * Vec3::NEG_Z;
    let mut heading = forward - up * forward.dot(up);
    if heading.length_squared() < 0.01 {
        // Looking straight along the normal: the view's up becomes heading.
        let view_up = view * Vec3::Y * -forward.dot(up).signum();
        heading = view_up - up * view_up.dot(up);
    }
    let heading = heading.normalize();
    let pitch = forward
        .dot(up)
        .atan2(forward.dot(heading))
        .clamp(-PITCH_LIMIT, PITCH_LIMIT);
    (basis(heading, up), pitch)
}

/// Leaves the surface of `frame` with the given body-relative velocity.
fn booted_to_eva(
    frame: &BodyFrame,
    feet: Vec3,
    up: Vec3,
    velocity: Vec3,
    orientation: Quat,
    pitch: f32,
    t: &Tuning,
) -> Mode {
    let center = feet + up * (t.capsule_height / 2.0);
    Mode::Eva {
        position: frame.point_to_world(center),
        velocity: frame.velocity_to_world(center, velocity),
        orientation: frame.rotation * orientation,
        pitch,
    }
}

/// Lands on `frame` with feet at `contact` (world) and surface normal `normal` (world).
fn eva_to_booted(
    body: Entity,
    frame: &BodyFrame,
    contact: Vec3,
    normal: Vec3,
    orientation: Quat,
    pitch: f32,
) -> Mode {
    let up = frame.dir_to_local(normal).normalize();
    let view_local = frame.rotation.inverse() * orientation * Quat::from_rotation_x(pitch);
    let (orientation, pitch) = upright_from_view(view_local, up);
    Mode::Booted {
        body,
        feet: frame.point_to_local(contact) + up * SKIN,
        up,
        velocity: Vec3::ZERO,
        orientation,
        pitch,
    }
}

/// Re-expresses booted state in another body's frame (stepping from one body
/// onto another).
fn switch_frame(
    from: &BodyFrame,
    to: &BodyFrame,
    feet: Vec3,
    up: Vec3,
    velocity: Vec3,
    orientation: Quat,
) -> (Vec3, Vec3, Vec3, Quat) {
    let feet_w = from.point_to_world(feet);
    let vel_w = from.velocity_to_world(feet, velocity);
    let up_l = to.dir_to_local(from.dir_to_world(up));
    let vel_l = to.velocity_to_local(feet_w, vel_w);
    (
        to.point_to_local(feet_w),
        up_l,
        vel_l - up_l * vel_l.dot(up_l),
        to.rotation.inverse() * from.rotation * orientation,
    )
}

/// Deepest allowed overlap for a booted capsule after a transition. Avian's
/// depenetration ignores overlaps deeper than 0.5 m, so transitions must never
/// create them.
const MAX_TRANSITION_PENETRATION: f32 = 0.03;

/// Whether a booted capsule standing at `feet` with `up` (body-local) is clear
/// of geometry.
fn capsule_fits(ctx: &TickContext, frame: &BodyFrame, feet: Vec3, up: Vec3) -> bool {
    let half = ctx.tuning.capsule_height / 2.0;
    let mut deepest = 0.0f32;
    ctx.mas.intersections(
        &capsule(ctx.tuning),
        frame.point_to_world(feet + up * half),
        frame.rotation * Quat::from_rotation_arc(Vec3::Y, up),
        0.0,
        &ctx.filter,
        |_, contact, _| {
            deepest = deepest.max(contact.penetration);
            true
        },
    );
    deepest < MAX_TRANSITION_PENETRATION
}

/// Everything a single player tick needs from the world.
struct TickContext<'a, 'w, 's> {
    tuning: &'a Tuning,
    dt: f32,
    delta: Duration,
    mas: &'a MoveAndSlide<'w, 's>,
    filter: SpatialQueryFilter,
}

pub fn player_tick(
    time: Res<Time>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mas: MoveAndSlide,
    mut bodies: Bodies,
    mut players: Query<(
        &mut Player,
        &PlayerInput,
        &mut PreviousMode,
        &mut PlayerTelemetry,
    )>,
) {
    let ctx = TickContext {
        tuning: &tuning,
        dt: time.delta_secs(),
        delta: time.delta(),
        mas: &mas,
        filter: SpatialQueryFilter::default(),
    };
    if ctx.dt <= 0.0 {
        return;
    }
    for (mut player, input, mut previous, mut telemetry) in &mut players {
        let player = &mut *player;
        player.events.begin_tick(tick.0);
        let cmd = player.input.advance(input);
        player.detach_timer = (player.detach_timer - ctx.dt).max(0.0);
        telemetry.thrust = 0.0;
        if cmd.toggle_boots {
            player.boots_on = !player.boots_on;
            player.emit(PlayerEvent::BootsToggled(player.boots_on));
        }
        if cmd.toggle_assist {
            player.flight_assist = !player.flight_assist;
            player.emit(PlayerEvent::AssistToggled(player.flight_assist));
        }
        let roll = cmd.roll * tuning.roll_rate_deg.to_radians() * ctx.dt;
        player.mode.apply_look(cmd.look, roll);
        // Recorded after look so the camera interpolates movement only; look
        // is applied immediately (see `update_camera`).
        previous.0 = player.mode;
        if cmd.grapple {
            if player.grapple.take().is_none() {
                fire_grapple(&ctx, &bodies, player);
            } else {
                player.emit(PlayerEvent::GrappleReleased);
            }
        }
        if cmd.set_reference {
            match aim(&ctx, &bodies, player, REFERENCE_RANGE) {
                Some((body, _, distance)) => {
                    player.reference_body = Some(body);
                    player.emit(PlayerEvent::ReferenceSet { distance });
                }
                None => player.emit(PlayerEvent::NoReferenceTarget),
            }
        }
        if player
            .grapple
            .is_some_and(|g| bodies.frame(g.body).is_none())
        {
            player.grapple = None;
        }
        match player.mode {
            Mode::Booted { .. } => booted_tick(&ctx, &mut bodies, player, &cmd),
            Mode::Eva { .. } => telemetry.thrust = eva_tick(&ctx, &mut bodies, player, &cmd),
        }
    }
}

/// How far away a body can be picked as the reference frame.
const REFERENCE_RANGE: f32 = 2000.0;

/// The body under the crosshair: (body, world hit point, distance).
fn aim(
    ctx: &TickContext,
    bodies: &Bodies,
    player: &Player,
    range: f32,
) -> Option<(Entity, Vec3, f32)> {
    let frame = player.mode.body().and_then(|b| bodies.frame(b));
    let (eye, view) = player.mode.world_view(ctx.tuning, frame.as_ref());
    let dir = Dir3::new(view * Vec3::NEG_Z).ok()?;
    let hit = ctx
        .mas
        .spatial_query
        .cast_ray(eye, dir, range, true, &ctx.filter)?;
    let body = bodies.body_of_collider(hit.entity)?;
    Some((body, eye + dir * hit.distance, hit.distance))
}

fn fire_grapple(ctx: &TickContext, bodies: &Bodies, player: &mut Player) {
    let Some((body, point, distance)) = aim(ctx, bodies, player, ctx.tuning.grapple_range) else {
        player.emit(PlayerEvent::GrappleMissed);
        return;
    };
    let Some(body_frame) = bodies.frame(body) else {
        return;
    };
    player.grapple = Some(Grapple {
        body,
        anchor: body_frame.point_to_local(point),
        length: distance,
        tension: 0.0,
    });
    player.reference_body = Some(body);
    player.emit(PlayerEvent::GrappleAttached { distance, body });
}

/// Applies reel input: returns the impulse limit and, while reeling, the
/// winch speed. Paying out lengthens the rope directly.
fn reel(
    grapple: &mut Grapple,
    cmd: &TickInput,
    ctx: &TickContext,
    distance: f32,
) -> (f32, Option<f32>) {
    let t = ctx.tuning;
    if cmd.reel_in && grapple.length > MIN_ROPE {
        (t.reel_force * ctx.dt, Some(winch_speed(distance, t)))
    } else {
        if cmd.pay_out {
            grapple.length = (grapple.length + t.reel_speed * ctx.dt).min(t.grapple_range);
        }
        (t.rope_break_force * ctx.dt, None)
    }
}

/// The winch brings the player to rest this far from the anchor.
const WINCH_ARRIVE: f32 = 1.2;

/// Winch speed at `distance` from the anchor: full speed far out, easing off
/// so that the jetpack (flight assist) can stop you at `WINCH_ARRIVE` using
/// half its thrust, keeping the rest to damp the swing that reeling in spins
/// up (angular momentum is conserved as the radius shrinks).
fn winch_speed(distance: f32, t: &Tuning) -> f32 {
    (t.thrust_accel * (distance - WINCH_ARRIVE).max(0.0))
        .sqrt()
        .min(t.reel_speed)
}

/// The winch stops taking in rope at this length.
const MIN_ROPE: f32 = 0.5;

/// While reeling, the winch takes in slack as the ends approach. Must run
/// before solving, against the current distance, or the winch sees the slack
/// the player closed since last tick and disengages.
fn take_in_slack(grapple: &mut Grapple, winch: Option<f32>, distance: f32) {
    if winch.is_some() {
        grapple.length = grapple.length.min(distance).max(MIN_ROPE);
    }
}

fn booted_tick(ctx: &TickContext, bodies: &mut Bodies, player: &mut Player, cmd: &TickInput) {
    let t = ctx.tuning;
    let Mode::Booted {
        body,
        mut feet,
        mut up,
        mut velocity,
        mut orientation,
        pitch,
    } = player.mode
    else {
        return;
    };
    let Some(frame) = bodies.frame(body) else {
        // Body vanished: float free where we were, at rest.
        player.mode = Mode::Eva {
            position: feet,
            velocity: Vec3::ZERO,
            orientation,
            pitch,
        };
        return;
    };
    let previous_frame = bodies.previous_frame(body).unwrap_or(frame);

    let leave = move |player: &mut Player, velocity: Vec3, why: Detach| {
        player.mode = booted_to_eva(&frame, feet, up, velocity, orientation, pitch, t);
        player.detach_timer = t.detach_cooldown;
        player.emit(PlayerEvent::Detached(why));
    };

    if !player.boots_on {
        leave(player, velocity + up * 0.3, Detach::BootsOff);
        return;
    }
    if cmd.jump {
        leave(player, velocity + up * t.jump_speed, Detach::Jumped);
        return;
    }

    // Adhesion: the surface must drag us along with its acceleration.
    let feet_w = frame.point_to_world(feet);
    let accel = frame.dir_to_local(frame.point_acceleration(&previous_frame, ctx.dt, feet_w));
    let accel_normal = accel.dot(up);
    if -accel_normal > t.adhesion_accel {
        leave(player, velocity, Detach::RippedOff(-accel_normal));
        return;
    }

    // Rope tension against the boots.
    if let Some(mut grapple) = player.grapple {
        let center_l = feet + up * (t.capsule_height / 2.0);
        let center_w = frame.point_to_world(center_l);
        if grapple.body == body {
            // Anchored to the body we stand on: the rope just pays out.
            let anchor_w = frame.point_to_world(grapple.anchor);
            grapple.length = grapple
                .length
                .max(anchor_w.distance(center_w))
                .min(t.grapple_range);
            grapple.tension = 0.0;
        } else if let (Some(anchor_frame), Some(anchor_mass), Some(own_mass)) = (
            bodies.frame(grapple.body),
            bodies.mass(grapple.body),
            bodies.mass(body),
        ) {
            let anchor_w = anchor_frame.point_to_world(grapple.anchor);
            let (max_impulse, winch) = reel(&mut grapple, cmd, ctx, anchor_w.distance(center_w));
            take_in_slack(&mut grapple, winch, anchor_w.distance(center_w));
            let dir = (anchor_w - center_w).normalize_or_zero();
            let player_end = RopeEnd {
                position: center_w,
                velocity: frame.velocity_to_world(center_l, velocity),
                // Booted, we are part of the body underfoot.
                inverse_mass: own_mass.inverse_effective_mass(&frame, feet_w, dir),
            };
            let anchor_end = RopeEnd {
                position: anchor_w,
                velocity: anchor_frame.point_velocity(anchor_w),
                inverse_mass: anchor_mass.inverse_effective_mass(&anchor_frame, anchor_w, dir),
            };
            grapple.tension = 0.0;
            if let Some(imp) = solve_rope(
                player_end,
                anchor_end,
                grapple.length,
                ctx.dt,
                t.rope_stiffness,
                max_impulse,
                winch,
            ) {
                let tension = imp.magnitude / ctx.dt;
                grapple.tension = tension;
                // Split the pull into peel-off (against adhesion) and drag
                // along the surface (against shear). Pulling into the surface is free.
                let pull = frame.dir_to_local(imp.direction) * tension;
                let peel = pull.dot(up);
                let drag = (pull - up * peel).length();
                if peel > t.player_mass * t.adhesion_accel || drag > t.player_mass * t.shear_accel {
                    // The rope beats the boots. Solve it as EVA next tick.
                    player.grapple = Some(grapple);
                    leave(player, velocity, Detach::Yanked(tension));
                    return;
                }
                if imp.clamped && winch.is_none() {
                    player.grapple = None;
                    player.emit(PlayerEvent::RopeSnapped);
                }
                bodies.apply_impulse(grapple.body, -imp.direction * imp.magnitude, anchor_w);
                bodies.apply_impulse(body, imp.direction * imp.magnitude, feet_w);
            }
        }
        if player.grapple.is_some() {
            player.grapple = Some(grapple);
        }
    }

    // Shear: tangential frame acceleration beyond what the boots grip makes us skid.
    let accel_tangent = accel - up * accel_normal;
    let excess = accel_tangent.length() - t.shear_accel;
    let skidding = excess > 0.0;

    // Walking. While skidding the boots have no grip to spare for it.
    let heading = (orientation * Vec3::NEG_Z).normalize();
    let right = orientation * Vec3::X;
    let wish_dir = if skidding {
        Vec3::ZERO
    } else {
        (heading * cmd.movement.z + right * cmd.movement.x).clamp_length_max(1.0)
    };
    let speed = if cmd.sprint {
        t.sprint_speed
    } else {
        t.walk_speed
    };
    let wish = wish_dir * speed;
    velocity -= up * velocity.dot(up);
    if skidding {
        velocity -= accel_tangent.normalize() * excess * ctx.dt;
    } else {
        velocity = velocity.move_towards(wish, t.walk_accel * ctx.dt);
    }

    // Move in a frozen snapshot of the body's frame.
    let shape = capsule(t);
    let half = t.capsule_height / 2.0;
    let capsule_rot = |up: Vec3| frame.rotation * Quat::from_rotation_arc(Vec3::Y, up);
    let start_feet = feet;
    let mut hits: Vec<(Entity, Vec3, Vec3)> = Vec::new();
    let out = ctx.mas.move_and_slide(
        &shape,
        frame.point_to_world(feet + up * half),
        capsule_rot(up),
        frame.dir_to_world(velocity),
        ctx.delta,
        &MoveAndSlideConfig::default(),
        &ctx.filter,
        |hit| {
            hits.push((hit.entity, hit.normal.as_vec3(), hit.point));
            MoveAndSlideHitResponse::Accept
        },
    );
    feet = frame.point_to_local(out.position) - up * half;
    velocity = frame.dir_to_local(out.projected_velocity);
    velocity -= up * velocity.dot(up);

    // Walking straight into a wall climbs onto it. A wall is steeper than
    // walkable ground, which the ground snap below leans onto instead.
    let wish_w = frame.dir_to_world(wish_dir);
    let up_w = frame.dir_to_world(up);
    let max_cos = t.max_ground_angle_deg.to_radians().cos();
    let wall = hits
        .iter()
        .filter(|(_, n, _)| n.dot(up_w).abs() < max_cos)
        .filter(|(_, n, _)| wish_w.dot(-*n) > 0.7)
        .max_by(|a, b| wish_w.dot(-a.1).total_cmp(&wish_w.dot(-b.1)))
        .copied();
    if t.wall_walk && wish_dir.length() > 0.5 {
        if let Some((entity, normal_w, point_w)) = wall {
            let new_up = frame.dir_to_local(normal_w);
            let d = (feet - frame.point_to_local(point_w)).dot(new_up);
            let new_feet = feet - new_up * d + up * (t.capsule_radius + 0.02);
            if capsule_fits(ctx, &frame, new_feet, new_up) {
                let turn = Quat::from_rotation_arc(up, new_up);
                orientation = turn * orientation;
                velocity = turn * velocity;
                player.emit(PlayerEvent::SurfaceChanged { body });
                player.mode = Mode::Booted {
                    body,
                    feet: new_feet,
                    up: new_up,
                    velocity,
                    orientation,
                    pitch,
                };
                retarget_body(bodies, player, entity, &frame, t);
                return;
            }
        }
    }

    // Stick to the ground, following its normal.
    let center_w = frame.point_to_world(feet + up * half);
    let ground = ctx.mas.cast_move(
        &shape,
        center_w,
        capsule_rot(up),
        -up_w * t.ground_probe,
        SKIN,
        &ctx.filter,
    );
    if let Some(hit) = ground.filter(|h| h.normal1.dot(up_w) > max_cos) {
        let snapped_center = center_w - up_w * hit.distance;
        feet = frame.point_to_local(snapped_center) - up * half;
        // Lean toward the surface normal at a limited rate, so walking over
        // a faceted surface turns the body smoothly instead of snapping at
        // every face. Tilting pivots the capsule about the feet; only follow
        // the surface if the tilted capsule still fits.
        let target_up = frame.dir_to_local(hit.normal1).normalize();
        if up.dot(target_up) < 0.9999 {
            let follow = 1.0 - (-t.surface_follow_rate * ctx.dt).exp();
            let turn = Quat::IDENTITY.slerp(Quat::from_rotation_arc(up, target_up), follow);
            let new_up = (turn * up).normalize();
            if capsule_fits(ctx, &frame, feet, new_up) {
                orientation = (turn * orientation).normalize();
                velocity = turn * velocity;
                up = new_up;
            }
        }
        player.mode = Mode::Booted {
            body,
            feet,
            up,
            velocity,
            orientation,
            pitch,
        };
        retarget_body(bodies, player, hit.entity, &frame, t);
        return;
    }

    // No ground: we walked off an edge. Look back under the edge for the face
    // that wraps around it.
    let tangent = velocity.normalize_or_zero();
    if tangent != Vec3::ZERO {
        let origin_w = frame.point_to_world(feet - up * WRAP_DEPTH);
        let back = Dir3::new(frame.dir_to_world(-tangent)).unwrap();
        if let Some(hit) = ctx
            .mas
            .spatial_query
            .cast_ray(origin_w, back, 1.5, false, &ctx.filter)
        {
            let new_up = frame.dir_to_local(hit.normal).normalize();
            let new_feet = frame.point_to_local(origin_w + back * hit.distance) + new_up * SKIN;
            if hit.normal.dot(frame.dir_to_world(tangent)) > 0.3
                && capsule_fits(ctx, &frame, new_feet, new_up)
            {
                let turn = Quat::from_rotation_arc(up, new_up);
                orientation = turn * orientation;
                velocity = turn * velocity;
                player.emit(PlayerEvent::SurfaceChanged { body });
                player.mode = Mode::Booted {
                    body,
                    feet: new_feet,
                    up: new_up,
                    velocity,
                    orientation,
                    pitch,
                };
                retarget_body(bodies, player, hit.entity, &frame, t);
                return;
            }
        }
    }

    // Nowhere to put a foot: the boots refuse to step into the void.
    player.mode = Mode::Booted {
        body,
        feet: start_feet,
        up,
        velocity: Vec3::ZERO,
        orientation,
        pitch,
    };
}

/// If the surface we just stepped onto belongs to another body moving slowly
/// enough relative to the current one, move into its frame. A fast-moving
/// surface is not something boots can step onto; it is an obstacle.
fn retarget_body(
    bodies: &Bodies,
    player: &mut Player,
    collider: Entity,
    from: &BodyFrame,
    t: &Tuning,
) {
    let Mode::Booted {
        body,
        feet,
        up,
        velocity,
        orientation,
        pitch,
    } = player.mode
    else {
        return;
    };
    let Some(new_body) = bodies.body_of_collider(collider) else {
        return;
    };
    if new_body == body {
        return;
    }
    let Some(to) = bodies.frame(new_body) else {
        return;
    };
    let feet_w = from.point_to_world(feet);
    if (from.point_velocity(feet_w) - to.point_velocity(feet_w)).length() > t.attach_max_speed {
        return;
    }
    let (feet, up, velocity, orientation) =
        switch_frame(from, &to, feet, up, velocity, orientation);
    player.mode = Mode::Booted {
        body: new_body,
        feet,
        up,
        velocity,
        orientation,
        pitch,
    };
    player.reference_body = Some(new_body);
    player.emit(PlayerEvent::BodyChanged { body: new_body });
}

/// Returns the jetpack output as a fraction of base thrust.
fn eva_tick(ctx: &TickContext, bodies: &mut Bodies, player: &mut Player, cmd: &TickInput) -> f32 {
    let t = ctx.tuning;
    let Mode::Eva {
        mut position,
        mut velocity,
        mut orientation,
        mut pitch,
    } = player.mode
    else {
        return 0.0;
    };
    let inverse_mass = 1.0 / t.player_mass;

    // The body slowly turns to follow where the head is looking.
    let follow = pitch * (1.0 - (-BODY_FOLLOW_RATE * ctx.dt).exp());
    orientation = (orientation * Quat::from_rotation_x(follow)).normalize();
    pitch -= follow;
    let view = orientation * Quat::from_rotation_x(pitch);

    // Jetpack. Thrust is always limited to the jetpack's real acceleration;
    // flight assist only chooses the thrust direction.
    let input =
        (view * Vec3::new(cmd.movement.x, cmd.movement.y, -cmd.movement.z)).clamp_length_max(1.0);
    let boost = if cmd.sprint { t.boost_multiplier } else { 1.0 };
    let max_dv = t.thrust_accel * boost * ctx.dt;
    let reference_velocity = player
        .reference_body
        .and_then(|b| {
            bodies.matched_velocity(b, position, t.match_surface_near, t.match_surface_far)
        })
        .unwrap_or(Vec3::ZERO);
    // Reeling in: (anchor point, anchor velocity) of the rope being wound.
    let reeling_to = player.grapple.filter(|_| cmd.reel_in).and_then(|g| {
        bodies.frame(g.body).map(|f| {
            let anchor = f.point_to_world(g.anchor);
            (anchor, f.point_velocity(anchor))
        })
    });
    let swinging = !cmd.reel_in && player.grapple.is_some_and(|g| g.tension > 0.0);
    let thrust_dv = if cmd.brake {
        (reference_velocity - velocity).clamp_length_max(t.brake_accel * ctx.dt)
    } else if player.flight_assist && !(swinging && input == Vec3::ZERO) {
        // Steer toward the commanded velocity relative to the reference
        // body. With no input that is "hold still", except: while swinging
        // on a taut rope the assist stays out of the way, and while reeling
        // it flies with the winch (toward the anchor at the winch's speed),
        // damping sideways swing and stopping you short of the anchor.
        let target = match reeling_to {
            Some((anchor, anchor_velocity)) if input == Vec3::ZERO => {
                let offset = anchor - position;
                anchor_velocity + offset.normalize_or_zero() * winch_speed(offset.length(), t)
            }
            _ => reference_velocity + input * t.assist_max_speed * boost,
        };
        (target - velocity).clamp_length_max(max_dv)
    } else {
        input * max_dv
    };
    velocity += thrust_dv;
    let thrust = thrust_dv.length() / (t.thrust_accel * ctx.dt);

    // Rope.
    if let Some(mut grapple) = player.grapple {
        if let (Some(frame), Some(mass)) = (bodies.frame(grapple.body), bodies.mass(grapple.body)) {
            let anchor_w = frame.point_to_world(grapple.anchor);
            let (max_impulse, winch) = reel(&mut grapple, cmd, ctx, anchor_w.distance(position));
            take_in_slack(&mut grapple, winch, anchor_w.distance(position));
            let dir = (anchor_w - position).normalize_or_zero();
            let player_end = RopeEnd {
                position,
                velocity,
                inverse_mass,
            };
            let anchor_end = RopeEnd {
                position: anchor_w,
                velocity: frame.point_velocity(anchor_w),
                inverse_mass: mass.inverse_effective_mass(&frame, anchor_w, dir),
            };
            grapple.tension = 0.0;
            let mut snapped = false;
            if let Some(imp) = solve_rope(
                player_end,
                anchor_end,
                grapple.length,
                ctx.dt,
                t.rope_stiffness,
                max_impulse,
                winch,
            ) {
                grapple.tension = imp.magnitude / ctx.dt;
                velocity += imp.direction * imp.magnitude * inverse_mass;
                bodies.apply_impulse(grapple.body, -imp.direction * imp.magnitude, anchor_w);
                // A force-limited winch saturating is normal; a rope is not.
                snapped = imp.clamped && winch.is_none();
            }
            player.grapple = if snapped {
                player.emit(PlayerEvent::RopeSnapped);
                None
            } else {
                Some(grapple)
            };
        }
    }

    let shape = capsule(t);
    let axis = orientation * Vec3::Y;

    // Bodies that moved into us during the physics step.
    let mut contacts: Vec<(Entity, Vec3, Vec3)> = Vec::new();
    let mut intrusions: Vec<(Dir3, f32)> = Vec::new();
    ctx.mas.intersections(
        &shape,
        position,
        orientation,
        SKIN,
        &ctx.filter,
        |entity, contact, normal| {
            contacts.push((entity, normal.as_vec3(), contact.point));
            intrusions.push((normal, contact.penetration + SKIN));
            true
        },
    );
    position += ctx
        .mas
        .depenetrate_intersections(&DepenetrationConfig::default(), &intrusions);

    let out = ctx.mas.move_and_slide(
        &shape,
        position,
        orientation,
        velocity,
        ctx.delta,
        &MoveAndSlideConfig::default(),
        &ctx.filter,
        |hit| {
            contacts.push((hit.entity, hit.normal.as_vec3(), hit.point));
            MoveAndSlideHitResponse::Accept
        },
    );
    position = out.position;

    // Resolve contacts against the (possibly moving) bodies we touched.
    let mut resolved: Vec<Entity> = Vec::new();
    for (collider, normal, point) in contacts {
        let Some(body) = bodies.body_of_collider(collider) else {
            continue;
        };
        let (Some(frame), Some(mass)) = (bodies.frame(body), bodies.mass(body)) else {
            continue;
        };
        let relative = velocity - frame.point_velocity(point);
        let closing = relative.dot(normal);

        let feet_contact =
            (point - position).dot(axis) < -(t.capsule_height / 2.0) * (1.0 - 2.0 * FEET_REGION);
        let upright = axis.dot(normal) > t.attach_max_angle_deg.to_radians().cos();
        if player.boots_on
            && player.detach_timer <= 0.0
            && feet_contact
            && upright
            && relative.length() < t.attach_max_speed
        {
            let feet_point = position - axis * (t.capsule_height / 2.0);
            // Project the feet onto the contact plane.
            let contact = feet_point - normal * (feet_point - point).dot(normal);
            let landed = eva_to_booted(body, &frame, contact, normal, orientation, pitch);
            if let Mode::Booted { feet, up, .. } = landed
                && capsule_fits(ctx, &frame, feet, up)
            {
                player.mode = landed;
                player.reference_body = Some(body);
                player.emit(PlayerEvent::Landed {
                    speed: relative.length(),
                    body,
                });
                return thrust;
            }
        }
        // One bounce per body per tick; the attach test above sees every contact.
        if closing >= 0.0 || resolved.contains(&body) {
            continue;
        }
        resolved.push(body);
        let inverse_body = mass.inverse_effective_mass(&frame, point, normal);
        let normal_impulse =
            -(1.0 + t.bounce_restitution) * closing / (inverse_mass + inverse_body);
        let tangential = relative - normal * closing;
        let max_friction = t.bounce_friction * normal_impulse;
        let friction = if tangential.length() > 1e-4 {
            let stop = tangential.length() / (inverse_mass + inverse_body);
            -tangential.normalize() * stop.min(max_friction)
        } else {
            Vec3::ZERO
        };
        let impulse = normal * normal_impulse + friction;
        velocity += impulse * inverse_mass;
        bodies.apply_impulse(body, -impulse, point);
        if -closing > 0.5 {
            player.emit(PlayerEvent::Impact {
                speed: -closing,
                body,
            });
        }
    }

    player.mode = Mode::Eva {
        position,
        velocity,
        orientation,
        pitch,
    };
    thrust
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_roll_is_clockwise() {
        // E (positive roll) dips the view's right side, like a clockwise turn of the head.
        let mut mode = Mode::Eva {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
        mode.apply_look(Vec2::ZERO, 0.2);
        let Mode::Eva { orientation, .. } = mode else {
            unreachable!()
        };
        assert!((orientation * Vec3::X).y < -0.1);
    }
}
