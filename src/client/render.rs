//! Greybox visuals: meshes derived from colliders, lighting, starfield, and
//! space dust for a sense of motion.

use avian3d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::image::Image;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::camera::{PlayerCamera, render_frame};
use super::{LocalInput, LocalPlayer};
use crate::sim::SimMaterial;
use crate::sim::asteroids::FIELD_DIRECTION;
use crate::sim::frame::BodyFrame;
use crate::sim::player::{Grapple, Mode, Player};
use crate::sim::ship::{Control, ControlAction, HullPart};
use crate::sim::tuning::Tuning;

#[derive(Resource)]
pub struct Materials {
    hull: Handle<StandardMaterial>,
    hostile_hull: Handle<StandardMaterial>,
    trim: Handle<StandardMaterial>,
    rock: Handle<StandardMaterial>,
}

pub fn setup_materials(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.insert_resource(Materials {
        hull: materials.add(StandardMaterial {
            base_color: Color::srgb(0.55, 0.57, 0.6),
            metallic: 0.6,
            perceptual_roughness: 0.5,
            ..default()
        }),
        hostile_hull: materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.1, 0.1),
            metallic: 0.7,
            perceptual_roughness: 0.4,
            ..default()
        }),
        trim: materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.45, 0.12),
            perceptual_roughness: 0.6,
            ..default()
        }),
        rock: materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.28, 0.25),
            perceptual_roughness: 0.95,
            ..default()
        }),
    });
}

/// Builds a flat-shaded mesh matching a collider exactly, so what you see is
/// what your boots stand on.
pub fn collider_mesh(collider: &Collider) -> Option<Mesh> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    append_shape(collider.shape(), Vec3::ZERO, Quat::IDENTITY, &mut positions);
    if positions.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.compute_flat_normals();
    Some(mesh)
}

/// Appends un-indexed triangles for a parry shape.
fn append_shape(
    shape: &avian3d::parry::shape::SharedShape,
    offset: Vec3,
    rotation: Quat,
    out: &mut Vec<[f32; 3]>,
) {
    let (vertices, indices) = if let Some(cuboid) = shape.as_cuboid() {
        cuboid.to_trimesh()
    } else if let Some(hull) = shape.as_convex_polyhedron() {
        hull.to_trimesh()
    } else if let Some(compound) = shape.as_compound() {
        for (pose, part) in compound.shapes() {
            let part_offset = offset + rotation * Vec3::from_array(pose.translation.to_array());
            let part_rotation = rotation * Quat::from_array(pose.rotation.to_array());
            append_shape(part, part_offset, part_rotation, out);
        }
        return;
    } else {
        warn!(
            "no mesh conversion for collider shape {:?}",
            shape.shape_type()
        );
        return;
    };
    for tri in indices {
        for i in tri {
            let v = Vec3::from_array(vertices[i as usize].to_array());
            out.push((offset + rotation * v).to_array());
        }
    }
}

/// Gives every sim entity tagged with a [`SimMaterial`] a mesh: from its
/// collider (so what you see is what your boots stand on), or from its
/// [`HullPart`] box for visual-only ship pieces.
pub fn attach_meshes(
    add: On<Add, SimMaterial>,
    query: Query<(Option<&Collider>, Option<&HullPart>, &SimMaterial)>,
    materials: Res<Materials>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let Ok((collider, part, kind)) = query.get(add.entity) else {
        return;
    };
    let mesh = match (collider, part) {
        (Some(collider), _) => collider_mesh(collider),
        (None, Some(part)) => Some(Cuboid::from_size(part.size).into()),
        (None, None) => None,
    };
    let Some(mesh) = mesh else {
        return;
    };
    let material = match kind {
        SimMaterial::Hull => materials.hull.clone(),
        SimMaterial::HostileHull => materials.hostile_hull.clone(),
        SimMaterial::Trim => materials.trim.clone(),
        SimMaterial::Rock => materials.rock.clone(),
    };
    commands
        .entity(add.entity)
        .insert((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
}

pub fn setup_lighting(mut commands: Commands) {
    commands.insert_resource(ClearColor(Color::BLACK));
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.6, 0.7, 1.0),
        brightness: 60.0,
        ..default()
    });
    commands.spawn((
        Name::new("Sun"),
        DirectionalLight {
            illuminance: 12_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.4, -0.5, 0.3), Vec3::Y),
    ));
}

/// An interior light of a ship, and how it shines when nothing is wrong
/// (combat alerts recolor it; see `combat_fx`).
#[derive(Component, Clone, Copy)]
pub struct ShipLight {
    pub color: Color,
    pub intensity: f32,
}

