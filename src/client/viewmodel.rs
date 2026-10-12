//! What you hold, in first person: your arms and whatever your stance puts
//! in each hand, in front of the camera.
//!
//! Each hand is a child of the camera, posed from the local player's
//! predicted loadout: the gun or grapple its slot holds (with your glove
//! closed on its grip), or, when the other hand braces a rifle, your glove
//! on the rifle's foregrip. Shots kick the hand that fired, a reload dips
//! it, a stance change drops both hands out of view and brings them back,
//! and the hands lag a little behind the look. Purely visual.
//!
//! The hands are drawn by a camera of their own, on their own render layer
//! (see `camera::spawn_camera`): over the world rather than in it, so they
//! never clip into a wall, and lit by every light but your own headlamp.

use bevy::camera::visibility::RenderLayers;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;

use super::{LocalInput, LocalPlayer};
use super::camera::{PlayerCamera, VIEW_MODEL_LAYER};
use super::gallery::asset_root;
use crate::sim::events::PlayerEvent;
use crate::sim::player::Player;
use super::pickup_look::WeaponPaint;
use crate::sim::items::{Paint, Rarity, Weapon};
use crate::sim::weapons::{GunKind, Hand, Slot};

/// What a hand shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Held {
    /// A gun of a frame, in its paint (`None`: standard issue, as modelled).
    Gun(GunKind, Option<Paint>),
    Grapple,
    /// The glove alone, on the foregrip of the rifle the other hand holds.
    Brace,
}

impl Held {
    pub fn model(self) -> Option<&'static str> {
        match self {
            Held::Gun(frame, _) => frame_model(frame),
            Held::Grapple => Some("weapons/grapple/grapple.glb"),
            Held::Brace => None,
        }
    }
}

/// A frame's model, if it has one.
pub fn frame_model(frame: GunKind) -> Option<&'static str> {
    match frame {
        GunKind::Smg => Some("weapons/smg_frame/smg_frame.glb"),
        GunKind::Rifle => Some("weapons/rifle/rifle.glb"),
        _ => None,
    }
}

impl Held {
    /// The paint a gun wears, if not its model's own.
    pub fn paint(self) -> Option<Paint> {
        match self {
            Held::Gun(_, paint) => paint,
            _ => None,
        }
    }

    /// For a weapon in hand.
    pub fn gun(weapon: &Weapon) -> Held {
        Held::Gun(weapon.frame, (weapon.rarity != Rarity::Standard).then(|| weapon.look().paint))
    }
}

const ARMS: &str = "crew/arms_fp/arms_fp.glb";

/// One hand, under the camera.
#[derive(Component)]
pub struct ViewHand {
    hand: Hand,
    showing: Option<Held>,
    /// Recoil, decaying (0..1), and the last shot's tick.
    kick: f32,
    fired: Option<u32>,
    /// How far the reel has turned (rad).
    reel: f32,
}

/// A model shown in a hand (despawned when the hand changes what it holds).
#[derive(Component)]
pub struct HandModel;

/// The arms model in a hand, showing only that hand's arm.
#[derive(Component)]
pub struct Arm(Hand);

/// The hands' rest pose, in the camera's frame: where the grip sits.
const MAIN_GRIP: Vec3 = Vec3::new(0.23, -0.27, -0.52);
/// A braced rifle sits nearer the middle and closer, its stock toward the
/// shoulder.
const BRACED_GRIP: Vec3 = Vec3::new(0.13, -0.25, -0.42);
/// The rifle's foregrip from its grip (the model's `SOCKET_foregrip`).
const RIFLE_FOREGRIP: Vec3 = Vec3::new(0.0, 0.02, -0.298);
/// Guns point where the tracers cross the line of fire (see
/// `weapons_fx::CONVERGE`).
const CONVERGE: f32 = 40.0;
/// How long recoil takes to settle (1/s), and how far it throws the hand
/// back (m) and the muzzle up (rad).
const KICK_DECAY: f32 = 14.0;
const KICK_BACK: f32 = 0.035;
const KICK_UP: f32 = 0.06;
/// How far a stance change drops the hands (m).
const SWITCH_DROP: f32 = 0.3;
/// How far a reload dips the hand (m), and turns it (rad).
const RELOAD_DIP: f32 = 0.12;
const RELOAD_ROLL: f32 = 0.6;
/// How fast the reel turns while reeling (rad/s).
const REEL_SPIN: f32 = 18.0;

