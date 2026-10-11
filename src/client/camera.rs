//! First-person camera.
//!
//! The render pose is built in the frame the player lives in: a booted player's
//! eye is placed relative to the *interpolated* transform of the body underfoot,
//! so the player never jitters against the deck no matter how the body moves.
//! Events that relocate the view (landing, edge wraps, wall transitions) are
//! smoothed over by a decaying pose offset.

use bevy::prelude::*;

use super::events::Presented;
use super::sfx::STEP_LENGTH;
use crate::sim::combat::Beams;
use crate::sim::events::{Detach, PlayerEvent};
use crate::sim::frame::BodyFrame;
use super::{LocalInput, LocalPlayer, ViewCorrection};
use crate::sim::player::{Mode, Player, PlayerTelemetry, PreviousMode};
use crate::sim::tuning::Tuning;

#[derive(Component)]
pub struct PlayerCamera;

/// Render-side camera state.
#[derive(Component, Default)]
pub struct CameraSmoothing {
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
        // Ears for other players' sounds (see `sfx::play_remote_sounds`).
        SpatialListener::new(0.2),
        Transform::default(),
    ));
}

/// Pose-only frame from a body's interpolated render transform.
pub(crate) fn render_frame(transform: &Transform) -> BodyFrame {
    BodyFrame {
        position: transform.translation,
        rotation: transform.rotation,
        ..BodyFrame::IDENTITY
    }
}

/// Transient camera effects driven by gameplay events.
#[derive(Resource, Default)]
pub struct CameraFx {
    /// 0..1; shake amplitude is trauma².
    trauma: f32,
    /// Downward eye offset after a landing, in meters.
    dip: f32,
    /// Extra FOV from boosting, in degrees.
    fov_kick: f32,
    /// 0..1 black overlay, fading out after a hard cut.
    fade: f32,
}

impl CameraFx {
    /// Opacity of the black overlay (see [`PlayerEvent::Respawned`]).
    pub fn fade(&self) -> f32 {
        self.fade
    }
}

pub fn camera_fx_events(mut fx: ResMut<CameraFx>, mut events: MessageReader<Presented>) {
    for Presented(event) in events.read() {
        match *event {
            PlayerEvent::Landed { speed, .. } => {
                fx.dip = (fx.dip + 0.04 + 0.04 * speed).min(0.3);
                fx.trauma += speed * 0.06;
            }
            PlayerEvent::Impact { speed, .. } => fx.trauma += speed * 0.1,
            PlayerEvent::Detached(Detach::RippedOff(_) | Detach::Yanked(_)) => fx.trauma += 0.5,
            PlayerEvent::RopeSnapped => fx.trauma += 0.3,
            PlayerEvent::Detached(Detach::Blasted) => fx.trauma += 1.0,
            PlayerEvent::BeamHit => fx.trauma += 0.6,
            // A hard cut: start black and fade in so it reads as a jump.
            PlayerEvent::Respawned => fx.fade = 1.0,
            _ => {}
        }
        fx.trauma = fx.trauma.min(1.0);
    }
}

/// Shakes the view when a beam starts hitting the ship the local player
/// stands on. Beams are a function of the tick on every peer, so this needs
/// no event: it watches [`Beams`] for the rising edge.
pub fn beam_shake(
    beams: Res<Beams>,
    player: Single<&Player, With<LocalPlayer>>,
    mut fx: ResMut<CameraFx>,
    mut was_hit: Local<bool>,
) {
    let hit = player
        .mode
        .body()
        .and_then(|body| beams.0.iter().find(|b| b.target == body));
    if let Some(beam) = hit
        && !*was_hit
    {
        // Shields soak most of it.
        fx.trauma = (fx.trauma + if beam.shielded { 0.3 } else { 0.55 }).min(1.0);
    }
    *was_hit = hit.is_some();
}