/// Interior lights ride along with the ship.
pub fn add_ship_lights(add: On<Add, crate::sim::ship::ShipHull>, mut commands: Commands) {
    let ship = add.entity;
    let normal = ShipLight {
        color: Color::srgb(1.0, 0.92, 0.8),
        intensity: 60_000.0,
    };
    for z in [-10.0, -4.0, 5.0, 11.0] {
        let light = commands
            .spawn((
                normal,
                PointLight {
                    intensity: normal.intensity,
                    range: 12.0,
                    color: normal.color,
                    ..default()
                },
                Transform::from_xyz(0.0, 2.7, z),
            ))
            .id();
        commands.entity(ship).add_child(light);
    }
}

#[derive(Component)]
pub struct Starfield;

const STARFIELD_RADIUS: f32 = 15_000.0;

pub fn setup_starfield(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (w, h) = (4096u32, 2048u32);
    let mut data = vec![0u8; (w * h * 4) as usize];
    for px in data.chunks_exact_mut(4) {
        px[3] = 255;
    }
    let mut rng = ChaCha8Rng::seed_from_u64(7);
    for _ in 0..14_000 {
        // Uniform on the sphere, mapped to equirectangular.
        let u: f32 = rng.random();
        let lat = (2.0 * rng.random::<f32>() - 1.0).asin();
        let x = (u * w as f32) as u32 % w;
        let y = ((0.5 - lat / std::f32::consts::PI) * h as f32) as u32 % h;
        let brightness = rng.random::<f32>().powi(6);
        let tint: [f32; 3] = match rng.random_range(0..4) {
            0 => [0.8, 0.85, 1.0],
            1 => [1.0, 0.9, 0.75],
            _ => [1.0, 1.0, 1.0],
        };
        let i = ((y * w + x) * 4) as usize;
        for c in 0..3 {
            data[i + c] = data[i + c].max((40.0 + 215.0 * brightness * tint[c]) as u8);
        }
    }
    let image = Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    commands.spawn((
        Name::new("Starfield"),
        Starfield,
        Mesh3d(meshes.add(Sphere::new(STARFIELD_RADIUS).mesh().uv(64, 32))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color_texture: Some(images.add(image)),
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::default(),
        bevy::light::NotShadowCaster,
    ));
}

pub fn follow_starfield(
    camera: Single<&Transform, (With<PlayerCamera>, Without<Starfield>)>,
    mut stars: Single<&mut Transform, With<Starfield>>,
) {
    stars.translation = camera.translation;
}

/// Specks at rest in the asteroid field's frame. They stream past at your
/// speed relative to the field, which is what makes velocity legible.
#[derive(Component)]
pub struct Dust;

const DUST_COUNT: usize = 700;
const DUST_BOX: f32 = 60.0;

pub fn setup_dust(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = meshes.add(Sphere::new(0.025).mesh().ico(0).unwrap());
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.6, 0.65),
        unlit: true,
        ..default()
    });
    let mut rng = ChaCha8Rng::seed_from_u64(11);
    for _ in 0..DUST_COUNT {
        let p = Vec3::new(rng.random(), rng.random(), rng.random()) * DUST_BOX
            - Vec3::splat(DUST_BOX / 2.0);
        commands.spawn((
            Dust,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(p),
            bevy::light::NotShadowCaster,
        ));
    }
}

pub fn update_dust(
    time: Res<Time>,
    tuning: Res<Tuning>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Dust>)>,
    mut dust: Query<&mut Transform, With<Dust>>,
) {
    let drift = FIELD_DIRECTION * tuning.field_speed * time.delta_secs();
    let center = camera.translation;
    for mut t in &mut dust {
        let rel = t.translation + drift - center;
        let wrapped = (rel + Vec3::splat(DUST_BOX / 2.0)).rem_euclid(Vec3::splat(DUST_BOX))
            - Vec3::splat(DUST_BOX / 2.0);
        t.translation = center + wrapped;
    }
}

