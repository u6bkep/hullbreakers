//! Procedurally synthesized sound effects, driven by [`PlayerEvent`]s and
//! continuous player state. Everything is heard "through the suit": there is
//! no sound in vacuum, so your own effects are non-spatial and contact
//! sounds are dull and bassy. Other players are heard only through structure
//! you share: their footsteps, landings, bumps and grapple strikes on the
//! body you stand on reach you from their direction, fading with distance.
//! In EVA you hear nothing of them.

use std::f32::consts::TAU;

use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::sim::asteroids::Asteroid;
use super::events::Presented;
use crate::sim::events::{Detach, PlayerEvent};
use super::events::EventCursor;
use super::render::Avatar;
use super::LocalPlayer;
use crate::sim::player::{Mode, Player, PlayerTelemetry};
use crate::sim::tuning::Tuning;

const RATE: u32 = 44_100;

/// Distance walked per footstep. Shared with the camera's head bob.
pub const STEP_LENGTH: f32 = 1.0;

/// Encodes mono samples in [-1, 1] as a 16-bit WAV file.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

/// Small synthesis toolkit.
struct Synth {
    rng: ChaCha8Rng,
}

impl Synth {
    fn buffer(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * RATE as f32) as usize]
    }

    fn noise(&mut self) -> f32 {
        self.rng.random_range(-1.0..1.0)
    }

    /// Low-passed noise burst with an exponential decay: thuds and footfalls.
    fn thud(&mut self, seconds: f32, decay: f32, cutoff: f32, gain: f32) -> Vec<f32> {
        let mut out = Self::buffer(seconds);
        let a = 1.0 - (-TAU * cutoff / RATE as f32).exp();
        let mut lp = 0.0;
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            lp += a * (self.noise() - lp);
            *s = lp * (-t * decay).exp() * gain;
        }
        out
    }

    /// Inharmonic partials with fast decay: a metallic clank.
    fn clank(&mut self, seconds: f32, base: f32, decay: f32, gain: f32) -> Vec<f32> {
        let partials = [(1.0, 1.0), (2.76, 0.5), (5.4, 0.3), (8.93, 0.15)];
        let mut out = Self::buffer(seconds);
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            let tone: f32 = partials
                .iter()
                .map(|(m, g)| g * (TAU * base * m * t).sin() * (-t * decay * m.sqrt()).exp())
                .sum();
            *s = tone * gain * 0.5;
        }
        out
    }

    /// A pitch sweep with noise: zips and whooshes.
    fn sweep(&mut self, seconds: f32, from: f32, to: f32, noise: f32, gain: f32) -> Vec<f32> {
        let mut out = Self::buffer(seconds);
        let mut phase = 0.0;
        let n = out.len() as f32;
        for (i, s) in out.iter_mut().enumerate() {
            let k = i as f32 / n;
            phase += TAU * (from + (to - from) * k) / RATE as f32;
            let env = (k * 40.0).min(1.0) * (1.0 - k).powi(2);
            *s = (phase.sin() * (1.0 - noise) + self.noise() * noise) * env * gain;
        }
        out
    }

    /// Square-ish tone, for UI beeps.
    fn beep(&mut self, seconds: f32, freq: f32, gain: f32) -> Vec<f32> {
        let mut out = Self::buffer(seconds);
        let n = out.len() as f32;
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            let k = i as f32 / n;
            let env = (k * 50.0).min(1.0) * (1.0 - k);
            *s = ((TAU * freq * t).sin() * 3.0).tanh() * env * gain;
        }
        out
    }

    /// Seamlessly looping band-limited noise (crossfaded tail into head).
    /// Makes `raw` (longer than `len`) loop seamlessly by crossfading the
    /// tail past `len` into the head.
    fn crossfade_loop(raw: Vec<f32>, len: usize) -> Vec<f32> {
        let fade = raw.len() - len;
        let mut out = raw[..len].to_vec();
        for i in 0..fade {
            let w = i as f32 / fade as f32;
            out[i] = raw[len + i] * (1.0 - w) + raw[i] * w;
        }
        out
    }

    /// Looping noise through `poles` one-pole low-pass stages (more poles =
    /// more muffled).
    fn noise_loop(&mut self, seconds: f32, cutoff: f32, poles: usize, gain: f32) -> Vec<f32> {
        let len = (seconds * RATE as f32) as usize;
        let a = 1.0 - (-TAU * cutoff / RATE as f32).exp();
        let mut stages = vec![0.0f32; poles];
        let raw: Vec<f32> = (0..len + len / 4)
            .map(|_| {
                let mut x = self.noise();
                for lp in stages.iter_mut() {
                    *lp += a * (x - *lp);
                    x = *lp;
                }
                x * gain
            })
            .collect();
        Self::crossfade_loop(raw, len)
    }

    /// Looping winch: rope sliding through a fabric guide (band-passed noise
    /// with a slow, irregular swell) over a soft geared-motor hum.
    fn winch_loop(&mut self) -> Vec<f32> {
        let len = RATE as usize * 2;
        let (lo, hi) = (250.0, 1400.0);
        let a_lo = 1.0 - (-TAU * lo / RATE as f32).exp();
        let a_hi = 1.0 - (-TAU * hi / RATE as f32).exp();
        let (mut lp_lo, mut lp_hi, mut swell, mut swell_target) = (0.0f32, 0.0f32, 0.5f32, 0.5f32);
        let raw: Vec<f32> = (0..len + len / 4)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                // Band-pass: difference of two low-passes.
                let n = self.noise();
                lp_lo += a_lo * (n - lp_lo);
                lp_hi += a_hi * (n - lp_hi);
                if i % 2205 == 0 {
                    swell_target = self.rng.random_range(0.45..1.0);
                }
                swell += (swell_target - swell) * 0.0004;
                let slide = (lp_hi - lp_lo) * swell;
                // Hum: a 60 Hz fundamental with soft harmonics and a 21 Hz gear flutter.
                let hum: f32 = (1..=5)
                    .map(|h| (TAU * 60.0 * h as f32 * t).sin() / (h * h) as f32)
                    .sum();
                let flutter = 1.0 + 0.25 * (TAU * 21.0 * t).sin();
                slide * 3.0 + hum * flutter * 0.12
            })
            .collect();
        Self::crossfade_loop(raw, len)
    }
}

