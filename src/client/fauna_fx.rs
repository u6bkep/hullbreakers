//! How the field's life looks and sounds (see `sim::fauna`). The limpet,
//! a brain-driven body, is drawn and animated with the robots (see
//! `render`, `animate`, `robot_fx`); this is the grazer.
//!
//! A grazer's model is its bell, the flaps round its rim, its fronds (each
//! a chain of three) and its heart. Each pulse is drawn on the tick it
//! happens (every peer knows when: see [`Grazer::last_pulse`]): the rim
//! snaps in and the bell squeezes tall, then both ease back out over the
//! rest of its period; the fronds trail and stream back after the push,
//! waving in a slow travelling wave; the heart beats with it. Nothing here
//! feeds back into the simulation.
//!
//! As everywhere, sound needs a way to you: a grazer's pulses are heard by
//! whoever stands on it or has a rope in it, a low whump through the boots
//! or the line; startled, it moans.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_4, PI};

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::LocalPlayer;
use super::sfx::{Synth, mix, normalize, wav};
use crate::sim::bots::Wrecked;
use crate::sim::fauna::{Grazer, Mood};
use crate::sim::player::Player;
use crate::sim::SimTick;

pub struct FaunaFxPlugin;

impl Plugin for FaunaFxPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_fauna_fx)
            .add_observer(dress_grazer)
            .add_observer(grazer_died)
            .add_systems(
                PostUpdate,
                (rig_grazers, animate_grazers)
                    .chain()
                    .after(super::BodyPoses)
                    .before(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(Update, (grazer_sounds, drift_spores))
            .add_systems(bevy_egui::EguiPrimaryContextPass, label_grazers);
    }
}

/// Where the grazer's model is.
const MODEL: &str = "fauna/grazer/grazer.glb";

/// A grazer as drawn: its model's moving parts (once it's in), and what's
/// been heard of it.
#[derive(Component, Default)]
pub struct GrazerLook {
    parts: Option<HashMap<String, (Entity, Transform)>>,
    /// The last pulse heard, and the mood it was last in.
    heard: u32,
    mood: Option<Mood>,
}

/// The parts a grazer moves.
fn part_names() -> Vec<String> {
    let mut names = vec!["bell".to_string(), "heart".to_string()];
    names.extend((0..8).map(|k| format!("rim_{k}")));
    for i in 0..6 {
        for seg in ["a", "b", "c"] {
            names.push(format!("frond_{i}_{seg}"));
        }
    }
    names
}

fn dress_grazer(add: On<Add, Grazer>, assets: Res<AssetServer>, mut commands: Commands) {
    let model = commands
        .spawn((
            Name::new("Grazer model"),
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(MODEL))),
            Transform::default(),
        ))
        .id();
    commands
        .entity(add.entity)
        .insert((GrazerLook::default(), Visibility::default()))
        .add_child(model);
}

fn rig_grazers(
    mut looks: Query<(Entity, &mut GrazerLook)>,
    children: Query<&Children>,
    names: Query<(&Name, &Transform)>,
) {
    for (grazer, mut look) in &mut looks {
        if look.parts.is_some() {
            continue;
        }
        let wanted = part_names();
        let parts: HashMap<String, (Entity, Transform)> = children
            .iter_descendants(grazer)
            .filter_map(|e| {
                let (name, transform) = names.get(e).ok()?;
                wanted.contains(&name.to_string()).then(|| (name.to_string(), (e, *transform)))
            })
            .collect();
        if parts.len() == wanted.len() {
            look.parts = Some(parts);
        }
    }
}

/// Now, in ticks (with the fraction since the last): the draw clock on a
/// client, the simulation's on a host.
fn now(tick: &SimTick, fixed: &Time<Fixed>) -> f64 {
    tick.0 as f64 + fixed.overstep_fraction() as f64
}

/// How far into its pulse it is (0 at the push, 1 at the next).
fn pulse(grazer: &Grazer, now: f64) -> f32 {
    let tick = now.floor() as u32;
    let since = grazer.last_pulse(tick);
    let k = ((tick.wrapping_sub(since)) as f64 + now.fract()) / grazer.period() as f64;
    k.clamp(0.0, 1.0) as f32
}

/// The squeeze through a pulse: snapping in fast, easing back out.
fn squeeze(k: f32) -> f32 {
    const SNAP: f32 = 0.12;
    if k < SNAP {
        (k / SNAP * PI / 2.0).sin()
    } else {
        (1.0 - (k - SNAP) / (1.0 - SNAP)).powi(2)
    }
}

