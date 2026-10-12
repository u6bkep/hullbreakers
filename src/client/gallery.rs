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

use super::guns::{self, GunLook, GunModels};
use crate::sim::items::{Rarity, Weapon};
use crate::sim::weapons::GunKind;
use crate::sim::scenarios::gallery::{GALLERY_AT, GalleryAnchor};

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
    /// How far it reaches from its turning axis (m), once loaded.
    radius: Option<f32>,
    /// Its place in the row: alternate labels sit higher, so neighbours'
    /// don't overlap.
    index: usize,
}

/// Space between exhibits as first set out, before they're measured (m).
const SPACING: f32 = 4.0;
/// Labels show within this range (m), sizes within the nearer one.
const LABEL_RANGE: f32 = 25.0;
const SIZE_RANGE: f32 = 12.0;
/// How fast exhibits turn (rad/s).
const TURN_RATE: f32 = 0.3;

pub fn set_out_gallery(add: On<Add, GalleryAnchor>, assets: Res<AssetServer>, mut guns: ResMut<GunModels>, mut commands: Commands) {
    // Weapon parts have racks of their own (below), not places in the row.
    let models: Vec<String> = find_models(&asset_root()).into_iter().filter(|m| !m.starts_with(PARTS)).collect();
    let first = -(models.len().saturating_sub(1) as f32) * SPACING / 2.0;
    let mut anchor = commands.entity(add.entity);
    // The authority's anchor is placed already; a client's arrives bare.
    anchor.insert_if_new(Transform::from_translation(GALLERY_AT));
    anchor.insert_if_new(Visibility::default());
    anchor.with_children(|row| {
        for (i, model) in models.iter().enumerate() {
            row.spawn((
                Exhibit {
                    label: short_path(model),
                    radius: None,
                    index: i,
                },
                WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(model.clone()))),
                Transform::from_xyz(first + i as f32 * SPACING, 0.0, 0.0),
            ));
        }
    });
    info!("gallery: {} models", models.len());
    set_out_armoury(add.entity, &assets, &mut guns, &mut commands);
}

/// Where weapon parts' models are (they're shown on racks, not in the row).
const PARTS: &str = "weapons/parts/";

/// The gallery's areas, from its anchor, round where visitors start (the
/// anchor's `START`): the model row ahead of them, the robots to their left
/// (set out by the authority, see `bots::spawn_zoo`), the weapon parts to
/// their right, the rolled guns behind them. Each is seen whole from the
/// start by turning, and close up by a short flight.
pub const START: Vec3 = Vec3::new(0.0, 0.0, 7.0);
/// The parts' wall: its middle, right of the start, facing it.
const PARTS_AT: Vec3 = Vec3::new(3.2, 0.0, 7.4);
/// The rolled guns' wall: its middle, behind the start, facing it.
const ROLLED_AT: Vec3 = Vec3::new(0.0, 0.0, 11.0);

/// Something in the armoury (a part, a rolled gun, a heading) and its
/// label: under it, or (a heading) where it is, larger.
#[derive(Component)]
pub struct RackItem {
    text: String,
    heading: bool,
    /// The middle of its area (`PARTS_AT` or `ROLLED_AT`): its label shows
    /// only while you face that.
    area: Vec3,
}

impl RackItem {
    fn item(area: Vec3, text: impl Into<String>) -> RackItem {
        RackItem { text: text.into(), heading: false, area }
    }

    fn heading(area: Vec3, text: impl Into<String>) -> RackItem {
        RackItem { text: text.into(), heading: true, area }
    }
}

