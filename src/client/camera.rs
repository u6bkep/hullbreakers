//! First-person camera.
//!
//! The render pose is built in the frame the player lives in: a booted player's
//! eye is placed relative to the *interpolated* transform of the body underfoot,
//! so the player never jitters against the deck no matter how the body moves.
//! Events that relocate the view (landing, edge wraps, wall transitions) are
//! smoothed over by a decaying pose offset.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{CameraOutputMode, Hdr};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::cluster::GlobalClusterSettings;
use bevy::prelude::*;
use bevy::render::render_resource::BlendState;

use super::events::Presented;
use super::sfx::STEP_LENGTH;
use super::{LocalInput, LocalPlayer, ViewCorrection};
use crate::sim::combat::Beams;
use crate::sim::events::{Detach, PlayerEvent};
use crate::sim::frame::BodyFrame;
use crate::sim::player::{Mode, Player, PlayerTelemetry, PreviousMode};
use crate::sim::tuning::Tuning;

#[derive(Component)]
pub struct PlayerCamera;

/// Render-side camera state.
#[derive(Component, Default)]
pub struct CameraSmoothing {
    position_offset: Vec3,
    /// What's left of the network's corrections to blend out (apart from
    /// `position_offset`, which is relocations: landings, edge wraps), and
    /// how fast it's going (see [`blend_out`]).
    correction_offset: Vec3,
    correction_rate: Vec3,
    rotation_offset: Quat,
    last_eye: Vec3,
    last_rotation: Quat,
    bob_phase: f32,
}

/// The render layer the first-person view model (your hands and what they
/// hold) is on. Only the view-model camera draws it.
pub const VIEW_MODEL_LAYER: usize = 1;
/// The view model's own sun (see `render::setup_lighting`) is on these: one
/// the view-model camera sees and the world doesn't, and one every world
/// mesh is on as well, so the world still shadows your hands.
pub const HAND_SUN_LAYER: usize = 2;
pub const SHADOWS_HANDS_LAYER: usize = 3;

/// The camera that draws the view model over the world (see
/// [`spawn_camera`]).
#[derive(Component)]
pub struct ViewModelCamera;

/// The view is drawn by two cameras into one picture. The player camera
/// draws the world; the view-model camera, riding with it, then draws your
/// hands on top with a depth buffer of their own (so a gun never pokes into
/// a wall) and finishes the picture: bloom, tonemapping and the HUD happen
/// once, there, over both. Your own headlamp lights only the world: at arm's
/// length it would only glare off your sleeve. Every other light lights
/// both (see [`light_view_model`]), but the sun: the hands have one of their
/// own, with a single short shadow cascade where they are (a second set of
/// the sun's full cascades just for them cost 0.3 ms a frame in a
/// firefight).
pub fn spawn_camera(
    mut commands: Commands,
    tuning: Res<Tuning>,
    egui: Option<ResMut<bevy_egui::EguiGlobalSettings>>,
    clustering: Option<ResMut<GlobalClusterSettings>>,
) {
    // Bevy's GPU light clustering lights every view with every point and
    // spot light, whatever their layers (0.19); its CPU clustering honours
    // them, which keeps your headlamp off your hands.
    if let Some(mut clustering) = clustering {
        clustering.gpu_clustering = None;
    }
    // The HUD goes on the camera that finishes the picture, not on
    // whichever camera egui would pick.
    if let Some(mut egui) = egui {
        egui.auto_create_primary_context = false;
    }
    let player_camera = commands
        .spawn((
            Name::new("Camera"),
            PlayerCamera,
            Camera3d::default(),
            Camera {
                // The view-model camera writes the finished picture out.
                output_mode: CameraOutputMode::Skip,
                ..default()
            },
            // HDR, tonemapped (and bloomed, so lasers, alarms and explosions
            // glow) by the view-model camera.
            Hdr,
            Tonemapping::None,
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
            // A little right of and below the eye, so what it lights shows
            // some shape. The world's layer only: not on your own hands.
            children![(
                headlamp(true),
                RenderLayers::layer(0),
                Transform::from_xyz(0.2, -0.15, 0.0)
            )],
        ))
        .id();
    commands.spawn((
        Name::new("View-model camera"),
        ViewModelCamera,
        Camera3d::default(),
        Camera {
            order: 1,
            // Over the world, as drawn.
            clear_color: ClearColorConfig::None,
            // The whole picture, as it is: nothing under it to blend with.
            output_mode: CameraOutputMode::Write {
                blend_state: Some(BlendState::REPLACE),
                clear_color: ClearColorConfig::None,
            },
            ..default()
        },
        Hdr,
        bevy::post_process::bloom::Bloom::NATURAL,
        // Its field of view follows the player camera's (see `update_camera`).
        Projection::Perspective(PerspectiveProjection {
            fov: tuning.fov_deg.to_radians(),
            near: 0.01,
            far: 5.0,
            ..default()
        }),
        RenderLayers::from_layers(&[VIEW_MODEL_LAYER, HAND_SUN_LAYER]),
        bevy_egui::PrimaryEguiContext,
        Transform::default(),
        ChildOf(player_camera),
    ));
}

