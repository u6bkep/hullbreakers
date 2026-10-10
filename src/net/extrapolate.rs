//! The extrapolated replication policy: a body reaches clients as its
//! [`Motion`], re-sent only when the body departs from it.
//!
//! The authority simulates the body as usual and, after each tick, compares
//! it with where its last sent motion says it should be. Free flight matches
//! (isotropic rocks follow it exactly), so nothing is sent; a collision, an
//! impulse, or a scripted manoeuvre makes it diverge, and a fresh motion goes
//! out. Clients pose the body from its motion at their own tick, which is
//! the present (not an interpolated past), and blend over each correction.

use avian3d::prelude::*;
use bevy::prelude::*;

use lightyear::prelude::client::LocalTimelineSync;

use crate::sim::{SimTick, TICK_HZ};
use crate::sim::bodies::BodyInertia;
use crate::sim::frame::BodyFrame;
use crate::sim::motion::Motion;

/// Replicate this body by its motion.
#[derive(Component)]
pub struct Extrapolated;

/// How far a body may stray from its sent motion before it is re-sent:
/// position anywhere on the body (m), and velocity (m/s) at its extent.
const POSITION_TOLERANCE: f32 = 0.01;
const VELOCITY_TOLERANCE: f32 = 0.01;
/// How long a client takes to blend over a correction (s).
const CORRECTION_TIME: f32 = 0.12;

/// Authority: keeps each extrapolated body's motion within tolerance of the
/// simulated body, sampling a new one when it isn't.
pub fn track_motion(
    tick: Res<SimTick>,
    mut bodies: Query<
        (
            Entity,
            &Position,
            &Rotation,
            &LinearVelocity,
            &AngularVelocity,
            &BodyInertia,
            Option<&mut Motion>,
        ),
        With<Extrapolated>,
    >,
    mut commands: Commands,
    mut stats: Local<(u32, u32)>,
) {
    // Report the re-send rate every 10 s (ticks, sends).
    stats.0 += 1;
    if stats.0 == 600 {
        info!("extrapolated bodies: {:.1} motion updates/s", stats.1 as f32 / 10.0);
        *stats = (0, 0);
    }
    for (entity, pos, rot, lin, ang, inertia, motion) in &mut bodies {
        let actual = BodyFrame {
            position: pos.0,
            rotation: rot.0,
            center_of_mass: pos.0 + rot.0 * inertia.center_of_mass,
            linear_velocity: lin.0,
            angular_velocity: ang.0,
        };
        let Some(mut motion) = motion else {
            commands
                .entity(entity)
                .insert(Motion::sample(tick.0, &actual));
            continue;
        };
        let predicted = motion.frame_at(tick.0, inertia.center_of_mass);
        // Errors at the body's extent, so a big rock's slight turn counts.
        let reach = inertia.reach;
        let turn = predicted.rotation.angle_between(actual.rotation);
        let position_error =
            (predicted.center_of_mass - actual.center_of_mass).length() + turn * reach;
        let velocity_error = (predicted.linear_velocity - actual.linear_velocity).length()
            + (predicted.angular_velocity - actual.angular_velocity).length() * reach;
        if position_error > POSITION_TOLERANCE || velocity_error > VELOCITY_TOLERANCE {
            *motion = Motion::sample(tick.0, &actual);
            stats.1 += 1;
        }
    }
}

/// Client: what a body was last drawn from, and the correction still being
/// blended out.
#[derive(Component, Default)]
pub struct MotionSmoothing {
    drawn_from: Option<Motion>,
    offset: Vec3,
    rotation: Quat,
}

/// Client: a body that has just arrived gets velocities (posed from its
/// motion each tick).
pub fn prepare_extrapolated(add: On<Add, Motion>, mut commands: Commands) {
    commands.entity(add.entity).insert((
        LinearVelocity::default(),
        AngularVelocity::default(),
        MotionSmoothing {
            rotation: Quat::IDENTITY,
            ..default()
        },
    ));
}