/// The armoury. To the right of the start, every part variant: a block per
/// crew frame, a row per slot, a column per variant, side on (muzzle to the
/// left as you face it), headed by its frame and its slot. Behind the start,
/// a dozen rolled guns, each put together from its parts as it's drawn in
/// play (see `guns`), an SMG row over a rifle row.
fn set_out_armoury(anchor: Entity, assets: &AssetServer, guns: &mut GunModels, commands: &mut Commands) {
    let put = |commands: &mut Commands, item: RackItem, at: Vec3, turn: Quat| {
        commands.spawn((item, Transform::from_translation(at).with_rotation(turn), Visibility::default(), ChildOf(anchor))).id()
    };
    // Parts, on a wall facing −X (the start): along it, +Z is to the right.
    let slots = ["Barrel", "Magazine", "Stock", "Sight", "Muzzle"];
    let block = PART_COLUMNS as f32 * PART_SPACING;
    let left = PARTS_AT.z - (2.0 * block + PART_BLOCK_GAP) / 2.0;
    for (b, frame) in GunKind::CREW.into_iter().enumerate() {
        let z0 = left + b as f32 * (block + PART_BLOCK_GAP);
        let top = PARTS_AT.y + (slots.len() as f32 - 1.0) * PART_ROW / 2.0;
        put(commands, RackItem::heading(PARTS_AT, format!("{} parts", frame.name())), Vec3::new(PARTS_AT.x, top + 0.45, z0 + block / 2.0), Quat::IDENTITY);
        for (r, slot) in slots.iter().enumerate() {
            let y = top - r as f32 * PART_ROW;
            if b == 0 {
                put(commands, RackItem::heading(PARTS_AT, slot.to_string()), Vec3::new(PARTS_AT.x, y, left - 0.55), Quat::IDENTITY);
            }
            for (c, part) in guns::VARIANTS.iter().filter(|p| p.slot() == *slot).enumerate() {
                let Some(path) = guns::part_model(frame, *part) else { continue };
                let at = Vec3::new(PARTS_AT.x, y, z0 + (c as f32 + 0.5) * PART_SPACING);
                let item = put(commands, RackItem::item(PARTS_AT, variant_word(part.name())), at, Quat::IDENTITY);
                commands.entity(item).insert(WorldAssetRoot(guns.get(assets, &path)));
            }
        }
    }
    // Rolled guns, on a wall facing −Z (the start): +X is to its left, so a
    // gun turned a quarter about +Y has its muzzle to the left.
    let side_on = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    let rarities = [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::Exotic, Rarity::Common, Rarity::Rare];
    put(commands, RackItem::heading(ROLLED_AT, "Rolled guns"), ROLLED_AT + Vec3::Y * 1.05, Quat::IDENTITY);
    for (row, frame) in GunKind::CREW.into_iter().enumerate() {
        let y = ROLLED_AT.y + 0.45 - row as f32 * 0.9;
        for (i, rarity) in rarities.into_iter().enumerate() {
            let weapon = Weapon::new(frame, 1000 + i as u32 * 7919, rarity);
            // Left to right as you face it: from +X to −X.
            let x = ROLLED_AT.x + (rarities.len() as f32 - 1.0) * ROLLED_SPACING / 2.0 - i as f32 * ROLLED_SPACING;
            // The name in two lines: maker and model, then epithet and
            // rarity, so neighbours' labels keep apart.
            let name = weapon.name();
            let (first, last) = name.rsplit_once(' ').unwrap_or((name.as_str(), ""));
            guns::spawn_gun(
                commands,
                guns,
                assets,
                GunLook::of(&weapon),
                (
                    RackItem::item(ROLLED_AT, format!("{first}\n{last} ({rarity:?})")),
                    Transform::from_xyz(x, y, ROLLED_AT.z).with_rotation(side_on),
                    Visibility::default(),
                    ChildOf(anchor),
                ),
            );
        }
    }
}

/// "Long barrel" → "long": a part's word, its slot being its row's heading.
fn variant_word(name: &str) -> String {
    name.split(' ').next().unwrap_or(name).to_lowercase()
}

/// The parts' wall: room per variant along a row, between rows, and
/// between the frames' blocks (m); the most variants a slot has.
const PART_SPACING: f32 = 0.5;
const PART_ROW: f32 = 0.5;
const PART_BLOCK_GAP: f32 = 0.8;
const PART_COLUMNS: usize = 4;
/// Room per rolled gun (m).
const ROLLED_SPACING: f32 = 1.05;

