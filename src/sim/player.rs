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
use bevy::prelude::*;

use super::bodies::Bodies;
use super::frame::BodyFrame;
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

/// One tick of player intent. Continuous fields are overwritten by the input
/// layer each frame; `look` accumulates; edge-triggered flags are cleared once
/// the simulation has consumed them.
#[derive(Component, Clone, Debug, Default)]
pub struct PlayerCommand {
    /// Accumulated mouse look in radians: x = yaw right, y = pitch down.
    pub look: Vec2,
    /// -1..1, positive rolls clockwise.
    pub roll: f32,
    /// x = right, y = up, z = forward, each -1..1.
    pub movement: Vec3,
    pub sprint: bool,
    pub brake: bool,
    pub reel_in: bool,
    pub pay_out: bool,
    pub jump: bool,
    pub toggle_boots: bool,
    pub grapple: bool,
}

impl PlayerCommand {
    fn consume_edges(&mut self) -> Self {
        let taken = self.clone();
        self.look = Vec2::ZERO;
        self.jump = false;
        self.toggle_boots = false;
        self.grapple = false;
        taken
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
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
                    (Quat::from_axis_angle(view_axis, -roll_angle) * *orientation).normalize();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grapple {
    pub body: Entity,
    /// Anchor point in the body's local coordinates.
    pub anchor: Vec3,
    pub length: f32,
    /// Last solved rope tension in newtons, for display.
    pub tension: f32,
}

#[derive(Component, Clone, Debug)]
pub struct Player {
    pub mode: Mode,
    pub boots_on: bool,
    pub fuel: f32,
    pub grapple: Option<Grapple>,
    /// The body velocities are reported (and braked) relative to.
    pub reference_body: Option<Entity>,
    pub detach_timer: f32,
    /// Incremented whenever the view jumps discontinuously (landing, edge
    /// wraps, wall transitions) so the camera can smooth over it.
    pub discontinuity: u32,
    pub last_event: String,
}

/// The mode at the start of the most recent tick, for render interpolation.
#[derive(Component, Clone, Copy, Debug)]
pub struct PreviousMode(pub Mode);

pub fn spawn_player(
    mut commands: Commands,
    tuning: Res<Tuning>,
    ship: Single<Entity, With<super::ship::Ship>>,
) {
    let mode = Mode::Booted {
        body: *ship,
        feet: Vec3::new(0.0, SKIN, 5.0),
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    commands.spawn((
        Name::new("Player"),
        Player {
            mode,
            boots_on: true,
            fuel: tuning.fuel_capacity,
            grapple: None,
            reference_body: Some(*ship),
            detach_timer: 0.0,
            discontinuity: 0,
            last_event: String::new(),
        },
        PreviousMode(mode),
        PlayerCommand::default(),
    ));
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
    tuning: Res<Tuning>,
    mas: MoveAndSlide,
    mut bodies: Bodies,
    mut players: Query<(&mut Player, &mut PlayerCommand, &mut PreviousMode)>,
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
    for (mut player, mut command, mut previous) in &mut players {
        previous.0 = player.mode;
        let cmd = command.consume_edges();
        let player = &mut *player;
        player.detach_timer -= ctx.dt;
        if cmd.toggle_boots {
            player.boots_on = !player.boots_on;
        }
        let roll = cmd.roll * tuning.roll_rate_deg.to_radians() * ctx.dt;
        player.mode.apply_look(cmd.look, roll);
        if cmd.grapple {
            if player.grapple.take().is_none() {
                fire_grapple(&ctx, &bodies, player);
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
            Mode::Eva { .. } => eva_tick(&ctx, &mut bodies, player, &cmd),
        }
        player.fuel = player.fuel.min(tuning.fuel_capacity);
    }
}

fn fire_grapple(ctx: &TickContext, bodies: &Bodies, player: &mut Player) {
    let frame = player.mode.body().and_then(|b| bodies.frame(b));
    let (eye, view) = player.mode.world_view(ctx.tuning, frame.as_ref());
    let Ok(dir) = Dir3::new(view * Vec3::NEG_Z) else {
        return;
    };
    let Some(hit) =
        ctx.mas
            .spatial_query
            .cast_ray(eye, dir, ctx.tuning.grapple_range, true, &ctx.filter)
    else {
        player.last_event = "grapple missed".into();
        return;
    };
    let Some(body) = bodies.body_of_collider(hit.entity) else {
        return;
    };
    let Some(body_frame) = bodies.frame(body) else {
        return;
    };
    let point = eye + dir * hit.distance;
    player.grapple = Some(Grapple {
        body,
        anchor: body_frame.point_to_local(point),
        length: hit.distance,
        tension: 0.0,
    });
    player.reference_body = Some(body);
    player.last_event = format!("grappled at {:.1} m", hit.distance);
}

/// Updates rope length from reel input; returns the winch's impulse limit.
fn reel(grapple: &mut Grapple, cmd: &PlayerCommand, ctx: &TickContext) -> (f32, bool) {
    let t = ctx.tuning;
    if cmd.reel_in {
        grapple.length = (grapple.length - t.reel_speed * ctx.dt).max(0.5);
        (t.reel_force * ctx.dt, true)
    } else {
        if cmd.pay_out {
            grapple.length = (grapple.length + t.reel_speed * ctx.dt).min(t.grapple_range);
        }
        (t.rope_break_force * ctx.dt, false)
    }
}

fn booted_tick(ctx: &TickContext, bodies: &mut Bodies, player: &mut Player, cmd: &PlayerCommand) {
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
    player.reference_body = Some(body);
    player.fuel += t.refuel_rate * ctx.dt;

    let leave = move |player: &mut Player, velocity: Vec3, event: &str| {
        player.mode = booted_to_eva(&frame, feet, up, velocity, orientation, pitch, t);
        player.detach_timer = t.detach_cooldown;
        player.last_event = event.into();
    };

    if !player.boots_on {
        leave(player, velocity + up * 0.3, "boots off");
        return;
    }
    if cmd.jump {
        leave(player, velocity + up * t.jump_speed, "jumped");
        return;
    }

    // Adhesion: the surface must drag us along with its acceleration.
    let feet_w = frame.point_to_world(feet);
    let accel = frame.dir_to_local(frame.point_acceleration(&previous_frame, ctx.dt, feet_w));
    let accel_normal = accel.dot(up);
    if -accel_normal > t.adhesion_accel {
        leave(
            player,
            velocity,
            &format!("ripped off ({:.1} m/s²)", -accel_normal),
        );
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
            let (max_impulse, reeling) = reel(&mut grapple, cmd, ctx);
            let anchor_w = anchor_frame.point_to_world(grapple.anchor);
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
            ) {
                let tension = imp.magnitude / ctx.dt;
                grapple.tension = tension;
                if tension > t.player_mass * t.adhesion_accel {
                    // The rope pulls harder than the boots hold. Solve it as EVA next tick.
                    player.grapple = Some(grapple);
                    leave(
                        player,
                        velocity,
                        &format!("yanked off by rope ({:.0} N)", tension),
                    );
                    return;
                }
                if imp.clamped && reeling {
                    grapple.length = grapple.length.max(anchor_w.distance(center_w));
                } else if imp.clamped {
                    player.grapple = None;
                    player.last_event = "rope snapped".into();
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
    if excess > 0.0 {
        velocity -= accel_tangent.normalize() * excess * ctx.dt;
    }

    // Walking.
    let heading = (orientation * Vec3::NEG_Z).normalize();
    let right = orientation * Vec3::X;
    let wish_dir = (heading * cmd.movement.z + right * cmd.movement.x).clamp_length_max(1.0);
    let speed = if cmd.sprint {
        t.sprint_speed
    } else {
        t.walk_speed
    };
    let wish = wish_dir * speed;
    velocity -= up * velocity.dot(up);
    velocity = velocity.move_towards(wish, t.walk_accel * ctx.dt);

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

    // Walking straight into a wall climbs onto it.
    let wish_w = frame.dir_to_world(wish_dir);
    let up_w = frame.dir_to_world(up);
    let wall = hits
        .iter()
        .filter(|(_, n, _)| n.dot(up_w).abs() < 0.77)
        .filter(|(_, n, _)| wish_w.dot(-*n) > 0.7)
        .max_by(|a, b| wish_w.dot(-a.1).total_cmp(&wish_w.dot(-b.1)))
        .copied();
    if t.wall_walk && wish_dir.length() > 0.5 {
        if let Some((entity, normal_w, point_w)) = wall {
            let new_up = frame.dir_to_local(normal_w);
            let d = (feet - frame.point_to_local(point_w)).dot(new_up);
            let new_feet = feet - new_up * d + up * (t.capsule_radius + 0.02);
            let turn = Quat::from_rotation_arc(up, new_up);
            orientation = turn * orientation;
            velocity = turn * velocity;
            player.discontinuity += 1;
            player.mode = Mode::Booted {
                body,
                feet: new_feet,
                up: new_up,
                velocity,
                orientation,
                pitch,
            };
            retarget_body(bodies, player, entity, &frame);
            return;
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
    let max_cos = t.max_ground_angle_deg.to_radians().cos();
    if let Some(hit) = ground.filter(|h| h.normal1.dot(up_w) > max_cos) {
        let snapped_center = center_w - up_w * hit.distance;
        feet = frame.point_to_local(snapped_center) - up * half;
        let new_up = frame.dir_to_local(hit.normal1).normalize();
        let turn = Quat::from_rotation_arc(up, new_up);
        if up.dot(new_up) < 0.985 {
            player.discontinuity += 1;
        }
        orientation = (turn * orientation).normalize();
        velocity = turn * velocity;
        up = new_up;
        player.mode = Mode::Booted {
            body,
            feet,
            up,
            velocity,
            orientation,
            pitch,
        };
        retarget_body(bodies, player, hit.entity, &frame);
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
            if hit.normal.dot(frame.dir_to_world(tangent)) > 0.3 {
                let new_up = frame.dir_to_local(hit.normal).normalize();
                let new_feet = frame.point_to_local(origin_w + back * hit.distance) + new_up * SKIN;
                let turn = Quat::from_rotation_arc(up, new_up);
                orientation = turn * orientation;
                velocity = turn * velocity;
                player.discontinuity += 1;
                player.mode = Mode::Booted {
                    body,
                    feet: new_feet,
                    up: new_up,
                    velocity,
                    orientation,
                    pitch,
                };
                retarget_body(bodies, player, hit.entity, &frame);
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

/// If the surface we just stepped onto belongs to another body, move into its frame.
fn retarget_body(bodies: &Bodies, player: &mut Player, collider: Entity, from: &BodyFrame) {
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
    player.last_event = "stepped onto another body".into();
}

fn eva_tick(ctx: &TickContext, bodies: &mut Bodies, player: &mut Player, cmd: &PlayerCommand) {
    let t = ctx.tuning;
    let Mode::Eva {
        mut position,
        mut velocity,
        mut orientation,
        mut pitch,
    } = player.mode
    else {
        return;
    };
    let inverse_mass = 1.0 / t.player_mass;

    // The body slowly turns to follow where the head is looking.
    let follow = pitch * (1.0 - (-BODY_FOLLOW_RATE * ctx.dt).exp());
    orientation = (orientation * Quat::from_rotation_x(follow)).normalize();
    pitch -= follow;
    let view = orientation * Quat::from_rotation_x(pitch);

    // Jetpack.
    let thrust_dir =
        (view * Vec3::new(cmd.movement.x, cmd.movement.y, -cmd.movement.z)).clamp_length_max(1.0);
    let thrust = t.thrust_accel * if cmd.sprint { t.boost_multiplier } else { 1.0 };
    let dv = thrust_dir * thrust * ctx.dt;
    if player.fuel >= dv.length() {
        velocity += dv;
        player.fuel -= dv.length();
    }
    if cmd.brake {
        if let Some(frame) = player.reference_body.and_then(|b| bodies.frame(b)) {
            let relative = velocity - frame.point_velocity(position);
            let dv = (-relative).clamp_length_max(t.brake_accel * ctx.dt);
            if player.fuel >= dv.length() {
                velocity += dv;
                player.fuel -= dv.length();
            }
        }
    }

    // Rope.
    if let Some(mut grapple) = player.grapple {
        if let (Some(frame), Some(mass)) = (bodies.frame(grapple.body), bodies.mass(grapple.body)) {
            let (max_impulse, reeling) = reel(&mut grapple, cmd, ctx);
            let anchor_w = frame.point_to_world(grapple.anchor);
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
            ) {
                grapple.tension = imp.magnitude / ctx.dt;
                velocity += imp.direction * imp.magnitude * inverse_mass;
                bodies.apply_impulse(grapple.body, -imp.direction * imp.magnitude, anchor_w);
                if imp.clamped && reeling {
                    grapple.length = grapple.length.max(anchor_w.distance(position));
                } else if imp.clamped {
                    snapped = true;
                }
            }
            player.grapple = if snapped {
                player.last_event = "rope snapped".into();
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
        if resolved.contains(&body) {
            continue;
        }
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
            player.mode = eva_to_booted(body, &frame, contact, normal, orientation, pitch);
            player.reference_body = Some(body);
            player.discontinuity += 1;
            player.last_event = format!("landed at {:.1} m/s", relative.length());
            return;
        }
        if closing >= 0.0 {
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
        if -closing > 1.0 {
            player.last_event = format!("bounced at {:.1} m/s", -closing);
        }
    }

    player.mode = Mode::Eva {
        position,
        velocity,
        orientation,
        pitch,
    };
}