/// Draws the grapple rope and, optionally, debug vectors.
pub fn draw_gizmos(
    mut gizmos: Gizmos,
    time: Res<Time>,
    tuning: Res<Tuning>,
    local: Res<LocalInput>,
    player: Single<(Entity, &Player), With<LocalPlayer>>,
    others: Query<(Entity, &Player), Without<LocalPlayer>>,
    transforms: Query<&Transform, (Without<PlayerCamera>, Without<Avatar>)>,
    avatars: Query<(&Avatar, &Transform)>,
    camera: Single<&Transform, With<PlayerCamera>>,
    mut rope_sag: Local<f32>,
) {
    let (me, player) = *player;
    let input = &local.0;
    // Our own body's center, as drawn.
    let own_center = {
        let frame = player
            .mode
            .body()
            .and_then(|b| transforms.get(b).ok())
            .map(render_frame);
        player.mode.world_kinematics(&tuning, frame.as_ref()).0
    };
    // Where a rope's far end is drawn: on a body, at another player, or at us.
    let rope_anchor = |grapple: &Grapple| -> Option<Vec3> {
        if grapple.target == me {
            return Some(own_center);
        }
        if let Some((_, avatar)) = avatars.iter().find(|(a, _)| a.player == grapple.target) {
            return Some(avatar.translation);
        }
        let body = transforms.get(grapple.target).ok()?;
        Some(render_frame(body).point_to_world(grapple.anchor))
    };
    // Other players' ropes, from their avatars.
    for (entity, other) in &others {
        let (Some(grapple), Some((_, avatar))) =
            (other.grapple, avatars.iter().find(|(a, _)| a.player == entity))
        else {
            continue;
        };
        if let Some(anchor) = rope_anchor(&grapple) {
            let hand = avatar.translation + avatar.rotation * Vec3::new(0.25, 0.2, -0.3);
            draw_rope(
                &mut gizmos,
                &tuning,
                &grapple,
                false,
                0.0,
                hand,
                anchor,
                time.elapsed_secs(),
            );
        }
    }
    if player.grapple.is_none() {
        *rope_sag = 0.0;
    }
    if let Some(grapple) = player.grapple {
        if let Some(anchor) = rope_anchor(&grapple) {
            let hand = camera.translation + camera.rotation * Vec3::new(0.25, -0.25, -0.4);
            // Slack is measured from the body center, as the sim does; the
            // hand is only where the rope is drawn from.
            let own_frame = player
                .mode
                .body()
                .and_then(|b| transforms.get(b).ok())
                .map(|t| BodyFrame {
                    position: t.translation,
                    rotation: t.rotation,
                    ..BodyFrame::IDENTITY
                });
            let (body_center, _) = player.mode.world_kinematics(&tuning, own_frame.as_ref());
            // Sag eases in and out so a rope hovering at full length doesn't flicker.
            let slack = grapple.length - body_center.distance(anchor);
            let target = if slack > 0.15 && grapple.tension < 1.0 {
                let d = body_center.distance(anchor);
                (((d + slack).powi(2) - d * d).max(0.0).sqrt() * 0.35).min(4.0)
            } else {
                0.0
            };
            *rope_sag += (target - *rope_sag) * (1.0 - (-time.delta_secs() * 8.0).exp());
            draw_rope(
                &mut gizmos,
                &tuning,
                &grapple,
                input.reel_in,
                *rope_sag,
                hand,
                anchor,
                time.elapsed_secs(),
            );
        }
    }
    if !tuning.show_gizmos {
        return;
    }
    match player.mode {
        Mode::Booted {
            body,
            feet,
            up,
            velocity,
            ..
        } => {
            if let Ok(body) = transforms.get(body) {
                let frame = BodyFrame {
                    position: body.translation,
                    rotation: body.rotation,
                    ..BodyFrame::IDENTITY
                };
                let f = frame.point_to_world(feet);
                gizmos.arrow(f, f + frame.dir_to_world(up), Color::srgb(0.2, 1.0, 0.3));
                gizmos.arrow(
                    f,
                    f + frame.dir_to_world(velocity),
                    Color::srgb(0.3, 0.6, 1.0),
                );
            }
        }
        Mode::Eva {
            position,
            velocity,
            orientation,
            ..
        } => {
            let ahead = camera.translation + camera.rotation * Vec3::NEG_Z * 2.0;
            gizmos.arrow(ahead, ahead + velocity * 0.2, Color::srgb(0.3, 0.6, 1.0));
            let axis = orientation * Vec3::Y;
            gizmos.line(
                position - axis * 0.9,
                position + axis * 0.9,
                Color::srgb(0.2, 1.0, 0.3),
            );
        }
    }
}