fn animate_grazers(
    time: Res<Time>,
    tick: Res<SimTick>,
    fixed: Res<Time<Fixed>>,
    grazers: Query<(&Grazer, &GrazerLook)>,
    mut transforms: Query<&mut Transform, Without<GrazerLook>>,
) {
    let now = now(&tick, &fixed);
    let t = time.elapsed_secs();
    for (grazer, look) in &grazers {
        let Some(parts) = &look.parts else { continue };
        let mut pose = |name: &str, turn: Quat, scale: Vec3| {
            if let Some((entity, rest)) = parts.get(name)
                && let Ok(mut transform) = transforms.get_mut(*entity)
            {
                transform.rotation = rest.rotation * turn;
                transform.scale = rest.scale * scale;
            }
        };
        let k = pulse(grazer, now);
        let s = squeeze(k);
        let startled = grazer.mood == Mood::Startled;
        let seed = grazer.seed as f32 * 0.37;
        // The bell squeezes tall and narrow.
        pose("bell", Quat::IDENTITY, Vec3::new(1.0 - 0.07 * s, 1.0 + 0.09 * s, 1.0 - 0.07 * s));
        // The rim: each flap curls in under the bell on the push, a little
        // out of step round the rim (it ripples), easing back.
        // (Each flap's local X is the rim's tangent; turning about +X curls
        // it in.)
        for r in 0..8 {
            let a = r as f32 * FRAC_PI_4;
            let lag = squeeze((k - 0.015 * r as f32).rem_euclid(1.0));
            let idle = 0.05 * (t * 1.3 + a * 2.0 + seed).sin();
            pose(&format!("rim_{r}"), Quat::from_rotation_x(0.75 * lag + idle), Vec3::ONE);
        }
        // The fronds: a wave travelling down each, quicker when it bolts,
        // and drawn in toward the axis (trailing) just after a push. (Each
        // segment's local −Z runs down it and +Y outward: turning about +X
        // swings it out, about Y to the side.)
        let wave = if startled { 3.2 } else { 1.1 };
        let sweep = 0.35 * s;
        for f in 0..6 {
            for (j, seg) in ["a", "b", "c"].into_iter().enumerate() {
                let phase = t * wave - j as f32 * 0.9 + f as f32 * 1.7 + seed;
                let swing = (0.12 + 0.06 * j as f32) * phase.sin();
                let sway = 0.06 * (phase * 0.7 + 1.0).cos();
                let trail = if j == 0 { -sweep } else { -sweep * 0.4 };
                let turn = Quat::from_rotation_x(swing + trail) * Quat::from_rotation_y(sway);
                pose(&format!("frond_{f}_{seg}"), turn, Vec3::ONE);
            }
        }
        // Its heart beats just before each push.
        let beat = squeeze((k + 0.06).rem_euclid(1.0));
        pose("heart", Quat::IDENTITY, Vec3::splat(1.0 + 0.18 * beat));
    }
}

// --- Sound ------------------------------------------------------------------

#[derive(Resource)]
struct FaunaFx {
    /// A grazer's pulse, through its flesh; its moan when startled; it
    /// dying.
    whump: Handle<AudioSource>,
    moan: Handle<AudioSource>,
    sigh: Handle<AudioSource>,
    /// What's left of a dead grazer: glowing spores.
    spore: Handle<Mesh>,
    spore_glow: [Handle<StandardMaterial>; 2],
}

fn setup_fauna_fx(
    mut commands: Commands,
    mut sources: ResMut<Assets<AudioSource>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut synth = Synth {
        rng: ChaCha8Rng::seed_from_u64(29),
    };
    let mut add = |samples: Vec<f32>| {
        sources.add(AudioSource {
            bytes: wav(&normalize(samples, 0.9)).into(),
        })
    };
    let glow = |c: LinearRgba| StandardMaterial {
        base_color: Color::BLACK,
        emissive: c,
        unlit: true,
        ..default()
    };
    commands.insert_resource(FaunaFx {
        whump: add(mix(synth.thud(0.7, 6.0, 140.0, 1.0), synth.sweep(0.6, 90.0, 55.0, 0.2, 0.5))),
        moan: add(mix(synth.sweep(1.4, 120.0, 70.0, 0.15, 0.8), synth.thud(1.0, 3.0, 200.0, 0.4))),
        sigh: add(synth.sweep(2.2, 160.0, 40.0, 0.35, 0.8)),
        spore: meshes.add(Sphere::new(0.06).mesh().ico(1).expect("ico subdivisions in range")),
        spore_glow: [
            materials.add(glow(LinearRgba::rgb(6.0, 0.6, 4.0))),
            materials.add(glow(LinearRgba::rgb(0.8, 4.0, 5.0))),
        ],
    });
}

fn play_at(commands: &mut Commands, sound: &Handle<AudioSource>, volume: f32, speed: f32, at: Vec3) {
    commands.spawn((
        AudioPlayer(sound.clone()),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(volume),
            speed,
            spatial: true,
            spatial_scale: Some(bevy::audio::SpatialScale::new(super::sfx::STRUCTURE_SCALE)),
            ..default()
        },
        Transform::from_translation(at),
    ));
}

