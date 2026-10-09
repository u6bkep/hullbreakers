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

use super::camera::PlayerCamera;
use crate::sim::SimMaterial;
use crate::sim::asteroids::FIELD_DIRECTION;
use crate::sim::frame::BodyFrame;
use crate::sim::player::{Mode, Player};
use crate::sim::ship::HullPart;
use crate::sim::tuning::Tuning;

#[derive(Resource)]
pub struct Materials {
    hull: Handle<StandardMaterial>,
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

/// Interior lights ride along with the ship.
pub fn setup_ship_lights(
    mut commands: Commands,
    ship: Single<Entity, With<crate::sim::ship::Ship>>,
) {
    for z in [-10.0, -4.0, 5.0, 11.0] {
        let light = commands
            .spawn((
                PointLight {
                    intensity: 60_000.0,
                    range: 12.0,
                    color: Color::srgb(1.0, 0.92, 0.8),
                    ..default()
                },
                Transform::from_xyz(0.0, 2.7, z),
            ))
            .id();
        commands.entity(*ship).add_child(light);
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
    tuning: Res<Tuning>,
    player: Single<&Player>,
    transforms: Query<&Transform, Without<PlayerCamera>>,
    camera: Single<&Transform, With<PlayerCamera>>,
) {
    if let Some(grapple) = player.grapple {
        if let Ok(body) = transforms.get(grapple.body) {
            let frame = BodyFrame {
                position: body.translation,
                rotation: body.rotation,
                ..BodyFrame::IDENTITY
            };
            let anchor = frame.point_to_world(grapple.anchor);
            let hand = camera.translation + camera.rotation * Vec3::new(0.25, -0.25, -0.4);
            let taut = grapple.tension > 1.0;
            let color = if taut {
                Color::srgb(1.0, 0.85, 0.3)
            } else {
                Color::srgb(0.6, 0.55, 0.4)
            };
            gizmos.line(hand, anchor, color);
            gizmos.sphere(Isometry3d::from_translation(anchor), 0.08, color);
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
