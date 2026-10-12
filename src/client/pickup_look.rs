//! How pickups look, and weapons' paint.
//!
//! A pickup is a case (or, aboard enemy ships, a bigger loot crate) with a
//! glowing band in the colour of the best thing in it: its rarity for a
//! gun, a supply green for ammo, cyan for cargo. Its best gun turns slowly
//! on the lid, in its own paint. Each is drawn where its description puts
//! it, with the body it lies on.
//!
//! Paint: a model under a [`WeaponPaint`] has its `paint_primary` and
//! `paint_secondary` materials recoloured, once they've loaded (the held
//! gun in first person, crewmates' guns, and the guns on pickups).
//! A pickup's gun is put together from its parts (see `guns`).

use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;

use super::guns::{GunLook, GunModels, spawn_gun};
use crate::sim::items::{Item, Paint, Rarity, Weapon};
use crate::sim::pickups::{CRATE_HALF, PICKUP_HALF, Pickup, PickupKind};

/// Recolours the model under it to a weapon's paint.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponPaint(pub Paint);

/// A mesh already given its paint.
#[derive(Component)]
pub struct Painted;

fn srgb([r, g, b]: [u8; 3]) -> Color {
    Color::srgb_u8(r, g, b)
}

/// Gives every loaded part of a painted model its paint: a copy of its
/// material in the paint's colour, for `paint_primary` and
/// `paint_secondary`. Runs before the view model takes its own copies of
/// materials (`viewmodel::dress_view_hands`), which keep the colour.
pub fn paint_weapons(
    roots: Query<(Entity, &WeaponPaint)>,
    children: Query<&Children>,
    meshes: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>), Without<Painted>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for (root, paint) in &roots {
        for part in children.iter_descendants(root) {
            let Ok((name, material)) = meshes.get(part) else { continue };
            let color = match name.0.as_str() {
                "paint_primary" => paint.0.body,
                "paint_secondary" => paint.0.accent,
                _ => {
                    commands.entity(part).insert(Painted);
                    continue;
                }
            };
            let Some(mut painted) = materials.get(&material.0).cloned() else { continue };
            painted.base_color = srgb(color);
            commands.entity(part).insert((Painted, MeshMaterial3d(materials.add(painted))));
        }
    }
}

#[derive(Resource)]
pub struct PickupLook {
    case: Handle<Mesh>,
    case_band: Handle<Mesh>,
    crate_box: Handle<Mesh>,
    crate_band: Handle<Mesh>,
    metal: Handle<StandardMaterial>,
    crate_metal: Handle<StandardMaterial>,
    /// Glows: by rarity (index), then supplies, then cargo.
    glows: Vec<Handle<StandardMaterial>>,
}

/// What a pickup was last drawn showing, to see when that changes (a crate
/// partly taken from).
#[derive(Component, PartialEq)]
pub struct PickupShown {
    glow: usize,
    showcase: Option<Weapon>,
}

/// A part of a pickup's look (rebuilt when what it shows changes).
#[derive(Component)]
pub struct PickupPart;

/// The gun turning on a pickup's lid.
#[derive(Component)]
pub struct Showcase;

const RARITIES: [Rarity; 5] = [Rarity::Standard, Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::Exotic];
const SUPPLY_GLOW: usize = RARITIES.len();
const CARGO_GLOW: usize = RARITIES.len() + 1;

pub fn setup_pickup_look(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let glow = |materials: &mut Assets<StandardMaterial>, [r, g, b]: [u8; 3]| {
        let color = Color::srgb_u8(r, g, b);
        materials.add(StandardMaterial {
            base_color: color,
            emissive: color.to_linear() * 3.0,
            ..default()
        })
    };
    let mut glows: Vec<_> = RARITIES.iter().map(|r| glow(&mut materials, r.color())).collect();
    // A supply colour of our own: a warm green, like the mission wall.
    glows.push(glow(&mut materials, [50, 230, 128]));
    glows.push(glow(&mut materials, [110, 200, 255]));
    let band = |half: Vec3| Cuboid::from_size(Vec3::new(half.x * 2.05, 0.05, half.z * 2.05));
    commands.insert_resource(PickupLook {
        case: meshes.add(Cuboid::from_size(PICKUP_HALF * 2.0)),
        case_band: meshes.add(band(PICKUP_HALF)),
        crate_box: meshes.add(Cuboid::from_size(CRATE_HALF * 2.0)),
        crate_band: meshes.add(band(CRATE_HALF)),
        metal: materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.27, 0.3),
            metallic: 0.8,
            perceptual_roughness: 0.4,
            ..default()
        }),
        crate_metal: materials.add(StandardMaterial {
            base_color: Color::srgb(0.17, 0.18, 0.2),
            metallic: 0.6,
            perceptual_roughness: 0.55,
            ..default()
        }),
        glows,
    });
}

