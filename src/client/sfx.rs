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

use crate::sim::vitals::HitLog;
use crate::sim::weapons::{Family, GunKind};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::sim::asteroids::Asteroid;
use crate::sim::combat::{Beams, Combatant, TURRETS, beam_on};
use crate::sim::mission::{Mission, Phase};
use crate::sim::ship::{HomeShip, ShipHull};
use crate::sim::SimTick;
use super::events::Presented;
use crate::sim::events::{Detach, PlayerEvent};
use super::events::EventCursor;
use super::render::Avatar;
use super::LocalPlayer;
use crate::sim::player::{Mode, Player, PlayerTelemetry};
use crate::sim::tuning::Tuning;

pub(crate) const RATE: u32 = 44_100;

/// Distance walked per footstep. Shared with the camera's head bob.
pub const STEP_LENGTH: f32 = 1.0;

/// Encodes mono samples in [-1, 1] as a 16-bit WAV file.
pub(crate) fn wav(samples: &[f32]) -> Vec<u8> {
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
pub(crate) struct Synth {
    pub(crate) rng: ChaCha8Rng,
}

impl Synth {
    pub(crate) fn buffer(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * RATE as f32) as usize]
    }

    pub(crate) fn noise(&mut self) -> f32 {
        self.rng.random_range(-1.0..1.0)
    }

    /// Low-passed noise burst with an exponential decay: thuds and footfalls.
    pub(crate) fn thud(&mut self, seconds: f32, decay: f32, cutoff: f32, gain: f32) -> Vec<f32> {
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
    pub(crate) fn clank(&mut self, seconds: f32, base: f32, decay: f32, gain: f32) -> Vec<f32> {
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
    pub(crate) fn sweep(&mut self, seconds: f32, from: f32, to: f32, noise: f32, gain: f32) -> Vec<f32> {
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
    pub(crate) fn beep(&mut self, seconds: f32, freq: f32, gain: f32) -> Vec<f32> {
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
    /// Heavy electrical drone: mains-hum harmonics (integer Hz, so the loop
    /// is exact), a slow throb, a thin whine and a low rumble of noise.
    fn hum_loop(&mut self) -> Vec<f32> {
        let len = RATE as usize * 2;
        let amps = [(1, 1.0), (2, 0.7), (3, 0.45), (4, 0.3), (6, 0.2), (8, 0.12)];
        let rumble = self.noise_loop(2.0, 400.0, 2, 0.5);
        (0..len)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let tone: f32 = amps
                    .iter()
                    .map(|(h, a)| a * (TAU * 55.0 * *h as f32 * t).sin())
                    .sum();
                let throb = 1.0 + 0.2 * (TAU * 7.0 * t).sin();
                let whine = 0.06 * (TAU * 3300.0 * t).sin() * (1.0 + (TAU * 11.0 * t).sin());
                (tone * 0.5 * throb).tanh() + whine + rumble[i] * 0.3
            })
            .collect()
    }

    /// Sparse random pops ringing through a hiss: electrical crackle.
    pub(crate) fn crackle(&mut self, seconds: f32, density: f32, decay_per_sample: f32) -> Vec<f32> {
        let a = 1.0 - (-TAU * 3000.0 / RATE as f32).exp();
        let mut lp = 0.0;
        let mut pop = 0.0f32;
        let mut out = Self::buffer(seconds);
        for s in out.iter_mut() {
            let x = self.noise();
            lp += a * (x - lp);
            if self.rng.random_range(0.0..1.0) < density {
                pop = self.rng.random_range(0.4..1.0);
            }
            pop *= decay_per_sample;
            // Highpassed noise, mostly gated by the pops.
            *s = (x - lp) * (0.15 + pop * 1.6);
        }
        out
    }

    /// Looping shield sizzle: crackle that never settles.
    fn sizzle_loop(&mut self) -> Vec<f32> {
        let len = RATE as usize * 2;
        let raw = self.crackle(2.5, 0.0012, 0.996);
        Self::crossfade_loop(raw, len)
    }

    /// Looping hull grind: low rumble chewed by a 9 Hz stutter and grit.
    fn grind_loop(&mut self) -> Vec<f32> {
        let len = RATE as usize * 2;
        let (a_low, a_grit) = (
            1.0 - (-TAU * 160.0 / RATE as f32).exp(),
            1.0 - (-TAU * 420.0 / RATE as f32).exp(),
        );
        let (mut l1, mut l2, mut g) = (0.0f32, 0.0f32, 0.0f32);
        let raw: Vec<f32> = (0..len + len / 4)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                let n = self.noise();
                l1 += a_low * (n - l1);
                l2 += a_low * (l1 - l2);
                g += a_grit * (n - g);
                let chew = 0.65 + 0.35 * (TAU * 9.0 * t).sin();
                (l2 * 5.0 + (g * 4.0).tanh() * 0.35) * chew
            })
            .collect();
        Self::crossfade_loop(raw, len)
    }

    /// Looping alarm klaxon: two rising whoops, silent at both ends of the
    /// 1.6 s loop.
    fn klaxon_loop(&mut self) -> Vec<f32> {
        let whoop = (RATE as f32 * 0.8) as usize;
        let mut phase = 0.0f32;
        let one: Vec<f32> = (0..whoop)
            .map(|i| {
                let k = i as f32 / whoop as f32;
                let rise = (k / 0.85).min(1.0);
                phase += TAU * (420.0 + 340.0 * rise * rise) / RATE as f32;
                let env = (i as f32 / 200.0).min(1.0) * ((1.0 - k) / 0.08).clamp(0.0, 1.0);
                ((phase.sin() * 4.0).tanh() * 0.8 + (phase * 2.0).sin() * 0.2) * env
            })
            .collect();
        concat(one.clone(), one)
    }

    /// Switch-gear press: a thud into a heavy metal ka-chunk.
    fn button_press(&mut self) -> Vec<f32> {
        let chunk = mix(self.thud(0.1, 45.0, 900.0, 0.9), self.clank(0.16, 320.0, 55.0, 0.5));
        let clack = concat(Self::buffer(0.05), self.clank(0.1, 690.0, 80.0, 0.3));
        mix(chunk, clack)
    }

    /// Jump/warp: noise and tone swelling upward, then cut off with a thump.
    fn warp(&mut self) -> Vec<f32> {
        let mut out = Self::buffer(0.75);
        let n = out.len() as f32;
        let (mut lp, mut phase) = (0.0f32, 0.0f32);
        for (i, s) in out.iter_mut().enumerate() {
            let k = i as f32 / n;
            let cutoff = 300.0 + 4700.0 * k * k;
            lp += (1.0 - (-TAU * cutoff / RATE as f32).exp()) * (self.noise() - lp);
            phase += TAU * (120.0 + 1300.0 * k * k) / RATE as f32;
            let env = if k < 0.82 {
                (k / 0.82).powf(1.5)
            } else {
                ((1.0 - k) / 0.18).powi(2)
            };
            *s = (lp * 1.6 + phase.sin() * 0.35) * env;
        }
        mix(out, concat(Self::buffer(0.6), self.thud(0.2, 20.0, 400.0, 1.0)))
    }

    /// A blast: a falling sub-bass boom under a noise burst whose lowpass
    /// closes from `bright` Hz. Low `bright` is the muffled felt-through-
    /// the-hull version.
    pub(crate) fn explosion(&mut self, seconds: f32, bright: f32, boom: f32) -> Vec<f32> {
        let mut out = Self::buffer(seconds);
        let (mut lp, mut phase) = (0.0f32, 0.0f32);
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            let cutoff = (bright * (-t * 3.0).exp()).max(90.0);
            lp += (1.0 - (-TAU * cutoff / RATE as f32).exp()) * (self.noise() - lp);
            phase += TAU * (28.0 + 80.0 * (-t * 2.5).exp()) / RATE as f32;
            let attack = (t / 0.003).min(1.0);
            *s = (lp * 2.5 * (-t * 2.2).exp() + phase.sin() * boom * (-t * 1.6).exp()) * attack;
        }
        out
    }

    /// Structure groaning under load: a wavering low tone with harmonics.
    fn groan(&mut self) -> Vec<f32> {
        let mut out = Self::buffer(1.6);
        let n = out.len() as f32;
        let mut phase = 0.0f32;
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            let k = i as f32 / n;
            phase += TAU * (72.0 - 22.0 * k + 3.0 * (TAU * 5.0 * t).sin()) / RATE as f32;
            let tone = phase.sin() + 0.5 * (phase * 2.0).sin() + 0.3 * (phase * 3.0).sin();
            let env = (std::f32::consts::PI * k).sin().powi(2);
            *s = (tone * 1.2).tanh() * env * (0.75 + 0.25 * (TAU * 13.0 * t).sin());
        }
        mix(out, self.thud(1.2, 3.0, 90.0, 0.8))
    }

    /// A metal creak: a squealing tone wandering in pitch.
    fn creak(&mut self) -> Vec<f32> {
        let mut out = Self::buffer(0.6);
        let n = out.len() as f32;
        let (mut phase, mut wander) = (0.0f32, 0.0f32);
        for (i, s) in out.iter_mut().enumerate() {
            let t = i as f32 / RATE as f32;
            let k = i as f32 / n;
            wander += 0.0015 * (self.noise() - wander);
            phase += TAU * (190.0 + 70.0 * (TAU * 6.0 * t).sin() + 4000.0 * wander) / RATE as f32;
            let env = (std::f32::consts::PI * k).sin();
            *s = (phase.sin() * 3.0).tanh() * env * (0.5 + 0.5 * self.noise().abs());
        }
        out
    }
}