/// Every light lights the view model as well as the world, unless it was
/// given layers of its own (your headlamp).
pub fn light_view_model<L: Component>(
    add: On<Add, L>,
    layered: Query<(), With<RenderLayers>>,
    mut commands: Commands,
) {
    if !layered.contains(add.entity) {
        commands
            .entity(add.entity)
            .insert(RenderLayers::from_layers(&[0, VIEW_MODEL_LAYER]));
    }
}

/// Every mesh is in the world, and shadows the hands, unless it was given
/// layers of its own.
pub fn layer_world_mesh(
    add: On<Add, Mesh3d>,
    layered: Query<(), With<RenderLayers>>,
    mut commands: Commands,
) {
    if !layered.contains(add.entity) {
        commands
            .entity(add.entity)
            .insert(RenderLayers::from_layers(&[0, SHADOWS_HANDS_LAYER]));
    }
}

/// A suit's headlamp: a spot along the view. Only your own casts shadows.
pub fn headlamp(shadows: bool) -> SpotLight {
    SpotLight {
        intensity: 2_000_000.0,
        range: 60.0,
        color: Color::srgb(1.0, 0.95, 0.88),
        inner_angle: 0.22,
        outer_angle: 0.5,
        shadow_maps_enabled: shadows,
        ..default()
    }
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
    /// How far the view is zoomed, eased toward the loadout's zoom (0 or 1:
    /// not).
    pub zoom: f32,
    /// A push's kick: the axis the head turns about (world), how far at
    /// most (rad), and how long ago it started (s).
    kick_axis: Vec3,
    kick_size: f32,
    kick_age: f32,
}

impl CameraFx {
    /// Adds to the shake (0..1, as trauma).
    pub fn shake(&mut self, amount: f32) {
        self.trauma = (self.trauma + amount).min(1.0);
    }

    /// A push in world direction `dir` (unit), of `size` (rad of head
    /// kick): the head turns the way it was pushed, easing in over a few
    /// frames and back (a bigger push than one under way replaces it).
    pub fn push(&mut self, dir: Vec3, size: f32) {
        let current = self.kick_size * kick_shape(self.kick_age);
        if size <= current {
            return;
        }
        // Pushed along `dir`, the head tips about the axis across it.
        let axis = Vec3::Y.cross(dir).try_normalize().unwrap_or(Vec3::X);
        self.kick_axis = axis;
        self.kick_size = size.min(MAX_KICK);
        self.kick_age = 0.0;
    }

    /// How hard the view is shaking (trauma, 0..1) and the push kick's
    /// size (rad), for tests.
    pub fn felt(&self) -> (f32, f32) {
        (self.trauma, self.kick_size)
    }

    fn kick_rotation(&self) -> Quat {
        Quat::from_axis_angle(self.kick_axis.try_normalize().unwrap_or(Vec3::X), self.kick_size * kick_shape(self.kick_age))
    }

    fn settle_kick(&mut self, dt: f32) {
        self.kick_age += dt;
    }

    /// Opacity of the black overlay (see [`PlayerEvent::Respawned`]).
    pub fn fade(&self) -> f32 {
        self.fade
    }
}

/// The largest head kick (rad), and how long one takes to peak (s).
const MAX_KICK: f32 = 0.09;
const KICK_PEAK: f32 = 0.07;