/// Rope states, readable at a glance:
/// - slack: grey, sagging by how much spare rope there is;
/// - taut and holding: straight, pale gold;
/// - pulling (reeling or under load): orange to red with load, with dashes
///   crawling toward you while the winch runs.
#[allow(clippy::too_many_arguments)]
fn draw_rope(
    gizmos: &mut Gizmos,
    tuning: &Tuning,
    grapple: &Grapple,
    reeling: bool,
    sag: f32,
    hand: Vec3,
    anchor: Vec3,
    t: f32,
) {
    let span = anchor - hand;
    let distance = span.length();
    let dir = span / distance.max(1e-3);
    let load = (grapple.tension / tuning.reel_force).clamp(0.0, 1.0);
    let pulling = reeling || grapple.tension > tuning.player_mass * 2.0;

    let slack = sag > 0.05;
    let color = if slack {
        Color::srgb(0.45, 0.45, 0.45)
    } else if pulling {
        Color::srgb(1.0, 0.65 - 0.5 * load, 0.15 - 0.1 * load)
    } else {
        Color::srgb(1.0, 0.9, 0.55)
    };

    if slack {
        // Bow the spare rope out in a fixed world direction, so it doesn't
        // swing around as you look about.
        let side = dir.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
        let points = (0..=24).map(|i| {
            let k = i as f32 / 24.0;
            hand + span * k + side * sag * (std::f32::consts::PI * k).sin()
        });
        gizmos.linestrip(points, color);
    } else {
        gizmos.line(hand, anchor, color);
        if reeling {
            // Dashes running toward the hand at the reel speed.
            let spacing = 1.5;
            let offset = (t * tuning.reel_speed) % spacing;
            let mut d = distance - offset;
            while d > 0.3 {
                let a = hand + dir * d;
                gizmos.line(a, a - dir * 0.4, Color::WHITE);
                d -= spacing;
            }
        }
    }
    gizmos.sphere(Isometry3d::from_translation(anchor), 0.08, color);
}

/// The visible body of another player.
#[derive(Component)]
pub struct Avatar {
    pub player: Entity,
}

/// Keeps a capsule (with a visor showing where they look) on every other
/// player, placed in the rendered frame of the body they stand on.
pub fn update_avatars(
    tuning: Res<Tuning>,
    players: Query<(Entity, &Player), Without<LocalPlayer>>,
    mut avatars: Query<(Entity, &Avatar, &mut Transform)>,
    transforms: Query<&Transform, Without<Avatar>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let pose = |player: &Player| -> Option<Transform> {
        let half = tuning.capsule_height / 2.0;
        let (center, rotation) = match player.mode {
            Mode::Eva {
                position,
                orientation,
                ..
            } => (position, orientation),
            Mode::Booted {
                body,
                feet,
                up,
                orientation,
                ..
            } => {
                let frame = render_frame(transforms.get(body).ok()?);
                (
                    frame.point_to_world(feet + up * half),
                    frame.rotation * orientation,
                )
            }
        };
        Some(Transform::from_translation(center).with_rotation(rotation))
    };
    let mut seen = Vec::new();
    for (entity, avatar, mut transform) in &mut avatars {
        match players.get(avatar.player).ok().and_then(|(_, p)| pose(p)) {
            Some(pose) => {
                *transform = pose;
                seen.push(avatar.player);
            }
            None if players.contains(avatar.player) => seen.push(avatar.player),
            None => commands.entity(entity).despawn(),
        }
    }
    for (entity, player) in &players {
        if seen.contains(&entity) {
            continue;
        }
        let Some(pose) = pose(player) else { continue };
        let suit = materials.add(StandardMaterial {
            base_color: Color::srgb(0.85, 0.85, 0.8),
            perceptual_roughness: 0.6,
            ..default()
        });
        let visor = materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.6, 0.1),
            emissive: LinearRgba::rgb(0.6, 0.35, 0.05),
            ..default()
        });
        let length = (tuning.capsule_height - 2.0 * tuning.capsule_radius).max(0.01);
        let eye = tuning.eye_height - tuning.capsule_height / 2.0;
        commands
            .spawn((
                Name::new("Avatar"),
                Avatar { player: entity },
                Mesh3d(meshes.add(Capsule3d::new(tuning.capsule_radius, length))),
                MeshMaterial3d(suit),
                pose,
            ))
            .with_child((
                Mesh3d(meshes.add(Cuboid::new(0.4, 0.15, 0.1))),
                MeshMaterial3d(visor),
                Transform::from_xyz(0.0, eye, -tuning.capsule_radius),
            ));
    }
}

/// Gives a ship's button a glowing cap, so there is something to aim at.
/// Its box is the sim's own (what the sim tests a click against), and the
/// colour says how much to worry about pressing it.
pub fn attach_control_caps(
    add: On<Add, Control>,
    controls: Query<&Control>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let Ok(control) = controls.get(add.entity) else {
        return;
    };
    let glow = match control.action {
        ControlAction::Scenario(_) => LinearRgba::rgb(0.1, 1.2, 0.5),
        ControlAction::Lurch => LinearRgba::rgb(2.0, 0.1, 0.05),
        ControlAction::DisableShields => LinearRgba::rgb(2.0, 1.2, 0.1),
    };
    commands.entity(add.entity).insert((
        Mesh3d(meshes.add(Cuboid::from_size(control.half_size * 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: glow,
            ..default()
        })),
    ));
}
