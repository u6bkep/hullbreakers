//! Robots and crewmates move their limbs: procedural animation of their
//! rigid-part models.
//!
//! Robot and suit models are built from parts pivoted at their joints, every hinge
//! turning about its local X (see the assets' `build.py`). Here each part is
//! turned from its rest pose by what the robot is doing: a Gunner's legs
//! swing the way it walks, at a pace set by its speed, its head and gun arm
//! follow its aim, and the gun kicks when it fires; a Swarmer's thruster
//! pods tilt against its motion and it bobs as it hovers. A crewmate walks
//! the same way, its head follows its look, and an arm with something in
//! hand raises it to aim, kicking as it fires. Purely visual:
//! nothing here feeds back into the simulation.

use std::collections::HashMap;

use bevy::prelude::*;

use super::render::Avatar;
use super::viewmodel::held;
use crate::sim::bots::{BodyTuning, Condition, Robot};
use crate::sim::events::PlayerEvent;
use crate::sim::player::{Mode, Player};
use crate::sim::tuning::Tuning;

/// A model's moving parts, found once it has loaded: each part's entity and
/// rest pose.
#[derive(Component)]
pub struct Rig {
    body: Body,
    parts: HashMap<String, (Entity, Transform)>,
    /// Walk cycle phase (radians).
    phase: f32,
    /// How far through a stride's swing (0 standing, 1 full stride).
    stride: f32,
    /// Gun recoil, decaying (radians), per hand (the robots' one gun is
    /// the main hand's), and the last tick a shot was seen.
    kick: [f32; 2],
    fired: u32,
    /// Hover bob phase.
    bob: f32,
    /// A flyer's velocity last frame, and its smoothed acceleration in its
    /// own frame: what its thrusters are doing.
    last_velocity: Option<Vec3>,
    thrust: Vec3,
    /// A Grappler's winch drum, turned so far (radians).
    spin: f32,
    /// A Grappler's harpoon arm drawn back for a throw (0 to 1), eased.
    draw: f32,
}

/// What kind of body a model is.
#[derive(Clone, Copy, PartialEq)]
enum Body {
    Robot(Robot),
    Crew,
}

/// The parts each kind of body moves.
fn parts_of(body: Body) -> &'static [&'static str] {
    match body {
        Body::Robot(Robot::Gunner) => &[
            "pelvis",
            "torso",
            "head",
            "thigh_l",
            "thigh_r",
            "shin_l",
            "shin_r",
            "foot_l",
            "foot_r",
            "upper_arm_l",
            "forearm_l",
            "upper_arm_r",
            "gun_arm_r",
        ],
        Body::Robot(Robot::Swarmer) => &["body", "gun", "thruster_fl", "thruster_fr", "thruster_rl", "thruster_rr"],
        Body::Robot(Robot::Grappler) => &[
            "pelvis",
            "torso",
            "head",
            "thigh_l",
            "thigh_r",
            "shin_l",
            "shin_r",
            "foot_l",
            "foot_r",
            "upper_arm_l",
            "claw_l",
            "upper_arm_r",
            "harpoon_r",
            "winch",
        ],
        Body::Robot(Robot::Turret) => &["head", "barrel"],
        Body::Crew => &[
            "pelvis",
            "torso",
            "head",
            "thigh_l",
            "thigh_r",
            "shin_l",
            "shin_r",
            "foot_l",
            "foot_r",
            "upper_arm_l",
            "forearm_l",
            "upper_arm_r",
            "forearm_r",
        ],
    }
}

/// Finds an avatar's parts once its model is in.
pub fn rig_avatars(
    mut commands: Commands,
    avatars: Query<(Entity, &Avatar), Without<Rig>>,
    players: Query<Option<&Robot>, With<Player>>,
    children: Query<&Children>,
    names: Query<(&Name, &Transform)>,
) {
    for (avatar, Avatar { player }) in &avatars {
        let Ok(kind) = players.get(*player) else { continue };
        let body = kind.map_or(Body::Crew, |k| Body::Robot(*k));
        let wanted = parts_of(body);
        let parts: HashMap<String, (Entity, Transform)> = children
            .iter_descendants(avatar)
            .filter_map(|e| {
                let (name, transform) = names.get(e).ok()?;
                wanted.contains(&name.as_str()).then(|| (name.to_string(), (e, *transform)))
            })
            .collect();
        // Not loaded yet: try again next frame.
        if parts.len() == wanted.len() {
            commands.entity(avatar).insert(Rig {
                body,
                parts,
                phase: 0.0,
                stride: 0.0,
                kick: [0.0; 2],
                fired: 0,
                bob: 0.0,
                last_velocity: None,
                thrust: Vec3::ZERO,
                spin: 0.0,
                draw: 0.0,
            });
        }
    }
}