/// A kick's shape over time: rises from 0 to 1 at [`KICK_PEAK`] (in a few
/// frames, so nothing jumps), then falls away.
fn kick_shape(age: f32) -> f32 {
    let x = age / KICK_PEAK;
    if x > 12.0 { 0.0 } else { x * (1.0 - x).exp() }
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

/// How long (s) a correction of `size` (m) takes to blend out: the usual
/// `smooth_time` for a small one, longer for a big one (up to
/// [`MAX_CORRECTION_TIME`]), so a metres-long correction glides rather
/// than lurches.
pub fn correction_time(size: f32, smooth_time: f32) -> f32 {
    (smooth_time * (1.0 + size / CORRECTION_SCALE)).clamp(smooth_time, MAX_CORRECTION_TIME.max(smooth_time))
}

/// The size of correction (m) that blends twice as slowly as a small one.
const CORRECTION_SCALE: f32 = 0.6;
const MAX_CORRECTION_TIME: f32 = 0.35;

/// What's left of a correction `offset` (m) after `dt` (s), and how fast
/// it's closing (`rate`, m/s). A critically damped spring: it starts from
/// rest, so the drawn eye never jumps the frame a correction lands (an
/// exponential blend's first step is its biggest), and settles in about
/// the [`correction_time`] of what's left.
pub fn blend_out(offset: Vec3, rate: Vec3, dt: f32, smooth_time: f32) -> (Vec3, Vec3) {
    let tau = correction_time(offset.length(), smooth_time) / 2.0;
    if tau <= 0.0 {
        return (Vec3::ZERO, Vec3::ZERO);
    }
    let w = 1.0 / tau;
    let rate = rate + (-2.0 * w * rate - w * w * offset) * dt;
    (offset + rate * dt, rate)
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
    mut view_model: Query<&mut Projection, (With<ViewModelCamera>, Without<PlayerCamera>)>,
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
    // Corrections from the network: keep showing the old view, blend out,
    // the bigger ones more slowly (see [`blend_out`]). A relocation this
    // frame already blends from the last drawn view, whatever moved it.
    if !relocated {
        smooth.correction_offset += correction.position;
        smooth.rotation_offset = (smooth.rotation_offset * correction.rotation).normalize();
    }
    *correction = ViewCorrection {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };
    smooth.position_offset *= decay;
    (smooth.correction_offset, smooth.correction_rate) =
        blend_out(smooth.correction_offset, smooth.correction_rate, time.delta_secs(), tuning.smooth_time);
    smooth.rotation_offset = Quat::IDENTITY.slerp(smooth.rotation_offset, decay);

    smooth.last_eye = eye;
    smooth.last_rotation = rotation;
    eye += smooth.position_offset + smooth.correction_offset;
    let mut rotation = (smooth.rotation_offset * rotation).normalize();
    // A push felt (see `client::push_feel`): the head knocked the way it
    // went, and back.
    rotation = (fx.kick_rotation() * rotation).normalize();
    fx.settle_kick(time.delta_secs());

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
    let fov = (tuning.fov_deg + fx.fov_kick).to_radians();
    // Zoom narrows the world's view (not the hands'), quickly.
    let zoom_target = player.loadout.zoom.max(1.0);
    fx.zoom = fx.zoom.max(1.0) + (zoom_target - fx.zoom.max(1.0)) * (1.0 - (-dt * 14.0).exp());
    if let Projection::Perspective(p) = &mut *projection {
        p.fov = 2.0 * ((fov / 2.0).tan() / fx.zoom).atan();
    }
    for mut projection in &mut view_model {
        if let Projection::Perspective(p) = &mut *projection {
            p.fov = fov;
        }
    }
}

#[cfg(test)]
mod blend_tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn settle(offset: f32, seconds: f32) -> Vec<f32> {
        let (mut o, mut r) = (Vec3::X * offset, Vec3::ZERO);
        (0..(seconds / DT) as usize)
            .map(|_| {
                (o, r) = blend_out(o, r, DT, 0.12);
                o.x
            })
            .collect()
    }

    #[test]
    fn a_correction_blends_out_from_rest_without_overshoot() {
        let path = settle(3.0, 1.5);
        // No jump the frame it lands: the first step is small.
        assert!(3.0 - path[0] < 0.05, "first step {}", 3.0 - path[0]);
        // Steps grow, then shrink: the biggest is under a tenth of it.
        let steps: Vec<f32> = std::iter::once(3.0).chain(path.iter().copied()).collect::<Vec<_>>().windows(2).map(|w| w[0] - w[1]).collect();
        assert!(steps.iter().all(|&s| s >= -1e-4), "overshoots: {steps:?}");
        assert!(steps.iter().cloned().fold(0.0, f32::max) < 0.3);
        // Gone in about a second.
        assert!(path[60].abs() < 0.1, "{} left after 1 s", path[60]);
    }

    #[test]
    fn small_corrections_are_quick_and_big_ones_glide() {
        let small = settle(0.1, 0.5);
        // About as quick as the old fixed blend (12.5% left after 0.25 s).
        let left = small[(0.25 / DT) as usize - 1].abs() / 0.1;
        assert!(left < 0.15, "a small correction lingers: {left:.3} of it left after 0.25 s");
        assert!(correction_time(3.0, 0.12) > 2.0 * correction_time(0.1, 0.12));
    }
}