/// The armoury's labels, when you're near: an item's under it, a heading's
/// where it is.
pub fn label_armoury(
    mut contexts: EguiContexts,
    camera: crate::client::hud::CameraView,
    items: Query<(&RackItem, &GlobalTransform, &ChildOf)>,
    anchors: Query<&GlobalTransform>,
) -> Result {
    if items.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let (camera, eye) = *camera;
    let view = crate::client::hud::Projector::new(camera, eye, ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("armoury")));
    let forward = Vec3::from(view.eye.forward());
    for (item, at, anchor) in &items {
        let place = if item.heading { at.translation() } else { at.translation() - Vec3::Y * 0.16 };
        let distance = view.eye.translation.distance(place);
        // Only while you face it and its area, so an area's labels stay
        // out of the view of the next.
        let Ok(anchor) = anchors.get(anchor.parent()) else { continue };
        let area = anchor.transform_point(item.area);
        if forward.angle_between(place - view.eye.translation) > FACING
            || forward.angle_between(area - view.eye.translation) > FACING
            || distance > ARMOURY_LABELS
        {
            continue;
        }
        let Some(p) = view.to_screen(place) else { continue };
        let (size, color) = if item.heading {
            (16.0, egui::Color32::from_rgb(255, 214, 150))
        } else {
            (12.0, egui::Color32::from_rgb(200, 225, 235))
        };
        painter.text(
            p,
            if item.heading { egui::Align2::CENTER_CENTER } else { egui::Align2::CENTER_TOP },
            &item.text,
            egui::FontId::proportional(size),
            color,
        );
    }
    Ok(())
}

/// The armoury's labels show within this (m), and within this of where
/// you're looking (rad).
const ARMOURY_LABELS: f32 = 9.0;
pub const FACING: f32 = 0.9;

/// Big things turn slowly: no faster at their rim than this (m/s).
const RIM_SPEED: f32 = 1.5;

pub fn turn_exhibits(time: Res<Time>, mut exhibits: Query<(&Exhibit, &mut Transform)>) {
    for (exhibit, mut transform) in &mut exhibits {
        let rate = exhibit.radius.map_or(TURN_RATE, |r| TURN_RATE.min(RIM_SPEED / r.max(0.1)));
        transform.rotate_y(rate * time.delta_secs());
    }
}

/// Exhibits bigger than this (m from their axis) go in a row of their own,
/// behind the others.
const BIG: f32 = 5.0;

/// Once every exhibit has loaded, sets them out by size: a row of the
/// smaller ones, side by side with room to turn, and the big ones (ships)
/// behind them.
pub fn arrange_exhibits(
    mut exhibits: Query<(Entity, &mut Exhibit, &mut Transform)>,
    children: Query<&Children>,
    meshes: Query<(&GlobalTransform, &Aabb)>,
    globals: Query<&GlobalTransform>,
    mut arranged: Local<bool>,
) {
    if *arranged || exhibits.is_empty() {
        return;
    }
    for (entity, mut exhibit, _) in &mut exhibits {
        if exhibit.radius.is_some() {
            continue;
        }
        let Ok(at) = globals.get(entity) else { continue };
        let to_local = at.affine().inverse();
        let mut reach = 0.0f32;
        let mut any = false;
        for part in children.iter_descendants(entity) {
            let Ok((transform, aabb)) = meshes.get(part) else { continue };
            any = true;
            for corner in 0..8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                let world = transform.transform_point(Vec3::from(aabb.center) + Vec3::from(aabb.half_extents) * sign);
                let p = to_local.transform_point3(world);
                reach = reach.max(Vec2::new(p.x, p.z).length());
            }
        }
        if any {
            exhibit.radius = Some(reach);
        }
    }
    if exhibits.iter().any(|(_, e, _)| e.radius.is_none()) {
        return;
    }
    let mut small: Vec<_> = exhibits.iter_mut().filter(|(_, e, _)| e.radius.unwrap() <= BIG).collect();
    let width: f32 = small.iter().map(|(_, e, _)| 2.0 * e.radius.unwrap() + GAP).sum();
    let mut x = -width / 2.0;
    for (_, e, t) in &mut small {
        let r = e.radius.unwrap();
        t.translation = Vec3::new(x + r + GAP / 2.0, 0.0, 0.0);
        x += 2.0 * r + GAP;
    }
    let big: Vec<_> = exhibits.iter_mut().filter(|(_, e, _)| e.radius.unwrap() > BIG).collect();
    let width: f32 = big.iter().map(|(_, e, _)| 2.0 * e.radius.unwrap() + GAP).sum();
    let mut x = -width / 2.0;
    for (_, e, mut t) in big {
        let r = e.radius.unwrap();
        t.translation = Vec3::new(x + r + GAP / 2.0, 0.0, -(r + 6.0));
        x += 2.0 * r + GAP;
    }
    *arranged = true;
}