/// Seconds a gun kick takes to settle.
const KICK_DECAY: f32 = 12.0;
/// How far a crewmate raises an arm from its carry to aim (rad).
const AIM_RAISE: f32 = 0.25;
/// A Gunner's stride (m): one full leg cycle covers two.
const STRIDE: f32 = 0.9;

pub fn animate_avatars(
    time: Res<Time>,
    base: Res<Tuning>,
    mut rigs: Query<(&Avatar, &mut Rig)>,
    players: Query<(&Player, Option<&BodyTuning>, Option<&Condition>)>,
    mut transforms: Query<&mut Transform, Without<Avatar>>,
) {
    let dt = time.delta_secs();
    for (Avatar { player }, mut rig) in &mut rigs {
        let Ok((player, own, condition)) = players.get(*player) else { continue };
        let rig = &mut *rig;
        // New shots kick the gun that fired.
        for (at, e) in player.events.iter() {
            if let PlayerEvent::Fired { hand, .. } = e
                && at.wrapping_sub(rig.fired) as i32 > 0
            {
                rig.fired = at;
                rig.kick[hand as usize] = 0.12;
            }
        }
        for k in &mut rig.kick {
            *k *= (-KICK_DECAY * dt).exp();
        }
        // Motion in the robot's own frame (x right, z back), and its aim.
        let (local_velocity, pitch) = match player.mode {
            Mode::Booted {
                velocity,
                orientation,
                pitch,
                ..
            } => (orientation.inverse() * velocity, pitch),
            Mode::Eva {
                velocity,
                orientation,
                pitch,
                ..
            } => (orientation.inverse() * velocity, pitch),
        };
        let mut pose = |name: &str, turn: Quat, shift: Vec3| {
            if let Some((entity, rest)) = rig.parts.get(name)
                && let Ok(mut transform) = transforms.get_mut(*entity)
            {
                transform.translation = rest.translation + shift;
                transform.rotation = rest.rotation * turn;
            }
        };
        let walk = own.map_or(base.walk_speed, |t| t.0.walk_speed);
        let flat = Vec3::new(local_velocity.x, 0.0, local_velocity.z);
        let speed = flat.length();
        let booted = matches!(player.mode, Mode::Booted { .. });
        match rig.body {
            Body::Robot(Robot::Gunner) | Body::Robot(Robot::Grappler) | Body::Crew => {
                let target = if booted { (speed / walk).min(1.2) } else { 0.0 };
                rig.stride += (target - rig.stride) * (1.0 - (-8.0 * dt).exp());
                rig.phase += speed / STRIDE * std::f32::consts::PI * dt;
                rig.bob += dt;
                // Legs swing along the way it's going: fore and aft, and
                // (less, and never across each other) to the side; the knee
                // bends as the leg comes through.
                let heading = flat.try_normalize().unwrap_or(Vec3::NEG_Z);
                let swing = 0.45 * rig.stride;
                for (side, offset, inward) in [("l", 0.0, 1.0f32), ("r", std::f32::consts::PI, -1.0)] {
                    let s = (rig.phase + offset).sin();
                    let c = (rig.phase + offset).cos();
                    // Forward is −Z: a positive turn about X brings the foot
                    // forward; about Z, toward +X.
                    let fore = -heading.z * swing * s;
                    let across = (heading.x * swing * 0.35 * s) * inward;
                    let across = across.min(0.05) * inward;
                    pose(
                        &format!("thigh_{side}"),
                        Quat::from_rotation_x(fore) * Quat::from_rotation_z(across),
                        Vec3::ZERO,
                    );
                    // Knee: bends (backward) through the forward swing.
                    let knee = -0.7 * rig.stride * c.max(0.0);
                    pose(&format!("shin_{side}"), Quat::from_rotation_x(knee), Vec3::ZERO);
                    // Foot: kept roughly flat.
                    pose(&format!("foot_{side}"), Quat::from_rotation_x(-0.5 * knee), Vec3::ZERO);
                }
                // The body bobs twice a cycle as it walks, and breathes
                // standing.
                let bob = 0.03 * rig.stride * (2.0 * rig.phase).cos().abs() + 0.006 * (rig.bob * 1.7).sin();
                pose("pelvis", Quat::IDENTITY, Vec3::Y * bob);
                pose("torso", Quat::from_rotation_x(-0.06 * rig.stride), Vec3::ZERO);
                if rig.body == Body::Crew {
                    // The head follows the look; an arm with something in
                    // hand raises it to aim along the look (from the
                    // suit's low-ready carry), and kicks as it fires; an
                    // empty one hangs and swings.
                    let held = held(player);
                    pose("head", Quat::from_rotation_x(pitch), Vec3::ZERO);
                    pose("torso", Quat::from_rotation_x(-0.06 * rig.stride + pitch * 0.2), Vec3::ZERO);
                    for (i, side) in [(0usize, "r"), (1, "l")] {
                        let swing = 0.25 * rig.stride * (rig.phase + if i == 0 { 0.0 } else { std::f32::consts::PI }).sin();
                        let (upper, fore) = if held[i].is_some() {
                            (AIM_RAISE + pitch * 0.8 + rig.kick[i], 0.0)
                        } else {
                            (-0.25 + swing, -0.9)
                        };
                        pose(&format!("upper_arm_{side}"), Quat::from_rotation_x(upper), Vec3::ZERO);
                        pose(&format!("forearm_{side}"), Quat::from_rotation_x(fore), Vec3::ZERO);
                    }
                    continue;
                }
                if rig.body == Body::Robot(Robot::Grappler) {
                    // The harpoon arm follows the aim and draws back as it
                    // winds up to throw, snapping forward as it does; the
                    // winch on its back runs while the rope's in someone;
                    // the claw arm hangs ready.
                    let winding = condition.map_or(0.0, |c| c.winding_fraction());
                    let target = if winding > 0.0 { 1.0 } else { 0.0 };
                    let rate = if target > rig.draw { 4.0 } else { 18.0 };
                    rig.draw += (target - rig.draw) * (1.0 - (-rate * dt).exp());
                    let reeling = player.grapple.is_some_and(|g| g.tension > 1.0);
                    let winch_rate = if reeling { 14.0 } else if player.grapple.is_some() { 2.0 } else { 0.0 };
                    rig.spin += winch_rate * dt;
                    pose("head", Quat::from_rotation_x(pitch * 0.6), Vec3::ZERO);
                    pose("upper_arm_r", Quat::from_rotation_x(pitch * 0.5 + 0.9 * rig.draw), Vec3::ZERO);
                    pose("harpoon_r", Quat::from_rotation_x(pitch * 0.5 - 0.5 * rig.draw), Vec3::ZERO);
                    pose("winch", Quat::from_rotation_x(-rig.spin), Vec3::ZERO);
                    let arm = -0.2 * rig.stride * rig.phase.sin();
                    pose("upper_arm_l", Quat::from_rotation_x(arm + 0.15), Vec3::ZERO);
                    pose("claw_l", Quat::from_rotation_x(0.35 + 0.1 * (rig.bob * 1.3).sin()), Vec3::ZERO);
                    continue;
                }
                // Aim: the head and gun follow the view's pitch; the gun
                // kicks up as it fires; the free arm swings a little.
                let kick = rig.kick[0];
                pose("head", Quat::from_rotation_x(pitch * 0.6), Vec3::ZERO);
                pose("upper_arm_r", Quat::from_rotation_x(pitch * 0.5), Vec3::ZERO);
                pose("gun_arm_r", Quat::from_rotation_x(pitch * 0.5 + kick), Vec3::Z * kick * 0.3);
                let arm = -0.25 * rig.stride * rig.phase.sin();
                pose("upper_arm_l", Quat::from_rotation_x(arm), Vec3::ZERO);
                pose("forearm_l", Quat::from_rotation_x(0.3 + 0.1 * rig.stride), Vec3::ZERO);
            }
            Body::Robot(Robot::Swarmer) => {
                rig.bob += dt;
                // Its thrusters push it the way it's speeding up: the pods
                // lean to point their exhaust against that, rear pods more.
                if dt > 0.0 {
                    let accel = rig.last_velocity.map_or(Vec3::ZERO, |v| (local_velocity - v) / dt);
                    rig.thrust += (accel - rig.thrust) * (1.0 - (-4.0 * dt).exp());
                }
                rig.last_velocity = Some(local_velocity);
                let lean = (rig.thrust.z * 0.08).clamp(-0.7, 0.7);
                let roll = (rig.thrust.x * 0.06).clamp(-0.4, 0.4);
                for (pod, k) in [("thruster_fl", 0.8), ("thruster_fr", 0.8), ("thruster_rl", 1.0), ("thruster_rr", 1.0)] {
                    let side = if pod.ends_with('l') { 1.0 } else { -1.0 };
                    pose(pod, Quat::from_rotation_x(lean * k) * Quat::from_rotation_z(roll * side), Vec3::ZERO);
                }
                let hover = 0.02 * (rig.bob * 2.3).sin();
                pose("body", Quat::from_rotation_z(0.04 * (rig.bob * 1.1).sin()), Vec3::Y * hover);
                // The gun turret jolts as it sprays.
                pose("gun", Quat::from_rotation_x(rig.kick[0] * 0.5), Vec3::ZERO);
            }
            Body::Robot(Robot::Turret) => {
                // The body turns the whole mount; the head tips to the
                // aim's pitch, and the barrel recoils into it as it fires.
                pose("head", Quat::from_rotation_x(pitch), Vec3::ZERO);
                pose("barrel", Quat::IDENTITY, Vec3::Z * rig.kick[0] * 1.2);
            }
        }
    }
}
