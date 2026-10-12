//! Crewmates as everyone else sees them: the crew suit in their own colour,
//! holding what their stance holds.

use bevy::gltf::GltfMaterialName;
use bevy::prelude::*;

use super::render::Avatar;
use super::viewmodel::{Held, held};
use crate::sim::player::{DeckSlot, Player};

pub const SUIT: &str = "crew/suit/suit.glb";

/// Each crewmate's colour, by the deck slot they came aboard at: on the
/// suit's `paint_secondary` (shoulders, helmet crest, backpack).
const COLOURS: [Color; 6] = [
    Color::srgb(0.231, 0.455, 0.851), // the suit's own blue
    Color::srgb(0.878, 0.478, 0.165),
    Color::srgb(0.310, 0.749, 0.353),
    Color::srgb(0.608, 0.365, 0.898),
    Color::srgb(0.839, 0.271, 0.271),
    Color::srgb(0.878, 0.769, 0.227),
];

/// A crewmate's suit, once its model is in: where its hands hold things
/// and what they're holding.
#[derive(Component, Default)]
pub struct CrewLook {
    dressed: bool,
    sockets: [Option<Entity>; 2],
    showing: [Option<Held>; 2],
}

/// A thing in a crewmate's hand.
#[derive(Component)]
pub struct HeldModel;

/// Paints a newly loaded suit in its wearer's colour, hangs the headlamp on
/// the helmet, and keeps each hand holding what the stance says.
pub fn dress_crew(
    assets: Res<AssetServer>,
    mut models: ResMut<super::guns::GunModels>,
    mut avatars: Query<(Entity, &Avatar, &mut CrewLook)>,
    players: Query<(&Player, Option<&DeckSlot>)>,
    children: Query<&Children>,
    names: Query<&Name>,
    painted: Query<(&GltfMaterialName, &MeshMaterial3d<StandardMaterial>)>,
    held_models: Query<(), With<HeldModel>>,
    lamps: Query<Entity, With<SpotLight>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    for (avatar, Avatar { player }, mut look) in &mut avatars {
        let Ok((player, slot)) = players.get(*player) else { continue };
        if !look.dressed {
            let mut sockets = [None, None];
            let mut lamp_socket = None;
            for part in children.iter_descendants(avatar) {
                match names.get(part).map(Name::as_str) {
                    Ok("SOCKET_hand_r") => sockets[0] = Some(part),
                    Ok("SOCKET_hand_l") => sockets[1] = Some(part),
                    Ok("SOCKET_headlamp") => lamp_socket = Some(part),
                    _ => {}
                }
            }
            // Not loaded yet.
            if sockets.contains(&None) {
                continue;
            }
            let colour = COLOURS[slot.map_or(0, |s| s.0 as usize) % COLOURS.len()];
            let mut own = None;
            for part in children.iter_descendants(avatar) {
                let Ok((name, material)) = painted.get(part) else { continue };
                if name.0 != "paint_secondary" {
                    continue;
                }
                let own = own.get_or_insert_with(|| {
                    let mut m = materials.get(&material.0).cloned().unwrap_or_default();
                    m.base_color = colour;
                    materials.add(m)
                });
                commands.entity(part).insert(MeshMaterial3d(own.clone()));
            }
            // The headlamp turns with the helmet.
            if let Some(socket) = lamp_socket {
                for &child in children.get(avatar).into_iter().flatten() {
                    if lamps.contains(child) {
                        commands.entity(child).insert((ChildOf(socket), Transform::default()));
                    }
                }
            }
            look.sockets = sockets;
            look.dressed = true;
        }
        let what = held(player);
        for hand in 0..2 {
            // A brace shows nothing: the fist is already on the foregrip's
            // side of the rifle.
            let item = what[hand].filter(|h| !matches!(h, Held::Brace(_)));
            if item == look.showing[hand] {
                continue;
            }
            let Some(socket) = look.sockets[hand] else { continue };
            for &child in children.get(socket).into_iter().flatten() {
                if held_models.contains(child) {
                    commands.entity(child).despawn();
                }
            }
            if let Some(item) = item {
                item.spawn(&mut commands, &mut models, &assets, (HeldModel, Transform::default(), ChildOf(socket)));
            }
            look.showing[hand] = item;
        }
    }
}