/// A grazer's pulses and moans, heard by whoever's on it or roped to it.
fn grazer_sounds(
    tick: Res<SimTick>,
    fx: Option<Res<FaunaFx>>,
    listener: Query<&Player, With<LocalPlayer>>,
    mut grazers: Query<(Entity, &Grazer, &mut GrazerLook, &GlobalTransform)>,
    mut commands: Commands,
) {
    let Some(fx) = fx else { return };
    let me = listener.single().ok();
    for (entity, grazer, mut look, at) in &mut grazers {
        let last = grazer.last_pulse(tick.0);
        let pulsed = last != look.heard;
        look.heard = last;
        let startled = grazer.mood == Mood::Startled && look.mood.is_some_and(|m| m != Mood::Startled);
        look.mood = Some(grazer.mood);
        let touching = me.is_some_and(|p| p.mode.body() == Some(entity) || p.grapple.is_some_and(|g| g.target == entity));
        if !touching {
            continue;
        }
        let pitch = 0.9 + (grazer.seed % 7) as f32 * 0.03;
        if pulsed {
            play_at(&mut commands, &fx.whump, 0.9, pitch, at.translation());
        }
        if startled {
            play_at(&mut commands, &fx.moan, 1.0, pitch, at.translation());
        }
    }
}

// --- Death ------------------------------------------------------------------

/// A spore drifting from a dead grazer, shrinking away.
#[derive(Component)]
struct Spore {
    velocity: Vec3,
    size: f32,
    age: f32,
    life: f32,
}

/// A grazer killed (not tidied away) bursts into a cloud of its glow.
fn grazer_died(
    remove: On<Remove, Grazer>,
    grazers: Query<(&GlobalTransform, Has<Wrecked>, Option<&avian3d::prelude::LinearVelocity>)>,
    listener: Query<&Player, With<LocalPlayer>>,
    fx: Option<Res<FaunaFx>>,
    mut commands: Commands,
) {
    let (Ok((at, wrecked, velocity)), Some(fx)) = (grazers.get(remove.entity), fx) else {
        return;
    };
    if !wrecked {
        return;
    }
    let base = velocity.map_or(Vec3::ZERO, |v| v.0);
    let mut rng = ChaCha8Rng::seed_from_u64(remove.entity.to_bits());
    for i in 0..48 {
        let dir = Vec3::new(rng.random_range(-1.0..1.0), rng.random_range(-0.6..1.0), rng.random_range(-1.0..1.0)).normalize_or(Vec3::Y);
        let from = at.translation() + dir * rng.random_range(0.2..1.4) * Vec3::new(1.4, 0.6, 1.4);
        let size = rng.random_range(0.6..1.6);
        commands.spawn((
            Mesh3d(fx.spore.clone()),
            MeshMaterial3d(fx.spore_glow[i % 2].clone()),
            Transform::from_translation(from).with_scale(Vec3::splat(size)),
            bevy::light::NotShadowCaster,
            Spore {
                velocity: base + dir * rng.random_range(0.4..2.2),
                size,
                age: 0.0,
                life: rng.random_range(2.0..4.5),
            },
        ));
    }
    let touching = listener.single().ok().is_some_and(|p| p.mode.body() == Some(remove.entity) || p.grapple.is_some_and(|g| g.target == remove.entity));
    if touching {
        play_at(&mut commands, &fx.sigh, 1.0, 1.0, at.translation());
    }
}

fn drift_spores(time: Res<Time>, mut spores: Query<(Entity, &mut Spore, &mut Transform)>, mut commands: Commands) {
    let dt = time.delta_secs();
    for (entity, mut spore, mut transform) in &mut spores {
        spore.age += dt;
        if spore.age > spore.life {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation += spore.velocity * dt;
        transform.scale = Vec3::splat(spore.size * (1.0 - spore.age / spore.life).sqrt());
    }
}

// --- The gallery --------------------------------------------------------------

/// In the gallery, its name under it.
fn label_grazers(
    mut contexts: bevy_egui::EguiContexts,
    gallery: Query<(), With<crate::sim::scenarios::gallery::GalleryAnchor>>,
    camera: super::hud::CameraView,
    grazers: Query<&GlobalTransform, With<Grazer>>,
) -> Result {
    if gallery.is_empty() || grazers.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let (camera, eye) = *camera;
    let view = super::hud::Projector::new(camera, eye, ctx);
    let painter = ctx.layer_painter(bevy_egui::egui::LayerId::new(bevy_egui::egui::Order::Background, bevy_egui::egui::Id::new("grazers")));
    for at in &grazers {
        let below = at.translation() - Vec3::Y * 2.3;
        let distance = view.eye.translation.distance(below);
        if Vec3::from(view.eye.forward()).angle_between(below - view.eye.translation) > super::gallery::FACING || distance > 30.0 {
            continue;
        }
        let Some(p) = view.to_screen(below) else { continue };
        painter.text(
            p,
            bevy_egui::egui::Align2::CENTER_TOP,
            "Grazer",
            bevy_egui::egui::FontId::proportional(17.0),
            bevy_egui::egui::Color32::from_rgb(255, 214, 150),
        );
    }
    Ok(())
}