pub fn update_camera(
    time: Res<Time>,
    mut fx: ResMut<CameraFx>,
    fixed: Res<Time<Fixed>>,
    tuning: Res<Tuning>,
    mut events: MessageReader<Presented>,
    mut correction: ResMut<ViewCorrection>,
    local: Res<LocalInput>,
    player: Single<(&Player, Option<&PreviousMode>, &PlayerTelemetry), With<LocalPlayer>>,
    transforms: Query<&Transform, Without<PlayerCamera>>,
    camera: Single<(&mut Transform, &mut CameraSmoothing, &mut Projection), With<PlayerCamera>>,
) {
    let (player, previous, telemetry) = *player;
    let input = &local.0;
    let (mut cam_transform, mut smooth, mut projection) = camera.into_inner();
    let alpha = fixed.overstep_fraction();

    // Apply look input the sim hasn't consumed yet, for zero-latency aim.
    let mut current = player.mode;
    // Where the sim recorded the previous tick (an authority), interpolate
    // between ticks; a replicated player is already interpolated.
    let mut prior = previous.map_or(player.mode, |p| p.0);
    let pending_look = player.input.pending_look(input);
    current.apply_look(pending_look, 0.0);
    prior.apply_look(pending_look, 0.0);

    let relocated = events.read().any(|e| e.0.relocates_view());
    let (local_eye, local_rot) = {
        let (eye, rot) = current.view(&tuning);
        let same_frame = match (prior, current) {
            (Mode::Eva { .. }, Mode::Eva { .. }) => true,
            (Mode::Booted { body: a, .. }, Mode::Booted { body: b, .. }) => a == b,
            _ => false,
        };
        if same_frame && !relocated {
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
            // One bob (half a sine period) per footstep.
            smooth.bob_phase += speed * time.delta_secs() * std::f32::consts::PI / STEP_LENGTH;
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

    if relocated {
        smooth.position_offset = smooth.last_eye + smooth.position_offset - eye;
        smooth.rotation_offset =
            (smooth.rotation_offset * smooth.last_rotation * rotation.inverse()).normalize();
    }
    let decay = if tuning.smooth_time > 0.0 {
        (-time.delta_secs() / tuning.smooth_time).exp()
    } else {
        0.0
    };
    // Corrections from the network: keep showing the old view, blend out.
    // A relocation this frame already blends from the last drawn view,
    // whatever moved it.
    if !relocated {
        smooth.position_offset += correction.position;
        smooth.rotation_offset = (smooth.rotation_offset * correction.rotation).normalize();
    }
    *correction = ViewCorrection {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };
    smooth.position_offset *= decay;
    smooth.rotation_offset = Quat::IDENTITY.slerp(smooth.rotation_offset, decay);

    smooth.last_eye = eye;
    smooth.last_rotation = rotation;
    eye += smooth.position_offset;
    let mut rotation = (smooth.rotation_offset * rotation).normalize();

    // Event effects: landing dip, shake, boost FOV.
    let dt = time.delta_secs();
    let t = time.elapsed_secs();
    eye -= rotation * Vec3::Y * fx.dip;
    let shake = fx.trauma * fx.trauma * 0.04;
    rotation *= Quat::from_euler(
        EulerRot::YXZ,
        shake * (t * 31.0).sin(),
        shake * (t * 37.0 + 1.3).sin(),
        shake * 1.5 * (t * 23.0 + 2.1).sin(),
    );
    fx.dip *= (-dt * 7.0).exp();
    fx.trauma = (fx.trauma - dt * 1.6).max(0.0);
    fx.fade = (fx.fade - dt * 1.5).max(0.0);
    let boosting = matches!(current, Mode::Eva { .. }) && input.sprint && telemetry.thrust > 0.1;
    let kick_target = if boosting { 8.0 } else { 0.0 };
    fx.fov_kick += (kick_target - fx.fov_kick) * (1.0 - (-dt * 4.0).exp());

    cam_transform.translation = eye;
    cam_transform.rotation = rotation;
    if let Projection::Perspective(p) = &mut *projection {
        p.fov = (tuning.fov_deg + fx.fov_kick).to_radians();
    }
}