/// Which glow a pickup wears: its best gun's rarity, else cargo, else
/// supplies.
fn glow_of(pickup: &Pickup) -> usize {
    match pickup.best_rarity() {
        Some(rarity) => RARITIES.iter().position(|r| *r == rarity).unwrap_or(0),
        None if pickup.items.iter().any(|i| matches!(i, Item::Cargo(_))) => CARGO_GLOW,
        None => SUPPLY_GLOW,
    }
}

#[allow(clippy::type_complexity)]
pub fn draw_pickups(
    mut commands: Commands,
    look: Res<PickupLook>,
    assets: Res<AssetServer>,
    mut models: ResMut<GunModels>,
    time: Res<Time>,
    tick: Res<crate::sim::SimTick>,
    bodies: Query<&Transform, (Without<Pickup>, Without<Showcase>)>,
    mut pickups: Query<
        (Entity, &Pickup, Option<&mut Transform>, Option<&mut Visibility>, Option<&PickupShown>, Option<&Children>),
        Without<Showcase>,
    >,
    parts: Query<(), With<PickupPart>>,
    mut showcases: Query<&mut Transform, With<Showcase>>,
) {
    let spin = Quat::from_rotation_y(time.elapsed_secs() * 0.8);
    for mut t in &mut showcases {
        t.rotation = spin;
    }
    for (entity, pickup, transform, visibility, shown, children) in &mut pickups {
        let frame = pickup.body.and_then(|b| bodies.get(b).ok()).map(|t| (t.translation, t.rotation));
        // Its body isn't here (yet, or any more): not drawn, not offered.
        let Some((at, turn)) = pickup.pose(frame, tick.0, 0.0) else {
            if let Some(mut visibility) = visibility {
                *visibility = Visibility::Hidden;
            }
            continue;
        };
        if let Some(mut visibility) = visibility
            && *visibility == Visibility::Hidden
        {
            *visibility = Visibility::Inherited;
        }
        let pose = Transform::from_translation(at).with_rotation(turn);
        let crate_kind = pickup.kind == PickupKind::Crate;
        match transform {
            Some(mut t) => *t = pose,
            None => {
                let (mesh, metal) = if crate_kind {
                    (look.crate_box.clone(), look.crate_metal.clone())
                } else {
                    (look.case.clone(), look.metal.clone())
                };
                commands.entity(entity).insert((pose, Visibility::default(), Mesh3d(mesh), MeshMaterial3d(metal)));
            }
        }
        let wanted = PickupShown {
            glow: glow_of(pickup),
            showcase: pickup.showcase(),
        };
        if shown == Some(&wanted) {
            continue;
        }
        for &child in children.into_iter().flatten() {
            if parts.contains(child) {
                commands.entity(child).despawn();
            }
        }
        let half = pickup.half();
        let band = if crate_kind { look.crate_band.clone() } else { look.case_band.clone() };
        commands.spawn((
            PickupPart,
            Mesh3d(band),
            MeshMaterial3d(look.glows[wanted.glow].clone()),
            Transform::from_xyz(0.0, if crate_kind { half.y * 0.4 } else { 0.0 }, 0.0),
            ChildOf(entity),
        ));
        if let Some(weapon) = wanted.showcase {
            // Upright on the lid, turning: grip at the bottom, the gun's
            // middle over the lid's.
            let showcase = commands
                .spawn((
                    PickupPart,
                    Showcase,
                    Transform::from_xyz(0.0, half.y + 0.04, 0.0).with_rotation(spin),
                    Visibility::default(),
                    ChildOf(entity),
                ))
                .id();
            let length = match weapon.frame {
                crate::sim::weapons::GunKind::Smg => 0.4,
                crate::sim::weapons::GunKind::Sniper => 1.1,
                _ => 0.8,
            };
            spawn_gun(
                &mut commands,
                &mut models,
                &assets,
                GunLook::of(&weapon),
                (Transform::from_xyz(0.0, 0.0, length * 0.35), ChildOf(showcase)),
            );
        }
        commands.entity(entity).insert(wanted);
    }
}