pub(crate) fn normalize(mut samples: Vec<f32>, peak: f32) -> Vec<f32> {
    let max = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if max > 1e-6 {
        samples.iter_mut().for_each(|s| *s *= peak / max);
    }
    samples
}

pub(crate) fn concat(a: Vec<f32>, b: Vec<f32>) -> Vec<f32> {
    let mut a = a;
    a.extend(b);
    a
}

pub(crate) fn mix(a: Vec<f32>, b: Vec<f32>) -> Vec<f32> {
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
    /// Your own gun, through your hand and suit: a thump more than a bang.
    shot_smg: Handle<AudioSource>,
    shot_rifle: Handle<AudioSource>,
    /// A shotgun's boom: low and long, with a rattle.
    shot_shotgun: Handle<AudioSource>,
    /// A sniper's crack: sharp, then a ringing tail.
    shot_sniper: Handle<AudioSource>,
    /// A plasma bolt leaving: a falling, hollow whump.
    shot_plasma: Handle<AudioSource>,
    /// An arc: a snap of crackle.
    shot_arc: Handle<AudioSource>,
    /// A beam: a short, bright hum (played as it fires, a few times a
    /// second).
    shot_laser: Handle<AudioSource>,
    /// Your shots landing: a tick, a brighter one on a weak spot, a thump
    /// for a kill. (Feedback, not something heard across space.)
    hit: Handle<AudioSource>,
    hit_weak: Handle<AudioSource>,
    kill: Handle<AudioSource>,
    grapple_fire: Handle<AudioSource>,
    grapple_attach: Handle<AudioSource>,
    grapple_release: Handle<AudioSource>,
    rope_taut: Handle<AudioSource>,
    rope_snap: Handle<AudioSource>,
    warning: Handle<AudioSource>,
    click_on: Handle<AudioSource>,
    click_off: Handle<AudioSource>,
    button: Handle<AudioSource>,
    warp: Handle<AudioSource>,
    fire_start: Handle<AudioSource>,
    fire_stop: Handle<AudioSource>,
    thump_shield: Handle<AudioSource>,
    thump_hull: Handle<AudioSource>,
    sizzle_burst: Handle<AudioSource>,
    blast_close: Handle<AudioSource>,
    blast_far: Handle<AudioSource>,
    creak: Handle<AudioSource>,
    groan: Handle<AudioSource>,
}

