//! Loaded models: where they come from, and the gallery for reviewing them.
//!
//! The gallery scenario's anchor names a place; each client sets out every
//! model it finds under `assets/` in a row there, turning slowly, labelled
//! with its path and size, with its sockets drawn as small axes. Collider
//! proxies (`COL_*` nodes) are hidden here as everywhere.

use std::path::{Path, PathBuf};

use bevy::camera::primitives::Aabb;
use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use bevy_egui::{EguiContexts, egui};

use super::camera::PlayerCamera;
use crate::sim::mission::{GALLERY_AT, GalleryAnchor};

/// Where assets are read from: the repo's `assets/` when run from a build
/// of it, otherwise `assets/` beside wherever the game is started.
pub fn asset_root() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
    if repo.is_dir() { repo } else { PathBuf::from("assets") }
}

/// Every `.glb` under `root`, as paths relative to it, sorted.
pub fn find_models(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Renders made by the asset builds.
                if path.file_name().is_some_and(|n| n == "preview") {
                    continue;
                }
                walk(&path, root, out);
            } else if path.extension().is_some_and(|e| e == "glb")
                && let Ok(relative) = path.strip_prefix(root)
            {
                out.push(relative.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

/// Hides collider proxies wherever a model is loaded.
pub fn hide_colliders(mut nodes: Query<(&Name, &mut Visibility), Added<Name>>) {
    for (name, mut visibility) in &mut nodes {
        if name.as_str().starts_with("COL_") {
            *visibility = Visibility::Hidden;
        }
    }
}

/// A model on show.
#[derive(Component)]
pub struct Exhibit {
    label: String,
}

/// Space between exhibits (m).
const SPACING: f32 = 4.0;
/// Labels show within this range (m), sizes within the nearer one.
const LABEL_RANGE: f32 = 25.0;
const SIZE_RANGE: f32 = 12.0;
/// How fast exhibits turn (rad/s).
const TURN_RATE: f32 = 0.3;

pub fn set_out_gallery(add: On<Add, GalleryAnchor>, assets: Res<AssetServer>, mut commands: Commands) {
    let models = find_models(&asset_root());
    let first = -(models.len().saturating_sub(1) as f32) * SPACING / 2.0;
    let mut anchor = commands.entity(add.entity);
    // The authority's anchor is placed already; a client's arrives bare.
    anchor.insert_if_new(Transform::from_translation(GALLERY_AT));
    anchor.insert_if_new(Visibility::default());
    anchor.with_children(|row| {
        for (i, model) in models.iter().enumerate() {
            row.spawn((
                Exhibit { label: model.trim_end_matches(".glb").to_string() },
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(model.clone()))),
                Transform::from_xyz(first + i as f32 * SPACING, 0.0, 0.0),
            ));
        }
    });
    info!("gallery: {} models", models.len());
}

pub fn turn_exhibits(time: Res<Time>, mut exhibits: Query<&mut Transform, With<Exhibit>>) {
    for mut transform in &mut exhibits {
        transform.rotate_y(TURN_RATE * time.delta_secs());
    }
}

/// Sockets as small axes: red along the socket's +X, green +Y, blue its
/// forward (−Z).
pub fn draw_sockets(mut gizmos: Gizmos, sockets: Query<(&Name, &GlobalTransform)>, exhibits: Query<(), With<Exhibit>>) {
    if exhibits.is_empty() {
        return;
    }
    for (name, transform) in &sockets {
        if !name.as_str().starts_with("SOCKET_") {
            continue;
        }
        let (_, rotation, at) = transform.to_scale_rotation_translation();
        let size = 0.12;
        gizmos.line(at, at + rotation * Vec3::X * size, Color::srgb(1.0, 0.2, 0.2));
        gizmos.line(at, at + rotation * Vec3::Y * size, Color::srgb(0.2, 1.0, 0.2));
        gizmos.line(at, at + rotation * Vec3::NEG_Z * size * 1.5, Color::srgb(0.3, 0.5, 1.0));
    }
}

/// Each exhibit's name and size, floating above it.
pub fn label_exhibits(
    mut contexts: EguiContexts,
    camera: Single<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    exhibits: Query<(Entity, &Exhibit, &GlobalTransform)>,
    children: Query<&Children>,
    meshes: Query<(&GlobalTransform, &Aabb)>,
) -> Result {
    if exhibits.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let (camera, view) = *camera;
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("gallery")));
    for (entity, exhibit, at) in &exhibits {
        // Bounds of everything drawn under it, in its own frame (so they
        // don't change as it turns).
        let to_local = at.affine().inverse();
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for part in children.iter_descendants(entity) {
            if let Ok((transform, aabb)) = meshes.get(part) {
                for corner in 0..8 {
                    let sign = Vec3::new(
                        if corner & 1 == 0 { -1.0 } else { 1.0 },
                        if corner & 2 == 0 { -1.0 } else { 1.0 },
                        if corner & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    let world = transform.transform_point(Vec3::from(aabb.center) + Vec3::from(aabb.half_extents) * sign);
                    let p = to_local.transform_point3(world);
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
            }
        }
        let (top, size) = if lo.x <= hi.x {
            (at.translation() + Vec3::Y * (hi.y + 0.25), hi - lo)
        } else {
            (at.translation() + Vec3::Y, Vec3::ZERO)
        };
        let Ok(p) = camera.world_to_viewport(view, top) else { continue };
        let distance = view.translation().distance(top);
        if (view.translation() - top).dot(view.forward().into()) > 0.0 || distance > LABEL_RANGE {
            continue;
        }
        painter.text(
            egui::pos2(p.x, p.y),
            egui::Align2::CENTER_BOTTOM,
            if distance < SIZE_RANGE {
                format!("{}\n{:.2} wide × {:.2} tall × {:.2} long (m)", exhibit.label, size.x, size.y, size.z)
            } else {
                exhibit.label.clone()
            },
            egui::FontId::monospace(13.0),
            egui::Color32::from_rgb(220, 235, 240),
        );
    }
    Ok(())
}

/// The material name the asset pipeline gives weak-spot cores (see
/// docs/art-style.md).
const CORE_MATERIAL: &str = "emissive_core";

/// Weak-spot cores pulse, so they read as alive (and as a target).
pub fn pulse_cores(
    time: Res<Time>,
    new: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>), Added<GltfMaterialName>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    // Each core material and its full glow.
    mut cores: Local<Vec<(Handle<StandardMaterial>, LinearRgba)>>,
) {
    for (name, material) in &new {
        if name.0 == CORE_MATERIAL && !cores.iter().any(|(h, _)| h.id() == material.0.id()) {
            if let Some(m) = materials.get(&material.0) {
                cores.push((material.0.clone(), m.emissive));
            }
        }
    }
    let pulse = 0.6 + 0.4 * (time.elapsed_secs() * 4.0).sin().abs();
    for (handle, full) in cores.iter() {
        if let Some(mut m) = materials.get_mut(handle) {
            m.emissive = *full * pulse;
        }
    }
}