fn normalize(mut samples: Vec<f32>, peak: f32) -> Vec<f32> {
    let max = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if max > 1e-6 {
        samples.iter_mut().for_each(|s| *s *= peak / max);
    }
    samples
}

fn concat(a: Vec<f32>, b: Vec<f32>) -> Vec<f32> {
    let mut a = a;
    a.extend(b);
    a
}

fn mix(a: Vec<f32>, b: Vec<f32>) -> Vec<f32> {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| a.get(i).unwrap_or(&0.0) + b.get(i).unwrap_or(&0.0))
        .collect()
}

#[derive(Resource)]
pub struct Sounds {
    step_hull: Handle<AudioSource>,
    step_rock: Handle<AudioSource>,
    land_hull: Handle<AudioSource>,
    land_rock: Handle<AudioSource>,
    release: Handle<AudioSource>,
    impact: Handle<AudioSource>,
    grapple_fire: Handle<AudioSource>,
    grapple_attach: Handle<AudioSource>,
    grapple_release: Handle<AudioSource>,
    rope_taut: Handle<AudioSource>,
    rope_snap: Handle<AudioSource>,
    warning: Handle<AudioSource>,
    click_on: Handle<AudioSource>,
    click_off: Handle<AudioSource>,
}

#[derive(Component)]
pub struct JetpackLoop;

#[derive(Component)]
pub struct ReelLoop;

/// Client-side state for continuous sounds.
#[derive(Resource, Default)]
pub struct AudioState {
    stride: f32,
    last_tension: f32,
    /// How long the rope has been (nearly) slack; a twang needs real slack first.
    slack_time: f32,
    twang_cooldown: f32,
    /// Rope length last frame, for how fast cable is running.
    last_rope_length: Option<f32>,
    /// Cable speed through the winch (m/s), smoothed.
    cable_speed: f32,
}