/// Looping combat sounds, faded by [`play_combat_sounds`].
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum CombatLoop {
    /// Turret `n` firing through your hull.
    Hum(u8),
    /// A beam fizzing on the shield.
    Sizzle,
    /// A beam grinding into the hull.
    Grind,
    Klaxon,
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

impl Sounds {
    /// A hull's clank, through your suit: what a blow sounds like too.
    pub fn impact(&self) -> &Handle<AudioSource> {
        &self.impact
    }
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
        shot_smg: add(mix(
            synth.thud(0.09, 60.0, 900.0, 0.9),
            synth.clank(0.06, 600.0, 60.0, 0.3),
        )),
        shot_rifle: add(mix(
            synth.thud(0.16, 45.0, 700.0, 1.0),
            synth.clank(0.1, 400.0, 40.0, 0.45),
        )),
        shot_shotgun: add(mix(
            synth.thud(0.32, 18.0, 420.0, 1.0),
            mix(synth.clank(0.2, 220.0, 22.0, 0.35), synth.crackle(0.12, 0.02, 0.996)),
        )),
        shot_sniper: add(mix(
            synth.thud(0.12, 70.0, 2400.0, 0.9),
            synth.clank(0.6, 520.0, 6.0, 0.45),
        )),
        shot_plasma: add(mix(
            synth.sweep(0.3, 420.0, 90.0, 0.15, 0.8),
            synth.thud(0.22, 22.0, 300.0, 0.6),
        )),
        shot_arc: add(mix(synth.crackle(0.18, 0.08, 0.993), synth.sweep(0.12, 2600.0, 1400.0, 0.3, 0.35))),
        shot_laser: add(mix(synth.beep(0.14, 1320.0, 0.35), synth.sweep(0.14, 2600.0, 2500.0, 0.2, 0.25))),
        hit: add(synth.clank(0.05, 1900.0, 90.0, 0.4)),
        hit_weak: add(mix(
            synth.clank(0.09, 2600.0, 45.0, 0.5),
            synth.clank(0.07, 3900.0, 60.0, 0.3),
        )),
        kill: add(mix(
            synth.thud(0.25, 40.0, 500.0, 0.9),
            synth.clank(0.18, 1300.0, 25.0, 0.5),
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
        button: add(synth.button_press()),
        warp: add(synth.warp()),
        fire_start: add(mix(
            synth.sweep(0.25, 150.0, 1200.0, 0.3, 0.6),
            synth.thud(0.2, 15.0, 300.0, 0.7),
        )),
        fire_stop: add(synth.sweep(0.5, 600.0, 60.0, 0.15, 0.6)),
        thump_shield: add(mix(
            synth.thud(0.25, 14.0, 500.0, 0.8),
            synth.sweep(0.2, 1500.0, 400.0, 0.7, 0.3),
        )),
        thump_hull: add(mix(
            synth.thud(0.45, 9.0, 260.0, 1.0),
            synth.clank(0.4, 95.0, 10.0, 0.5),
        )),
        sizzle_burst: add(synth.crackle(0.8, 0.004, 0.992)),
        blast_close: add(synth.explosion(2.6, 3500.0, 1.2)),
        blast_far: add(synth.explosion(2.2, 150.0, 1.6)),
        creak: add(synth.creak()),
        groan: add(synth.groan()),
    };
    let jet = add(synth.noise_loop(2.0, 650.0, 2, 1.0));
    let whine = add(synth.winch_loop());
    let hum = add(synth.hum_loop());
    let sizzle = add(synth.sizzle_loop());
    let grind = add(synth.grind_loop());
    let klaxon = add(synth.klaxon_loop());
    let looping = |volume: f32| PlaybackSettings {
        mode: PlaybackMode::Loop,
        volume: Volume::Linear(volume),
        ..default()
    };
    commands.spawn((JetpackLoop, AudioPlayer(jet), looping(0.0)));
    commands.spawn((ReelLoop, AudioPlayer(whine), looping(0.0)));
    for index in 0..TURRETS {
        commands.spawn((CombatLoop::Hum(index), AudioPlayer(hum.clone()), looping(0.0)));
    }
    commands.spawn((CombatLoop::Sizzle, AudioPlayer(sizzle), looping(0.0)));
    commands.spawn((CombatLoop::Grind, AudioPlayer(grind), looping(0.0)));
    commands.spawn((CombatLoop::Klaxon, AudioPlayer(klaxon), looping(0.0)));
    commands.insert_resource(sounds);
    commands.init_resource::<AudioState>();
    commands.init_resource::<CombatAudio>();
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
pub(crate) const STRUCTURE_SCALE: f32 = 0.25;
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
                PlayerEvent::Pressed { body, .. } if Some(body) == shared => {
                    play_at(&mut commands, &sounds.button, 0.8, pitch, position);
                }
                PlayerEvent::GrappleAttached { body, .. } if Some(body) == shared => {
                    // Heard where the hook struck the body.
                    let anchor = player
                        .grapple
                        .filter(|g| g.target == body)
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
    mut combat: ResMut<CombatAudio>,
    rocks: Query<(), With<Asteroid>>,
    time: Res<Time>,
    mut last_boom: Local<[f32; 2]>,
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
            // The ship underfoot blew apart: the blast is right here, through
            // the plating, and then the boots let go.
            PlayerEvent::Detached(Detach::Blasted) => {
                play(&mut commands, &sounds.blast_close, 1.6, pitch);
                play(&mut commands, &sounds.release, 0.5, pitch * 0.8);
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
            PlayerEvent::Pressed { .. } => play(&mut commands, &sounds.button, 0.8, pitch),
            PlayerEvent::Respawned => {
                combat.respawn_age = 0.0;
                play(&mut commands, &sounds.warp, 0.7, 1.0);
            }
            PlayerEvent::BeamHit => {
                play(&mut commands, &sounds.sizzle_burst, 0.9, pitch);
                play(&mut commands, &sounds.impact, 1.0, 0.6);
            }
            // The suit gives out: an alarm, then the respawn's warp.
            PlayerEvent::Died => play(&mut commands, &sounds.warning, 1.0, 0.7),
            // Gear clattering into place.
            PlayerEvent::PickedUp { .. } => {
                play(&mut commands, &sounds.button, 0.7, pitch * 0.8);
                play(&mut commands, &sounds.grapple_attach, 0.5, pitch * 0.7);
            }
            // Something set down; and the pack refusing more.
            PlayerEvent::Dropped(_) => play(&mut commands, &sounds.land_hull, 0.35, pitch * 1.4),
            PlayerEvent::InventoryFull | PlayerEvent::NotYours => play(&mut commands, &sounds.click_off, 0.8, pitch * 0.7),
            // A beam's hum comes on with it (and keeps on: `beam_hum`).
            PlayerEvent::BeamStarted { .. } => play(&mut commands, &sounds.shot_laser, 0.7, pitch),
            PlayerEvent::BeamStopped { .. } => {}
            PlayerEvent::Fired { hand, gun, kind, .. } => {
                let sound = match (kind.family(), gun) {
                    (Family::Plasma, _) => &sounds.shot_plasma,
                    (Family::Arc, _) => &sounds.shot_arc,
                    (Family::Laser, _) => &sounds.shot_laser,
                    (_, GunKind::Shotgun) => {
                        // Eight pellets, one boom (each hand its own: two
                        // shotguns, two booms).
                        let last = &mut last_boom[hand as usize];
                        if time.elapsed_secs() - *last < 0.05 {
                            continue;
                        }
                        *last = time.elapsed_secs();
                        &sounds.shot_shotgun
                    }
                    (_, GunKind::Sniper) => &sounds.shot_sniper,
                    (_, GunKind::Smg | GunKind::SwarmerSprayer) => &sounds.shot_smg,
                    (_, GunKind::Rifle | GunKind::GunnerRifle | GunKind::MarksmanRifle | GunKind::TurretCannon) => &sounds.shot_rifle,
                };
                play(&mut commands, sound, 0.7, pitch);
            }
        }
    }
}

/// A held beam hums on, a few times a second, each hand's its own.
pub fn beam_hum(
    mut commands: Commands,
    time: Res<Time>,
    sounds: Res<Sounds>,
    player: Option<Single<&Player, With<LocalPlayer>>>,
    mut last: Local<[f32; 2]>,
) {
    let Some(player) = player else { return };
    let now = time.elapsed_secs();
    for (i, state) in player.loadout.hands.iter().enumerate() {
        if state.beam.is_some() && now - last[i] >= 0.12 {
            last[i] = now;
            play(&mut commands, &sounds.shot_laser, 0.6, 1.0);
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

/// Client-side state for the battle sounds.
#[derive(Resource)]
pub struct CombatAudio {
    /// Per turret: whether your ship's beam was firing / hitting last frame.
    firing: [bool; TURRETS as usize],
    hit: [bool; TURRETS as usize],
    /// Ships seen last frame, with their integrity: one that vanishes with
    /// the mission decided was destroyed.
    ships: Vec<(Entity, f32)>,
    /// The body you stood on last frame.
    body: Option<Entity>,
    /// Seconds since the last scenario switch (ships vanish then, too).
    respawn_age: f32,
    creak_timer: f32,
}

impl Default for CombatAudio {
    fn default() -> Self {
        Self {
            firing: default(),
            hit: default(),
            ships: Vec::new(),
            body: None,
            respawn_age: 10.0,
            creak_timer: 4.0,
        }
    }
}

/// Hull integrity below which our ship starts to groan.
const CREAK_BELOW: f32 = 0.3;

/// Ship-to-ship battle sounds, all heard through the hull you stand on. The
/// structure rule: your own ship's turrets hum through it, beams on your
/// ship thump, sizzle (shield) or grind (hull) through it, and a raider
/// whose shield is down blares its klaxon through its own deck. Everything
/// is read from replicated state (combatants, the beams every peer traces,
/// the tick), so it is the same on a host and on a client.
///
/// Destruction: standing on the ship (see the `Blasted` event) is a huge
/// close blast. Standing on another ship when one is destroyed, you feel a
/// deep muffled thump through your own hull. In EVA you hear nothing: there
/// is nothing to carry it.
#[allow(clippy::too_many_arguments)]
pub fn play_combat_sounds(
    mut commands: Commands,
    time: Res<Time>,
    tick: Res<SimTick>,
    beams: Res<Beams>,
    sounds: Res<Sounds>,
    mut state: ResMut<CombatAudio>,
    player: Single<&Player, With<LocalPlayer>>,
    missions: Query<&Mission>,
    ships: Query<(Entity, &Combatant, &ShipHull, Has<HomeShip>)>,
    mut loops: Query<(&CombatLoop, &mut AudioSink)>,
) {
    let dt = time.delta_secs();
    let body = player.mode.body();
    let mut rng = rand::rng();
    state.respawn_age += dt;

    // A ship that disappeared because it was destroyed.
    let decided = missions.single().is_ok_and(|m| m.phase != Phase::Underway);
    let aboard_a_ship = body.is_some_and(|b| ships.contains(b));
    for &(ship, integrity) in &state.ships {
        if ships.contains(ship)
            || integrity > 0.15
            || !decided
            || state.respawn_age < 1.0
            || state.body == Some(ship)
            || !aboard_a_ship
        {
            continue;
        }
        play(&mut commands, &sounds.blast_far, 0.6, rng.random_range(0.9..1.1));
    }
    state.ships = ships.iter().map(|(e, c, ..)| (e, c.integrity)).collect();
    state.body = body;

    let firing_ship = body.is_some_and(|b| beams.0.iter().any(|x| x.shooter == b));
    let hits: Vec<_> = beams.0.iter().filter(|x| Some(x.target) == body).collect();
    let (shield_hit, hull_hit) = (
        hits.iter().any(|x| x.shielded),
        hits.iter().any(|x| !x.shielded),
    );
    for i in 0..TURRETS {
        let on = beam_on(i, tick.0);
        let (fire, hit) = (on && firing_ship, on && !hits.is_empty());
        let pitch = rng.random_range(0.95..1.05);
        let i = i as usize;
        if fire != state.firing[i] {
            let sound = if fire { &sounds.fire_start } else { &sounds.fire_stop };
            play(&mut commands, sound, if fire { 0.5 } else { 0.35 }, pitch);
        }
        if hit && !state.hit[i] {
            if hull_hit {
                play(&mut commands, &sounds.thump_hull, 1.0, pitch);
            } else {
                play(&mut commands, &sounds.thump_shield, 0.8, pitch);
            }
        }
        (state.firing[i], state.hit[i]) = (fire, hit);
    }

    // The klaxon: loud on the carrier's own deck, a faint suit-radio echo of
    // it everywhere else.
    let alarm = ships
        .iter()
        .find(|(_, c, h, _)| h.design().hostile && !c.shield)
        .map(|(e, ..)| e);
    let klaxon = match alarm {
        Some(raider) if body == Some(raider) => 0.3,
        Some(_) => 0.05,
        None => 0.0,
    };
    for (kind, mut sink) in &mut loops {
        let (target, rate) = match *kind {
            CombatLoop::Hum(i) => (if state.firing[i as usize] { 0.28 } else { 0.0 }, 30.0),
            CombatLoop::Sizzle => (if shield_hit { 0.22 } else { 0.0 }, 25.0),
            CombatLoop::Grind => (if hull_hit { 0.4 } else { 0.0 }, 25.0),
            CombatLoop::Klaxon => (klaxon, 8.0),
        };
        if let CombatLoop::Hum(i) = *kind {
            // The turrets are a little out of tune with each other.
            sink.set_speed(1.0 + 0.12 * i as f32);
        }
        let current = sink.volume().to_linear();
        sink.set_volume(Volume::Linear(
            current + (target - current) * (1.0 - (-dt * rate).exp()),
        ));
    }

    // A badly hurt ship groans.
    let hurt = body
        .and_then(|b| ships.get(b).ok())
        .filter(|(_, c, _, home)| *home && c.integrity < CREAK_BELOW);
    match hurt {
        Some((_, combatant, ..)) => {
            state.creak_timer -= dt;
            if state.creak_timer <= 0.0 {
                let severity = 1.0 - combatant.integrity / CREAK_BELOW;
                let volume = 0.25 + 0.4 * severity;
                let sound = if rng.random_bool(0.4) { &sounds.groan } else { &sounds.creak };
                play(&mut commands, sound, volume, rng.random_range(0.85..1.15));
                state.creak_timer = rng.random_range(3.0..9.0) * (1.0 - 0.5 * severity);
            }
        }
        None => state.creak_timer = state.creak_timer.max(2.0),
    }
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
            normalize(synth.hum_loop(), 0.9),
            normalize(synth.sizzle_loop(), 0.9),
            normalize(synth.grind_loop(), 0.9),
            normalize(synth.klaxon_loop(), 0.9),
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

    #[test]
    fn combat_sounds_are_sane() {
        let mut synth = Synth {
            rng: ChaCha8Rng::seed_from_u64(2),
        };
        let sounds = [
            ("button", synth.button_press(), 0.1, 0.5),
            ("warp", synth.warp(), 0.7, 1.0),
            ("blast_close", synth.explosion(2.6, 3500.0, 1.2), 2.0, 3.0),
            ("blast_far", synth.explosion(2.2, 150.0, 1.6), 2.0, 3.0),
            ("groan", synth.groan(), 1.5, 2.0),
            ("creak", synth.creak(), 0.5, 1.0),
            ("sizzle", synth.crackle(0.8, 0.004, 0.992), 0.7, 1.0),
        ];
        for (name, samples, min_s, max_s) in sounds {
            let seconds = samples.len() as f32 / RATE as f32;
            assert!((min_s..=max_s).contains(&seconds), "{name}: {seconds} s");
            assert!(samples.iter().all(|s| s.is_finite()), "{name}: non-finite");
            let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.05, "{name}: silent (peak {peak})");
        }
        // The klaxon starts and ends in silence, so it loops cleanly.
        let klaxon = synth.klaxon_loop();
        assert!(klaxon[0].abs() < 0.01 && klaxon[klaxon.len() - 1].abs() < 0.01);
    }
}

/// Sounds for your shots landing, from what the authority says you hit.
pub fn play_hit_sounds(
    mut commands: Commands,
    sounds: Res<Sounds>,
    log: Option<Single<&HitLog, With<LocalPlayer>>>,
    mut seen: Local<Option<u32>>,
) {
    let Some(log) = log else { return };
    if let Some(count) = *seen
        && let Some(hit) = log.since(count).copied().max_by_key(|h| (h.kill, h.weak))
    {
        let sound = if hit.kill {
            &sounds.kill
        } else if hit.weak {
            &sounds.hit_weak
        } else {
            &sounds.hit
        };
        play(&mut commands, sound, 0.6, 1.0);
    }
    *seen = Some(log.count);
}
