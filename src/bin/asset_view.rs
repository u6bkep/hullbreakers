//! Renders glTF assets under the game's lighting, for judging models.
//!
//!   asset_view <out_dir> <model.glb>...
//!
//! Paths are relative to `assets/`. The models stand side by side along X;
//! the viewer photographs them from a few angles into `out_dir` and exits.
//! Nodes named `COL_*` (collider proxies) are hidden. Run it inside the
//! headless harness: `HL_BIN=asset_view tools/hl game <out_dir> <models>`.

use std::path::PathBuf;

use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use hullbreakers::client::{ambience, gallery, render};

/// Distance between models (m).
const SPACING: f32 = 0.6;
/// Seconds to let models load before the first shot, then between shots.
const LOAD_SECONDS: f32 = 3.0;
const SHOT_SECONDS: f32 = 0.5;

#[derive(Resource)]
struct Plan {
    out: PathBuf,
    next: usize,
    timer: Timer,
}

/// Camera directions (from the models toward the camera) and shot names.
const ANGLES: [(&str, Vec3); 5] = [
    ("side", Vec3::new(1.0, 0.05, 0.0)),
    ("front_high", Vec3::new(0.6, 0.45, -0.7)),
    ("rear_low", Vec3::new(0.5, -0.25, 0.8)),
    ("top", Vec3::new(0.05, 1.0, 0.1)),
    ("shadow_side", Vec3::new(-1.0, 0.1, 0.2)),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out, models) = args.split_first().expect("usage: asset_view <out_dir> <model.glb>...");
    std::fs::create_dir_all(out).expect("create out dir");
    let models = models.to_vec();
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Hullbreakers asset view".into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: gallery::asset_root().to_string_lossy().into(),
                    ..default()
                }),
        )
        // The game's own lights, under the reference sky (`ambience`'s
        // until a scenario sets another: here, never).
        .add_plugins(ambience::AmbiencePlugin)
        .add_systems(Startup, render::setup_lighting)
        .insert_resource(Plan {
            out: PathBuf::from(out),
            next: 0,
            timer: Timer::from_seconds(LOAD_SECONDS, TimerMode::Once),
        })
        .add_systems(Startup, move |mut commands: Commands, assets: Res<AssetServer>| {
            for (i, model) in models.iter().enumerate() {
                commands.spawn((
                    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(model.clone()))),
                    Transform::from_xyz(i as f32 * SPACING, 0.0, 0.0),
                ));
            }
            commands.spawn((Camera3d::default(), Bloom::NATURAL, Transform::default()));
        })
        .add_systems(Update, (gallery::hide_colliders, shoot))
        .run();
}

fn shoot(
    time: Res<Time>,
    mut plan: ResMut<Plan>,
    mut commands: Commands,
    mut camera: Query<&mut Transform, With<Camera3d>>,
    meshes: Query<(&GlobalTransform, &bevy::camera::primitives::Aabb, Option<&Visibility>), With<Mesh3d>>,
    mut exit: MessageWriter<AppExit>,
) {
    if !plan.timer.tick(time.delta()).just_finished() {
        return;
    }
    // Frame everything drawn: its bounding sphere fills most of the view.
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for (transform, aabb, visibility) in &meshes {
        if visibility == Some(&Visibility::Hidden) {
            continue;
        }
        for corner in [-1.0f32, 1.0] {
            for (cx, cy) in [(-1.0f32, -1.0f32), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
                let local = Vec3::from(aabb.center) + Vec3::from(aabb.half_extents) * Vec3::new(cx, cy, corner);
                let p = transform.transform_point(local);
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
    }
    let (center, radius) = if lo.x <= hi.x {
        ((lo + hi) / 2.0, (hi - lo).length() / 2.0)
    } else {
        (Vec3::ZERO, 0.3)
    };
    // The default 45° vertical field of view.
    let distance = radius / (22.5f32.to_radians().sin()) * 1.05;
    // Steps alternate: put the camera at an angle, then (a frame later, so
    // the shot sees the move) photograph it.
    let (angle, shoot) = (plan.next / 2, plan.next % 2 == 1);
    let Some((name, dir)) = ANGLES.get(angle) else {
        exit.write(AppExit::Success);
        return;
    };
    if shoot {
        let path = plan.out.join(format!("{name}.png"));
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    } else {
        let up = if dir.y > 0.9 { Vec3::NEG_Z } else { Vec3::Y };
        *camera.single_mut().unwrap() =
            Transform::from_translation(center + dir.normalize() * distance).looking_at(center, up);
    }
    plan.next += 1;
    plan.timer = Timer::from_seconds(SHOT_SECONDS, TimerMode::Once);
}