pub fn setup_sounds(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
    let mut synth = Synth {
        rng: ChaCha8Rng::seed_from_u64(3),
    };
    // Every sound is peak-normalized; loudness is set at playback.
    let mut add = |samples: Vec<f32>| {
        sources.add(AudioSource {
            bytes: wav(&normalize(samples, 0.9)).into(),
        })
    };
    let sounds = Sounds {
        step_hull: add(mix(
            synth.thud(0.14, 30.0, 380.0, 0.8),
            synth.clank(0.1, 170.0, 70.0, 0.06),
        )),
        step_rock: add(synth.thud(0.15, 28.0, 280.0, 0.8)),
        land_hull: add(mix(
            synth.thud(0.35, 14.0, 600.0, 0.9),
            synth.clank(0.4, 190.0, 12.0, 0.6),
        )),
        land_rock: add(synth.thud(0.4, 10.0, 350.0, 1.0)),
        release: add(mix(
            synth.clank(0.2, 420.0, 30.0, 0.35),
            synth.sweep(0.15, 300.0, 120.0, 0.6, 0.25),
        )),
        impact: add(mix(
            synth.thud(0.3, 16.0, 450.0, 1.0),
            synth.clank(0.3, 140.0, 18.0, 0.4),
        )),
        grapple_fire: add(synth.sweep(0.18, 2200.0, 700.0, 0.55, 0.5)),
        grapple_attach: add(synth.clank(0.25, 880.0, 25.0, 0.6)),
        grapple_release: add(synth.clank(0.12, 1300.0, 50.0, 0.35)),
        rope_taut: add(synth.clank(0.5, 95.0, 9.0, 0.7)),
        rope_snap: add(mix(
            synth.sweep(0.25, 1800.0, 200.0, 0.4, 0.6),
            synth.thud(0.2, 20.0, 2000.0, 0.5),
        )),
        warning: add(concat(
            synth.beep(0.12, 880.0, 0.35),
            synth.beep(0.12, 660.0, 0.35),
        )),
        click_on: add(synth.beep(0.05, 1500.0, 0.25)),
        click_off: add(synth.beep(0.05, 900.0, 0.25)),
    };
    let jet = add(synth.noise_loop(2.0, 650.0, 2, 1.0));
    let whine = add(synth.winch_loop());
    let looping = |volume: f32| PlaybackSettings {
        mode: PlaybackMode::Loop,
        volume: Volume::Linear(volume),
        ..default()
    };
    commands.spawn((JetpackLoop, AudioPlayer(jet), looping(0.0)));
    commands.spawn((ReelLoop, AudioPlayer(whine), looping(0.0)));
    commands.insert_resource(sounds);
    commands.init_resource::<AudioState>();
}

fn play(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, speed: f32) {
    commands.spawn((
        AudioPlayer(sound.clone()),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(volume),
            speed,
            ..default()
        },
    ));
}

/// Distance (m) at which another player's sounds are as loud as your own
/// would be at 1 m: hull plating carries sound well.
const STRUCTURE_SCALE: f32 = 0.25;
/// Other players' sounds, relative to the same sound of your own.
const REMOTE_GAIN: f32 = 0.7;

fn play_at(
    commands: &mut Commands,
    sound: &Handle<AudioSource>,
    volume: f32,
    speed: f32,
    position: Vec3,
) {
    commands.spawn((
        AudioPlayer(sound.clone()),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(volume * REMOTE_GAIN),
            speed,
            spatial: true,
            spatial_scale: Some(bevy::audio::SpatialScale::new(STRUCTURE_SCALE)),
            ..default()
        },
        Transform::from_translation(position),
    ));
}

/// What presentation keeps for each other player.
#[derive(Component, Default)]
pub struct RemoteHearing {
    events: EventCursor,
    stride: f32,
}

