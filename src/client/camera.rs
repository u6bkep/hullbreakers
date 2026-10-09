//! First-person camera.
//!
//! The render pose is built in the frame the player lives in: a booted player's
//! eye is placed relative to the *interpolated* transform of the body underfoot,
//! so the player never jitters against the deck no matter how the body moves.
//! Discontinuities reported by the sim (landing, edge wraps, wall transitions)
//! are hidden by a decaying pose offset.

use bevy::prelude::*;

use crate::sim::frame::BodyFrame;
use crate::sim::player::{Mode, Player, PlayerCommand, PreviousMode};
use crate::sim::tuning::Tuning;

#[derive(Component)]
pub struct PlayerCamera;

/// Render-side camera state.
#[derive(Component, Default)]
pub struct CameraSmoothing {
    seen_discontinuity: u32,
    position_offset: Vec3,
    rotation_offset: Quat,
    last_eye: Vec3,
    last_rotation: Quat,
    bob_phase: f32,
}

pub fn spawn_camera(mut commands: Commands, tuning: Res<Tuning>) {
    commands.spawn((
        Name::new("Camera"),
        PlayerCamera,
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: tuning.fov_deg.to_radians(),
            near: 0.05,
            far: 20_000.0,
            ..default()
        }),
        CameraSmoothing {
            rotation_offset: Quat::IDENTITY,
            last_rotation: Quat::IDENTITY,
            ..default()
        },
        Transform::default(),
    ));
}

/// Pose-only frame from a body's interpolated render transform.
fn render_frame(transform: &Transform) -> BodyFrame {
    BodyFrame {
        position: transform.translation,
        rotation: transform.rotation,
        ..BodyFrame::IDENTITY
    }
}

pub fn update_camera(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<Tuning>,
    player: Single<(&Player, &PreviousMode, &PlayerCommand)>,
    transforms: Query<&Transform, Without<PlayerCamera>>,
    camera: Single<(&mut Transform, &mut CameraSmoothing, &mut Projection), With<PlayerCamera>>,
) {
    let (player, previous, command) = *player;
    let (mut cam_transform, mut smooth, mut projection) = camera.into_inner();
    let alpha = fixed.overstep_fraction();

    // Apply look input the sim hasn't consumed yet, for zero-latency aim.
    let mut current = player.mode;
    let mut prior = previous.0;
    current.apply_look(command.look, 0.0);
    prior.apply_look(command.look, 0.0);

    let discontinuity = player.discontinuity != smooth.seen_discontinuity;
    let (local_eye, local_rot) = {
        let (eye, rot) = current.view(&tuning);
        let same_frame = match (prior, current) {
            (Mode::Eva { .. }, Mode::Eva { .. }) => true,
            (Mode::Booted { body: a, .. }, Mode::Booted { body: b, .. }) => a == b,
            _ => false,
        };
        if same_frame && !discontinuity {
            let (prior_eye, prior_rot) = prior.view(&tuning);
            (prior_eye.lerp(eye, alpha), prior_rot.slerp(rot, alpha))
        } else {
            (eye, rot)
        }
    };

    let (mut eye, rotation) = match current {
        Mode::Booted {
            body, up, velocity, ..
        } => {
            let Ok(body_transform) = transforms.get(body) else {
                return;
            };
            let frame = render_frame(body_transform);
            // Head bob scales with walking speed.
            let speed = velocity.length();
            smooth.bob_phase += speed * time.delta_secs() * 2.2;
            let bob = up
                * smooth.bob_phase.sin().abs()
                * tuning.head_bob
                * (speed / tuning.walk_speed).min(1.5);
            (
                frame.point_to_world(local_eye + bob),
                frame.rotation * local_rot,
            )
        }
        Mode::Eva { .. } => (local_eye, local_rot),
    };

    if discontinuity {
        smooth.seen_discontinuity = player.discontinuity;
        smooth.position_offset = smooth.last_eye + smooth.position_offset - eye;
        smooth.rotation_offset =
            (smooth.rotation_offset * smooth.last_rotation * rotation.inverse()).normalize();
    }
    let decay = if tuning.smooth_time > 0.0 {
        (-time.delta_secs() / tuning.smooth_time).exp()
    } else {
        0.0
    };
    smooth.position_offset *= decay;
    smooth.rotation_offset = Quat::IDENTITY.slerp(smooth.rotation_offset, decay);

    smooth.last_eye = eye;
    smooth.last_rotation = rotation;
    eye += smooth.position_offset;
    cam_transform.translation = eye;
    cam_transform.rotation = (smooth.rotation_offset * rotation).normalize();

    if let Projection::Perspective(p) = &mut *projection {
        p.fov = tuning.fov_deg.to_radians();
    }
}