/// Room between exhibits (m).
const GAP: f32 = 1.5;

/// Sockets as small axes: red along the socket's +X, green +Y, blue its
/// forward (−Z): only the row's exhibits' (the robots and the armoury would
/// be a thicket of them).
pub fn draw_sockets(
    mut gizmos: Gizmos,
    sockets: Query<(&Name, &GlobalTransform)>,
    exhibits: Query<Entity, With<Exhibit>>,
    children: Query<&Children>,
) {
    for (name, transform) in exhibits.iter().flat_map(|e| children.iter_descendants(e)).filter_map(|e| sockets.get(e).ok()) {
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

/// Each exhibit's name floating above it (alternate ones higher, so
/// neighbours' keep apart), and, for the one nearest the crosshair, its size.
pub fn label_exhibits(
    mut contexts: EguiContexts,
    camera: crate::client::hud::CameraView,
    exhibits: Query<(Entity, &Exhibit, &GlobalTransform)>,
    children: Query<&Children>,
    meshes: Query<(&GlobalTransform, &Aabb)>,
) -> Result {
    if exhibits.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let (camera, eye) = *camera;
    let view = crate::client::hud::Projector::new(camera, eye, ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("gallery")));
    let forward: Vec3 = view.eye.forward().into();
    // Only while you face the row (its middle), so its ends' labels stay
    // out of the gallery's other areas, seen to either side of it.
    let middle = exhibits.iter().map(|(_, _, at)| at.translation()).sum::<Vec3>() / exhibits.iter().len() as f32;
    if forward.angle_between(middle - view.eye.translation) > FACING {
        return Ok(());
    }
    // The one looked at: the least angle off the crosshair, within a few
    // degrees.
    let looked_at = exhibits
        .iter()
        .map(|(e, _, at)| (e, forward.angle_between(at.translation() + Vec3::Y * 0.5 - view.eye.translation)))
        .filter(|(_, angle)| *angle < LOOK_ANGLE)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e);
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
            let stagger = if exhibit.index % 2 == 1 { 0.45 } else { 0.0 };
            (at.translation() + Vec3::Y * (hi.y + 0.25 + stagger), hi - lo)
        } else {
            (at.translation() + Vec3::Y, Vec3::ZERO)
        };
        let Some(p) = view.to_screen(top) else { continue };
        let distance = view.eye.translation.distance(top);
        // Only while you face the row, so its labels stay out of the view
        // of the gallery's other areas.
        if forward.angle_between(top - view.eye.translation) > FACING || distance > LABEL_RANGE {
            continue;
        }
        painter.text(
            p,
            egui::Align2::CENTER_BOTTOM,
            if looked_at == Some(entity) && distance < SIZE_RANGE {
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

/// An exhibit this near the crosshair (rad) is the one looked at.
const LOOK_ANGLE: f32 = 0.08;

/// "robots/gunner/gunner.glb" → "robots/gunner": a model's path, without
/// the file name that repeats its directory's.
fn short_path(model: &str) -> String {
    let path = model.trim_end_matches(".glb");
    match path.rsplit_once('/') {
        Some((dir, file)) if dir.ends_with(file) => dir.to_string(),
        _ => path.to_string(),
    }
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