/// Other players' contact sounds, carried through the body you share.
pub fn play_remote_sounds(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<Tuning>,
    sounds: Res<Sounds>,
    rocks: Query<(), With<Asteroid>>,
    listener: Query<&Player, With<LocalPlayer>>,
    mut players: Query<(Entity, &Player, Option<&mut RemoteHearing>), Without<LocalPlayer>>,
    avatars: Query<(&Avatar, &GlobalTransform)>,
    poses: Query<&GlobalTransform, Without<Avatar>>,
) {
    let shared = listener.single().ok().and_then(|p| p.mode.body());
    let dt = time.delta_secs();
    let mut rng = rand::rng();
    for (entity, player, hearing) in &mut players {
        let Some(mut hearing) = hearing else {
            commands.entity(entity).insert(RemoteHearing::default());
            continue;
        };
        // Read the log whether or not it's heard, so nothing plays late.
        let events = hearing.events.take_new(&player.events);
        let Some(position) = avatars
            .iter()
            .find(|(a, _)| a.player == entity)
            .map(|(_, t)| t.translation())
        else {
            continue;
        };
        let surface = |body: Entity| {
            if rocks.contains(body) {
                (&sounds.step_rock, &sounds.land_rock)
            } else {
                (&sounds.step_hull, &sounds.land_hull)
            }
        };
        for event in events {
            let pitch = rng.random_range(0.92..1.08);
            match event {
                PlayerEvent::Landed { speed, body } if Some(body) == shared => {
                    let volume = (0.4 + speed * 0.2).min(1.2);
                    play_at(&mut commands, surface(body).1, volume, pitch, position);
                }
                PlayerEvent::Impact { speed, body } if Some(body) == shared => {
                    let volume = (speed * 0.25).min(1.4);
                    play_at(&mut commands, &sounds.impact, volume, pitch * 0.9, position);
                }
                PlayerEvent::SurfaceChanged { body } | PlayerEvent::BodyChanged { body }
                    if Some(body) == shared =>
                {
                    play_at(&mut commands, surface(body).0, 0.8, pitch * 0.85, position);
                }
                PlayerEvent::GrappleAttached { body, .. } if Some(body) == shared => {
                    // Heard where the hook struck the body.
                    let anchor = player
                        .grapple
                        .filter(|g| g.body == body)
                        .and_then(|g| Some(poses.get(body).ok()?.transform_point(g.anchor)))
                        .unwrap_or(position);
                    play_at(&mut commands, &sounds.grapple_attach, 0.6, pitch, anchor);
                }
                _ => {}
            }
        }
        match player.mode {
            Mode::Booted { body, velocity, .. } if Some(body) == shared => {
                hearing.stride += velocity.length() * dt;
                if hearing.stride > STEP_LENGTH {
                    hearing.stride -= STEP_LENGTH;
                    let speed_gain = (velocity.length() / tuning.walk_speed).clamp(0.4, 1.3);
                    play_at(
                        &mut commands,
                        surface(body).0,
                        0.22 * speed_gain,
                        rng.random_range(0.9..1.1),
                        position,
                    );
                }
            }
            _ => hearing.stride = STEP_LENGTH * 0.6,
        }
    }
}

pub fn play_event_sounds(
    mut commands: Commands,
    mut events: MessageReader<Presented>,
    sounds: Res<Sounds>,
    rocks: Query<(), With<Asteroid>>,
) {
    let mut rng = rand::rng();
    for Presented(event) in events.read() {
        let pitch = rng.random_range(0.92..1.08);
        match *event {
            PlayerEvent::Landed { speed, body } => {
                let sound = if rocks.contains(body) {
                    &sounds.land_rock
                } else {
                    &sounds.land_hull
                };
                play(&mut commands, sound, (0.4 + speed * 0.2).min(1.2), pitch);
            }
            PlayerEvent::Impact { speed, .. } => {
                play(
                    &mut commands,
                    &sounds.impact,
                    (speed * 0.25).min(1.4),
                    pitch * 0.9,
                );
            }
            PlayerEvent::Detached(Detach::RippedOff(_) | Detach::Yanked(_)) => {
                play(&mut commands, &sounds.release, 0.9, pitch * 0.8);
                play(&mut commands, &sounds.warning, 0.7, 1.0);
            }
            PlayerEvent::Detached(_) => play(&mut commands, &sounds.release, 0.6, pitch),
            PlayerEvent::SurfaceChanged { body } | PlayerEvent::BodyChanged { body } => {
                let sound = if rocks.contains(body) {
                    &sounds.step_rock
                } else {
                    &sounds.step_hull
                };
                play(&mut commands, sound, 0.8, pitch * 0.85);
            }
            PlayerEvent::GrappleAttached { .. } => {
                play(&mut commands, &sounds.grapple_fire, 0.6, pitch);
                play(&mut commands, &sounds.grapple_attach, 0.6, pitch);
            }
            PlayerEvent::GrappleMissed => {
                play(&mut commands, &sounds.grapple_fire, 0.5, pitch * 1.1)
            }
            PlayerEvent::GrappleReleased => {
                play(&mut commands, &sounds.grapple_release, 0.6, pitch)
            }
            PlayerEvent::RopeSnapped => play(&mut commands, &sounds.rope_snap, 0.9, pitch),
            PlayerEvent::BootsToggled(on) | PlayerEvent::AssistToggled(on) => {
                play(
                    &mut commands,
                    if on {
                        &sounds.click_on
                    } else {
                        &sounds.click_off
                    },
                    0.5,
                    1.0,
                );
            }
            PlayerEvent::ReferenceSet { .. } => play(&mut commands, &sounds.click_on, 0.5, 1.4),
            PlayerEvent::NoReferenceTarget => play(&mut commands, &sounds.click_off, 0.4, 0.7),
        }
    }
}