/// Client: whether this peer's clock has synchronized with the authority's.
/// Before that, its ticks mean nothing to a [`Motion`].
pub fn synced(sync: Option<Res<LocalTimelineSync>>) -> bool {
    sync.is_some_and(|s| s.is_synced())
}

/// Client: the time (in ticks) bodies are drawn at. It follows the
/// simulation clock but glides over its jumps (lightyear re-syncing the
/// timeline), so bodies don't all snap when the clock does.
#[derive(Resource, Default)]
pub struct DrawClock(Option<f64>);

const CLOCK_SLEW_TIME: f32 = 0.15;
/// Beyond this many ticks off, the draw clock just jumps.
const CLOCK_SNAP_TICKS: f64 = 30.0;

pub fn advance_draw_clock(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tick: Res<SimTick>,
    sync: Option<Res<LocalTimelineSync>>,
    mut clock: ResMut<DrawClock>,
) {
    if !synced(sync) {
        clock.0 = None;
        return;
    }
    let target = tick.0 as f64 + fixed.overstep_fraction() as f64;
    let dt = time.delta_secs();
    clock.0 = Some(match clock.0 {
        Some(t) => {
            let t = t + (dt * TICK_HZ as f32) as f64;
            let error = target - t;
            if error.abs() > CLOCK_SNAP_TICKS {
                target
            } else {
                t + error * (1.0 - (-dt / CLOCK_SLEW_TIME).exp()) as f64
            }
        }
        None => target,
    });
}

/// Client, each tick: the physics state of every extrapolated body at this
/// tick, for spatial queries and reference frames.
pub fn follow_motion(
    tick: Res<SimTick>,
    mut bodies: Query<(
        &Motion,
        &BodyInertia,
        &mut Position,
        &mut Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    for (motion, inertia, mut pos, mut rot, mut lin, mut ang) in &mut bodies {
        let frame = motion.frame_at(tick.0, inertia.center_of_mass);
        pos.0 = frame.position;
        rot.0 = frame.rotation;
        lin.0 = frame.linear_velocity;
        ang.0 = frame.angular_velocity;
    }
}

/// Client, each frame: where every extrapolated body is drawn, between
/// ticks, with any correction blended out.
pub fn draw_extrapolated(
    time: Res<Time>,
    clock: Res<DrawClock>,
    mut unposed: Query<&mut Visibility, (With<BodyInertia>, Without<Motion>)>,
    mut bodies: Query<(
        &Motion,
        &BodyInertia,
        &mut MotionSmoothing,
        &mut Transform,
        &mut Visibility,
    )>,
) {
    // Nothing is drawn before it can be posed.
    for mut visibility in &mut unposed {
        *visibility = Visibility::Hidden;
    }
    let Some(now) = clock.0 else {
        for (.., mut visibility) in &mut bodies {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let (tick, fraction) = (now.floor() as u32, now.fract() as f32);
    let decay = (-time.delta_secs() / CORRECTION_TIME).exp();
    for (motion, inertia, mut smoothing, mut transform, mut visibility) in &mut bodies {
        let com = inertia.center_of_mass;
        let pose = motion.at(tick, fraction, com);
        *visibility = Visibility::Inherited;
        match smoothing.drawn_from {
            None => {}
            Some(old) if old != *motion => {
                // Keep drawing where it was; blend toward the new motion.
                let was = old.at(tick, fraction, com);
                smoothing.offset += was.position - pose.position;
                smoothing.rotation = (smoothing.rotation * was.rotation * pose.rotation.inverse())
                    .normalize();
            }
            Some(_) => {}
        }
        smoothing.drawn_from = Some(*motion);
        smoothing.offset *= decay;
        smoothing.rotation = Quat::IDENTITY.slerp(smoothing.rotation, decay);
        transform.translation = pose.position + smoothing.offset;
        transform.rotation = (smoothing.rotation * pose.rotation).normalize();
    }
}