pub fn spawn_view_hands(add: On<Add, PlayerCamera>, mut commands: Commands) {
    for hand in Hand::BOTH {
        commands.spawn((
            Name::new(format!("View hand {hand:?}")),
            ViewHand {
                hand,
                showing: None,
                kick: 0.0,
                fired: None,
                reel: 0.0,
            },
            Transform::default(),
            Visibility::default(),
            ChildOf(add.entity),
        ));
    }
}

/// What each hand holds, from the stance (an empty weapon slot shows
/// nothing, and neither do hands full of cargo).
pub fn held(player: &Player) -> [Option<Held>; 2] {
    let loadout = &player.loadout;
    let stance = loadout.stance();
    let item = |hand: Hand| match stance.slot(hand) {
        _ if loadout.hands_full() => None,
        Slot::Empty => None,
        Slot::Grapple => Some(Held::Grapple),
        Slot::Gun(_) => loadout.weapon(hand).map(Held::gun),
    };
    let (main, off) = (item(Hand::Main), item(Hand::Off));
    // A rifle with the other hand free is braced on its foregrip.
    let brace = |this: Option<Held>, other: Option<Held>| match (this, other) {
        (None, Some(Held::Gun(GunKind::Rifle, _))) => Some(Held::Brace),
        _ => this,
    };
    [brace(main, off), brace(off, main)]
}

pub fn pose_view_hands(
    time: Res<Time>,
    assets: Res<AssetServer>,
    local: Res<LocalInput>,
    player: Option<Single<&Player, With<LocalPlayer>>>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<ViewHand>)>,
    mut hands: Query<(Entity, &mut ViewHand, &mut Transform, &mut Visibility, Option<&Children>), Without<PlayerCamera>>,
    models: Query<(), With<HandModel>>,
    mut last_view: Local<Option<Quat>>,
    mut sway: Local<Vec2>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    let Some(player) = player else {
        for (_, _, _, mut visibility, _) in &mut hands {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    // Sway: the hands trail the look a little, and catch up.
    let view = camera.rotation;
    if let Some(last) = *last_view {
        let turn = last.inverse() * view;
        let (yaw, pitch, _) = turn.to_euler(EulerRot::YXZ);
        *sway += Vec2::new(-yaw, -pitch) * 0.6;
    }
    *last_view = Some(view);
    *sway *= (-10.0 * dt).exp();
    *sway = sway.clamp(Vec2::splat(-0.08), Vec2::splat(0.08));

    let what = held(&player);
    let loadout = &player.loadout;
    let switching = loadout.switch_fraction();
    let braced = what.contains(&Some(Held::Brace));
    let reeling = local.0.reel_in || local.0.pay_out;
    // The main hand's pose, for a brace to follow.
    let mut main_pose = Transform::default();
    let mut order: Vec<_> = hands.iter_mut().collect();
    order.sort_by_key(|(_, h, ..)| h.hand != Hand::Main);
    for (entity, mut hand, mut transform, mut visibility, children) in order {
        let index = hand.hand as usize;
        let item = what[index];
        if item != hand.showing {
            // Swap the models.
            for &child in children.into_iter().flatten() {
                if models.contains(child) {
                    commands.entity(child).despawn();
                }
            }
            if let Some(path) = item.and_then(Held::model) {
                let mut model = commands.spawn((
                    HandModel,
                    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path))),
                    Transform::default(),
                    ChildOf(entity),
                ));
                if let Some(paint) = item.and_then(Held::paint) {
                    model.insert(WeaponPaint(paint));
                }
            }
            if item.is_some() && asset_root().join(ARMS).is_file() {
                commands.spawn((
                    HandModel,
                    Arm(hand.hand),
                    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(ARMS))),
                    Transform::default(),
                    ChildOf(entity),
                ));
            }
            hand.showing = item;
        }
        *visibility = if item.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        let Some(item) = item else { continue };

        // Recoil from this hand's newest shot.
        if let Some((at, _)) = player
            .events
            .iter()
            .filter(|(_, e)| matches!(e, PlayerEvent::Fired { hand: h, .. } if *h == hand.hand))
            .last()
            && hand.fired != Some(at)
        {
            if hand.fired.is_some() {
                hand.kick = 1.0;
            }
            hand.fired = Some(at);
        }
        hand.kick *= (-KICK_DECAY * dt).exp();
        if item == Held::Grapple && reeling {
            hand.reel += REEL_SPIN * dt;
        }

        let side = match hand.hand {
            Hand::Main => 1.0,
            Hand::Off => -1.0,
        };
        let pose = if item == Held::Brace {
            // On the foregrip of the rifle in the other hand.
            Transform::from_translation(main_pose.transform_point(RIFLE_FOREGRIP)).with_rotation(main_pose.rotation)
        } else {
            let rest = if braced { BRACED_GRIP } else { MAIN_GRIP };
            let grip = Vec3::new(rest.x * side, rest.y, rest.z);
            // Aim at where the line of fire crosses the tracers' path.
            let aim = Transform::from_translation(grip).looking_at(Vec3::NEG_Z * CONVERGE, Vec3::Y);
            // Reloading: dip and turn the gun over, then bring it back.
            let dip = loadout
                .reload_progress(hand.hand)
                .map_or(0.0, |done| (std::f32::consts::PI * done).sin());
            let drop = switching * SWITCH_DROP + dip * RELOAD_DIP;
            let kick = hand.kick;
            let offset = Vec3::new(sway.x * 0.3, sway.y * 0.3 - drop, kick * KICK_BACK);
            Transform::from_translation(aim.translation + offset).with_rotation(
                aim.rotation
                    * Quat::from_rotation_y(sway.x)
                    * Quat::from_rotation_x(sway.y + kick * KICK_UP - switching * 0.8)
                    * Quat::from_rotation_z(dip * RELOAD_ROLL * side),
            )
        };
        if hand.hand == Hand::Main {
            main_pose = pose;
        }
        *transform = pose;
    }
}