/// Footsteps, jetpack hiss, reel whine, and the rope going taut.
pub fn play_continuous_sounds(
    mut commands: Commands,
    time: Res<Time>,
    tuning: Res<Tuning>,
    sounds: Res<Sounds>,
    mut state: ResMut<AudioState>,
    player: Single<(&Player, &PlayerTelemetry), With<LocalPlayer>>,
    rocks: Query<(), With<Asteroid>>,
    mut jet: Query<&mut AudioSink, (With<JetpackLoop>, Without<ReelLoop>)>,
    mut reel: Query<&mut AudioSink, (With<ReelLoop>, Without<JetpackLoop>)>,
) {
    let (player, telemetry) = *player;
    let dt = time.delta_secs();

    if let Mode::Booted { body, velocity, .. } = player.mode {
        state.stride += velocity.length() * dt;
        if state.stride > STEP_LENGTH {
            state.stride -= STEP_LENGTH;
            let sound = if rocks.contains(body) {
                &sounds.step_rock
            } else {
                &sounds.step_hull
            };
            let speed_gain = (velocity.length() / tuning.walk_speed).clamp(0.4, 1.3);
            play(
                &mut commands,
                sound,
                0.22 * speed_gain,
                rand::rng().random_range(0.9..1.1),
            );
        }
    } else {
        state.stride = STEP_LENGTH * 0.6;
    }

    if let Ok(mut sink) = jet.single_mut() {
        let target = (telemetry.thrust * 0.22).min(0.55);
        let current = sink.volume().to_linear();
        sink.set_volume(Volume::Linear(
            current + (target - current) * (1.0 - (-dt * 18.0).exp()),
        ));
        sink.set_speed(0.9 + telemetry.thrust.min(2.0) * 0.08);
    }

    let tension = player.grapple.map(|g| g.tension).unwrap_or(0.0);
    // The winch sounds like cable running through it: louder and higher the
    // faster it winds in or pays out, silent when the cable stops (a stalled
    // winch under load included).
    let length = player.grapple.map(|g| g.length);
    let running = match (state.last_rope_length, length) {
        (Some(last), Some(now)) if dt > 0.0 => (now - last).abs() / dt,
        _ => 0.0,
    };
    state.last_rope_length = length;
    // Rope length changes in fixed ticks; smooth over frames between them.
    state.cable_speed += (running - state.cable_speed) * (1.0 - (-dt * 12.0).exp());
    if let Ok(mut sink) = reel.single_mut() {
        const FULL_SPEED: f32 = 20.0;
        let run = (state.cable_speed / FULL_SPEED).clamp(0.0, 1.5);
        let target = if run > 0.02 { 0.06 + 0.24 * run.min(1.0) } else { 0.0 };
        let current = sink.volume().to_linear();
        sink.set_volume(Volume::Linear(
            current + (target - current) * (1.0 - (-dt * 20.0).exp()),
        ));
        sink.set_speed(0.7 + 0.45 * run);
    }

    // The rope snapping taut: a twang scaled by the jump in tension. Only
    // after real slack, and not more than twice a second.
    let taut_threshold = tuning.player_mass * 2.0;
    let was_slack = state.slack_time > 0.25;
    state.twang_cooldown -= dt;
    state.slack_time = if tension < tuning.player_mass * 0.5 {
        state.slack_time + dt
    } else {
        0.0
    };
    if state.last_tension < taut_threshold
        && tension >= taut_threshold
        && was_slack
        && state.twang_cooldown <= 0.0
    {
        state.twang_cooldown = 0.5;
        let strength = (tension / (tuning.player_mass * 20.0)).clamp(0.3, 1.2);
        play(
            &mut commands,
            &sounds.rope_taut,
            strength,
            rand::rng().random_range(0.9..1.1),
        );
    }
    state.last_tension = tension;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_and_length() {
        let bytes = wav(&[0.0, 0.5, -0.5, 1.0]);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(bytes.len(), 44 + 4 * 2);
    }

    #[test]
    fn loops_are_seamless() {
        let mut synth = Synth {
            rng: ChaCha8Rng::seed_from_u64(1),
        };
        for samples in [
            normalize(synth.noise_loop(2.0, 650.0, 2, 1.0), 0.9),
            normalize(synth.winch_loop(), 0.9),
        ] {
            let jump = (samples[0] - samples[samples.len() - 1]).abs();
            let typical =
                samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / samples.len() as f32;
            assert!(
                jump < typical * 6.0 + 0.02,
                "loop seam jump {jump} vs typical step {typical}"
            );
        }
    }
}