/// Model details the hands look after once loaded: everything held is on
/// the view model's layer and casts no shadow (hands with no body behind
/// them would cast a puzzling one), each arms model shows only its own
/// hand's arm, and the grapple's reel turns and its harpoon is gone while
/// it's out on the line.
pub fn dress_view_hands(
    player: Option<Single<&Player, With<LocalPlayer>>>,
    hands: Query<(Entity, &ViewHand)>,
    arms: Query<&Arm>,
    children: Query<&Children>,
    names: Query<&Name>,
    unshadowed: Query<(), (With<Mesh3d>, Without<NotShadowCaster>)>,
    layers: Query<&RenderLayers, With<Mesh3d>>,
    undimmed: Query<&MeshMaterial3d<StandardMaterial>, Without<Dimmed>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut parts: Query<(&mut Transform, &mut Visibility, Option<&RestPose>), Without<ViewHand>>,
    mut commands: Commands,
) {
    let out = player.is_some_and(|p| p.grapple.is_some());
    for (root, hand) in &hands {
        for child in children.iter_descendants(root) {
            if unshadowed.contains(child) {
                commands.entity(child).insert(NotShadowCaster);
            }
            if layers.get(child).is_ok_and(|l| *l != RenderLayers::layer(VIEW_MODEL_LAYER)) {
                commands.entity(child).insert(RenderLayers::layer(VIEW_MODEL_LAYER));
            }
            // Lights made to be seen across a room glare at arm's length.
            if let Ok(material) = undimmed.get(child)
                && let Some(mut dim) = materials.get(&material.0).cloned()
            {
                dim.emissive = dim.emissive * HELD_GLOW;
                commands.entity(child).insert((Dimmed, MeshMaterial3d(materials.add(dim))));
            }
        }
        for model in children.get(root).into_iter().flatten() {
            let arm = arms.get(*model).ok().map(|a| a.0);
            for part in children.iter_descendants(*model) {
                let Ok(name) = names.get(part) else { continue };
                let Ok((mut transform, mut visibility, rest)) = parts.get_mut(part) else {
                    continue;
                };
                match (name.as_str(), arm) {
                    ("arm_r", Some(a)) => *visibility = if a == Hand::Main { Visibility::Inherited } else { Visibility::Hidden },
                    ("arm_l", Some(a)) => *visibility = if a == Hand::Off { Visibility::Inherited } else { Visibility::Hidden },
                    ("harpoon", None) => *visibility = if out { Visibility::Hidden } else { Visibility::Inherited },
                    ("reel", None) => match rest {
                        Some(rest) => transform.rotation = rest.0.rotation * Quat::from_rotation_x(-hand.reel),
                        None => {
                            commands.entity(part).insert(RestPose(*transform));
                        }
                    },
                    _ => {}
                }
            }
        }
    }
}

/// A held thing's mesh, with its own dimmed copy of its material.
#[derive(Component)]
pub struct Dimmed;

/// How bright held things' lights are, of their full glow.
const HELD_GLOW: f32 = 0.3;

/// A part's pose as loaded, for turning it from.
#[derive(Component)]
pub struct RestPose(Transform);
