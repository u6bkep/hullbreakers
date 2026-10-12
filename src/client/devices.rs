//! Devices as they look (see `sim::devices`): greybox fittings whose lights
//! say their state. Every device is built from its ship's design wherever
//! the ship appears, and lit from the ship's replicated [`Devices`] state.
//!
//! The lights speak one language: **cyan** is power (a live circuit, a lit
//! conduit, a junction's live way, an open door), **amber** is waiting (a
//! powered switch not yet thrown, a turned key counting down, a gate), and
//! dark is dead. A shut door shows a dim red strip. (Magenta is for weak
//! spots only.)

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::light::{EnvironmentMapLight, GeneratedEnvironmentMapLight, LightProbe};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::LocalPlayer;
use super::combat_fx::{glyph, wrap};
use crate::sim::SimTick;
use crate::sim::devices::{Device, DeviceKind, DeviceState, Devices, face_basis};
use crate::sim::mission::{BodyId, Mission};
use crate::sim::player::DeckSlot;
use crate::sim::ship::ShipHull;
use crate::sim::TICK_HZ;

const POWER: Color = Color::srgb(0.25, 0.85, 1.0);
const WAITING: Color = Color::srgb(1.0, 0.58, 0.12);
const SHUT: Color = Color::srgb(0.9, 0.14, 0.06);

/// The shared materials devices are drawn with.
#[derive(Resource)]
pub struct DeviceLook {
    metal: Handle<StandardMaterial>,
    panel: Handle<StandardMaterial>,
    door: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    power: Handle<StandardMaterial>,
    standby: Handle<StandardMaterial>,
    waiting: Handle<StandardMaterial>,
    shut: Handle<StandardMaterial>,
    gate: Handle<StandardMaterial>,
    gate_next: Handle<StandardMaterial>,
    selected: Handle<StandardMaterial>,
}

pub fn setup_device_look(mut commands: Commands, mut materials: ResMut<Assets<StandardMaterial>>) {
    let glow = |materials: &mut Assets<StandardMaterial>, color: Color, k: f32| {
        materials.add(StandardMaterial {
            base_color: color,
            emissive: LinearRgba::from(color) * k,
            perceptual_roughness: 0.5,
            ..default()
        })
    };
    let flat = |materials: &mut Assets<StandardMaterial>, color: Color, metallic: f32| {
        materials.add(StandardMaterial {
            base_color: color,
            metallic,
            perceptual_roughness: 0.55,
            ..default()
        })
    };
    commands.insert_resource(DeviceLook {
        metal: flat(&mut materials, Color::srgb(0.2, 0.21, 0.23), 0.7),
        panel: flat(&mut materials, Color::srgb(0.07, 0.075, 0.085), 0.3),
        door: flat(&mut materials, Color::srgb(0.32, 0.33, 0.36), 0.6),
        dark: flat(&mut materials, Color::srgb(0.04, 0.04, 0.045), 0.2),
        power: glow(&mut materials, POWER, 5.0),
        standby: glow(&mut materials, WAITING, 0.6),
        waiting: glow(&mut materials, WAITING, 5.0),
        shut: glow(&mut materials, SHUT, 0.9),
        gate: glow(&mut materials, WAITING, 2.0),
        gate_next: glow(&mut materials, POWER, 9.0),
        selected: glow(&mut materials, Color::srgb(0.85, 0.85, 0.8), 0.35),
    });
}

/// A part of a device's look, and what decides it.
#[derive(Component, Clone, Debug)]
pub enum Part {
    /// Cyan when live, amber when powered but off, dark without power.
    Status,
    /// Cyan when live, dark otherwise.
    Live,
    /// A lever's or pull's handle: up when thrown, down when not.
    Handle,
    /// A key's bar, turned while it's turned; and its countdown strip.
    KeyBar,
    KeyTime { width: f32 },
    /// One way of a junction: lit when it's the live way.
    Way(u8),
    /// A junction's pointer, at its way.
    Pointer { ways: u8 },
    /// A door's panel: in the doorway shut, slid along `slide` open.
    DoorPanel { slide: Vec3 },
    /// A door's strip: dim red shut, cyan open.
    DoorStrip,
    /// A board's text (lit while the board has power), and a row's lamp.
    BoardFace { lit: Handle<StandardMaterial>, unlit: Handle<StandardMaterial> },
    Row(u8),
    /// A room light: on when live (an alarm flashes).
    Light { alarm: bool },
    /// A gate's strip: bright when it's the local player's next.
    GateStrip,
}

/// Which device of which ship a part belongs to.
#[derive(Component, Clone, Copy, Debug)]
pub struct OfDevice {
    ship: Entity,
    index: u16,
}

/// How a door's panel is drawn, eased toward its state.
#[derive(Component, Default)]
pub struct DoorEase(f32);

/// Builds every device of a new ship's design.
pub fn build_devices(
    add: On<Add, ShipHull>,
    hulls: Query<&ShipHull>,
    look: Res<DeviceLook>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
) {
    let ship = add.entity;
    let Ok(hull) = hulls.get(ship) else { return };
    let design = hull.design();
    for (index, device) in design.devices.iter().enumerate() {
        let of = OfDevice { ship, index: index as u16 };
        let (r, u, n) = face_basis(device.face);
        let facing = Quat::from_mat3(&Mat3::from_cols(r, u, n));
        let mut parts: Vec<(Part, Handle<Mesh>, Handle<StandardMaterial>, Transform)> = Vec::new();
        // In the device's frame: x across its face, y up it, z out of it.
        let at = |x: f32, y: f32, z: f32| Transform::from_xyz(x, y, z);
        let root = match &device.kind {
            DeviceKind::Lever | DeviceKind::Pull => {
                parts.push((Part::Status, cuboid(&mut meshes, Vec3::new(0.08, 0.08, 0.05)), look.dark.clone(), at(0.0, 0.24, 0.03)));
                parts.push((Part::Handle, cuboid(&mut meshes, Vec3::new(0.07, 0.32, 0.07)), look.metal.clone(), at(0.0, 0.0, 0.08)));
                // The plate it's mounted on.
                let plate = (cuboid(&mut meshes, Vec3::new(0.32, 0.6, 0.06)), look.panel.clone());
                Some((facing, vec![plate]))
            }
            DeviceKind::Key { .. } => {
                parts.push((Part::Status, cuboid(&mut meshes, Vec3::new(0.06, 0.06, 0.04)), look.dark.clone(), at(0.11, 0.11, 0.03)));
                parts.push((Part::KeyBar, cuboid(&mut meshes, Vec3::new(0.16, 0.035, 0.05)), look.metal.clone(), at(0.0, 0.0, 0.09)));
                parts.push((Part::KeyTime { width: 0.26 }, cuboid(&mut meshes, Vec3::new(0.26, 0.03, 0.03)), look.waiting.clone(), at(0.0, -0.12, 0.03)));
                let plate = (cuboid(&mut meshes, Vec3::new(0.32, 0.32, 0.05)), look.panel.clone());
                let socket = (meshes.add(Cylinder::new(0.06, 0.08)), look.metal.clone());
                Some((facing, vec![plate, socket]))
            }
            DeviceKind::Junction { ways } => {
                let ways = *ways;
                for k in 0..ways {
                    let x = way_x(k, ways);
                    parts.push((Part::Way(k), cuboid(&mut meshes, Vec3::new(0.09, 0.09, 0.04)), look.dark.clone(), at(x, 0.16, 0.08)));
                }
                parts.push((Part::Status, cuboid(&mut meshes, Vec3::new(0.05, 0.05, 0.04)), look.dark.clone(), at(-0.29, -0.2, 0.08)));
                parts.push((Part::Pointer { ways }, cuboid(&mut meshes, Vec3::new(0.035, 0.2, 0.05)), look.metal.clone(), at(0.0, -0.04, 0.1)));
                let label = images.add(text_image(&[device.label.to_uppercase()], Vec2::new(0.6, 0.1), [200, 205, 210, 255], [18, 20, 23, 255]));
                let face = materials.add(StandardMaterial {
                    base_color_texture: Some(label.clone()),
                    emissive_texture: Some(label),
                    emissive: LinearRgba::WHITE * 0.3,
                    perceptual_roughness: 0.6,
                    ..default()
                });
                parts.push((Part::Live, meshes.add(Rectangle::new(0.6, 0.1)), face, at(0.0, -0.21, 0.065)));
                let housing = (cuboid(&mut meshes, Vec3::new(0.7, 0.6, 0.12)), look.panel.clone());
                Some((facing, vec![housing]))
            }
            DeviceKind::Door { size, .. } => {
                // Doors are boxes in the ship's axes; they slide along their
                // longer side into the wall.
                let slide = if size.x >= size.z { Vec3::X * size.x } else { Vec3::Z * size.z };
                parts.push((Part::DoorPanel { slide }, cuboid(&mut meshes, *size), look.door.clone(), Transform::IDENTITY));
                let strip = Vec3::new(
                    if size.x >= size.z { size.x * 0.8 } else { size.x + 0.02 },
                    0.06,
                    if size.x >= size.z { size.z + 0.02 } else { size.z * 0.8 },
                );
                parts.push((Part::DoorStrip, cuboid(&mut meshes, strip), look.shut.clone(), at(0.0, 0.25, 0.0)));
                None
            }
            DeviceKind::Lamp => {
                parts.push((Part::Live, cuboid(&mut meshes, Vec3::new(0.14, 0.14, 0.06)), look.dark.clone(), at(0.0, 0.0, 0.03)));
                Some((facing, vec![(cuboid(&mut meshes, Vec3::new(0.22, 0.22, 0.04)), look.panel.clone())]))
            }
            DeviceKind::Board { size, rows } => {
                let labels: Vec<String> = rows.iter().map(|(l, _)| l.clone()).collect();
                let text_size = Vec2::new(size.x * 0.7, size.y * 0.88);
                let image = images.add(text_image(&labels, text_size, [210, 230, 240, 255], [10, 12, 15, 255]));
                let face = |materials: &mut Assets<StandardMaterial>, glow: f32| {
                    materials.add(StandardMaterial {
                        base_color_texture: Some(image.clone()),
                        emissive_texture: Some(image.clone()),
                        emissive: LinearRgba::WHITE * glow,
                        perceptual_roughness: 0.6,
                        ..default()
                    })
                };
                let (lit, unlit) = (face(&mut materials, 0.6), face(&mut materials, 0.0));
                parts.push((
                    Part::BoardFace { lit: lit.clone(), unlit: unlit.clone() },
                    meshes.add(Rectangle::from_size(text_size)),
                    unlit,
                    at(-size.x * 0.12, 0.0, 0.035),
                ));
                // A lamp beside each row.
                let pitch = text_size.y / rows.len() as f32;
                for k in 0..rows.len() {
                    let y = text_size.y / 2.0 - pitch * (k as f32 + 0.5);
                    parts.push((Part::Row(k as u8), cuboid(&mut meshes, Vec3::splat(pitch * 0.5).with_z(0.04)), look.dark.clone(), at(size.x * 0.36, y, 0.04)));
                }
                Some((facing, vec![(cuboid(&mut meshes, Vec3::new(size.x, size.y, 0.05)), look.panel.clone())]))
            }
            DeviceKind::Lights { at: points, flash, .. } => {
                for &p in points {
                    parts.push((
                        Part::Light { alarm: *flash },
                        cuboid(&mut meshes, Vec3::new(0.35, 0.05, 0.35)),
                        look.dark.clone(),
                        Transform::from_translation(p - device.at),
                    ));
                }
                None
            }
            DeviceKind::Conduit { path } => {
                for leg in path.windows(2) {
                    let (a, b) = (leg[0] - device.at, leg[1] - device.at);
                    let d = b - a;
                    let size = d.abs() + Vec3::splat(0.05);
                    parts.push((Part::Live, cuboid(&mut meshes, size), look.dark.clone(), Transform::from_translation((a + b) / 2.0)));
                }
                None
            }
            DeviceKind::Gate { size } => {
                // Strips along the opening's edges, on both faces of the frame.
                let t = crate::sim::devices::GATE_FRAME;
                for z in [-1.0, 1.0] {
                    let z = z * (t / 2.0 + 0.01);
                    for (offset, extent) in [
                        (Vec3::Y * (size.y / 2.0 + 0.06), Vec3::new(size.x, 0.1, 0.02)),
                        (Vec3::NEG_Y * (size.y / 2.0 + 0.06), Vec3::new(size.x, 0.1, 0.02)),
                        (Vec3::X * (size.x / 2.0 + 0.06), Vec3::new(0.1, size.y, 0.02)),
                        (Vec3::NEG_X * (size.x / 2.0 + 0.06), Vec3::new(0.1, size.y, 0.02)),
                    ] {
                        parts.push((Part::GateStrip, cuboid(&mut meshes, extent), look.gate.clone(), Transform::from_translation(offset.with_z(z))));
                    }
                }
                Some((facing, Vec::new()))
            }
        };
        let rotation = root.as_ref().map_or(Quat::IDENTITY, |(q, _)| *q);
        let fixed = root.map_or(Vec::new(), |(_, f)| f);
        let lights: Vec<(Color, Vec3)> = match &device.kind {
            DeviceKind::Lights { at: points, color, .. } => points.iter().map(|&p| (Color::srgb(color.x, color.y, color.z), p - device.at)).collect(),
            _ => Vec::new(),
        };
        let entity = commands
            .spawn((
                Name::new(format!("Device {}", device.id)),
                of,
                Transform::from_translation(device.at).with_rotation(rotation),
                Visibility::default(),
            ))
            .with_children(|b| {
                for (mesh, material) in fixed {
                    b.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default()));
                }
                for (part, mesh, material, transform) in parts {
                    let door = matches!(part, Part::DoorPanel { .. });
                    let mut e = b.spawn((part, of, Mesh3d(mesh), MeshMaterial3d(material), transform));
                    if door {
                        e.insert(DoorEase(0.0));
                    }
                }
                for (color, offset) in lights {
                    b.spawn((
                        of,
                        RoomLight,
                        PointLight {
                            intensity: 0.0,
                            range: 10.0,
                            color,
                            shadow_maps_enabled: false,
                            ..default()
                        },
                        Transform::from_translation(offset - Vec3::Y * 0.1),
                    ));
                }
            })
            .id();
        commands.entity(ship).add_child(entity);
    }
}

fn cuboid(meshes: &mut Assets<Mesh>, size: Vec3) -> Handle<Mesh> {
    meshes.add(Cuboid::from_size(size))
}

/// A room light's point light (its fixture is a [`Part::Light`]).
#[derive(Component)]
pub struct RoomLight;

/// How bright a room light is (as the home ship's ceiling lights).
const ROOM_LIGHT: f32 = 50_000.0;

/// Where a junction's way `k` of `ways` sits across its face.
fn way_x(k: u8, ways: u8) -> f32 {
    let spread = 0.4;
    if ways <= 1 {
        0.0
    } else {
        -spread / 2.0 + spread * k as f32 / (ways - 1) as f32
    }
}

/// The local player's next gate on `ship`, if they have one there.
fn next_gate(mission: Option<&Mission>, local: Option<u32>, ship: Option<BodyId>) -> Option<u16> {
    mission?
        .markers
        .iter()
        .find(|m| m.player.is_some() && m.player == local && ship.is_some() && m.body == ship)
        .and_then(|m| m.device)
}

/// Lights and poses every device's parts from its ship's state.
#[allow(clippy::too_many_arguments)]
pub fn show_devices(
    time: Res<Time>,
    tick: Res<SimTick>,
    look: Res<DeviceLook>,
    ships: Query<(&ShipHull, Option<&Devices>)>,
    ids: Query<&BodyId>,
    mission: Option<Single<&Mission>>,
    local: Option<Single<&DeckSlot, With<LocalPlayer>>>,
    mut parts: Query<(&Part, &OfDevice, &mut MeshMaterial3d<StandardMaterial>, &mut Transform, Option<&mut DoorEase>)>,
    mut lights: Query<(&OfDevice, &mut PointLight), With<RoomLight>>,
) {
    let mission = mission.as_deref().copied();
    let local = local.map(|l| l.0);
    let dt = time.delta_secs();
    let flash = (time.elapsed_secs() * 2.4).fract() < 0.55;
    let state_of = |of: &OfDevice| -> Option<(&'static Device, DeviceState, &Devices)> {
        let (hull, devices) = ships.get(of.ship).ok()?;
        let devices = devices?;
        let design = hull.design();
        Some((design.devices.get(of.index as usize)?, *devices.0.get(of.index as usize)?, devices))
    };
    for (part, of, mut material, mut transform, ease) in &mut parts {
        let Some((device, s, all)) = state_of(of) else { continue };
        let set = |material: &mut MeshMaterial3d<StandardMaterial>, handle: &Handle<StandardMaterial>| {
            if material.0 != *handle {
                material.0 = handle.clone();
            }
        };
        match part {
            Part::Status => {
                let h = if s.live {
                    &look.power
                } else if s.powered {
                    &look.standby
                } else {
                    &look.dark
                };
                set(&mut material, h);
            }
            Part::Live => {
                // A junction's label is lit by its own material; the rest
                // light up cyan.
                if !matches!(device.kind, DeviceKind::Junction { .. }) {
                    set(&mut material, if s.live { &look.power } else { &look.dark });
                }
            }
            Part::Handle => {
                let angle = if s.set == 1 { 0.6 } else { -0.6 };
                transform.rotation = Quat::from_rotation_x(-angle);
                transform.translation = Vec3::new(0.0, angle.signum() * 0.06, 0.08);
            }
            Part::KeyBar => {
                transform.rotation = Quat::from_rotation_z(if s.live { std::f32::consts::FRAC_PI_2 } else { 0.0 });
            }
            &Part::KeyTime { width } => {
                let DeviceKind::Key { hold, solo } = device.kind else { continue };
                let left = s.until.saturating_sub(tick.0) as f32 / TICK_HZ as f32;
                let full = solo.unwrap_or(hold).max(hold);
                let k = if s.live { (left / full).clamp(0.0, 1.0) } else { 0.0 };
                transform.scale = Vec3::new(k.max(1e-3), 1.0, 1.0);
                transform.translation.x = -width * (1.0 - k) / 2.0;
            }
            &Part::Way(k) => {
                let h = match (s.set == k, s.live) {
                    (true, true) => &look.power,
                    (true, false) => &look.selected,
                    _ => &look.dark,
                };
                set(&mut material, h);
            }
            &Part::Pointer { ways } => {
                let x = way_x(s.set, ways);
                transform.rotation = Quat::from_rotation_z(-(x / 0.2).atan());
            }
            &Part::DoorPanel { slide } => {
                let Some(mut ease) = ease else { continue };
                let target = if s.live { 1.0 } else { 0.0 };
                ease.0 += (target - ease.0).clamp(-dt * 1.4, dt * 1.4);
                transform.translation = slide * ease.0;
            }
            Part::DoorStrip => set(&mut material, if s.live { &look.power } else { &look.shut }),
            Part::BoardFace { lit, unlit } => set(&mut material, if s.powered { lit } else { unlit }),
            &Part::Row(k) => {
                let DeviceKind::Board { rows, .. } = &device.kind else { continue };
                let on = s.powered && rows.get(k as usize).is_some_and(|(_, logic)| logic.eval(&all.0));
                set(&mut material, if on { &look.power } else { &look.dark });
            }
            &Part::Light { alarm } => {
                let on = s.live && (!alarm || flash);
                set(&mut material, if on { &look.selected } else { &look.dark });
            }
            Part::GateStrip => {
                let next = next_gate(mission, local, ids.get(of.ship).ok().copied()) == Some(of.index);
                set(&mut material, if next { &look.gate_next } else { &look.gate });
            }
        }
    }
    for (of, mut light) in &mut lights {
        let Some((device, s, _)) = state_of(of) else { continue };
        let alarm = matches!(device.kind, DeviceKind::Lights { flash: true, .. });
        let on = s.live && (!alarm || flash);
        let intensity = if on { ROOM_LIGHT } else { 0.0 };
        if light.intensity != intensity {
            light.intensity = intensity;
        }
    }
}

/// A ship whose rooms are lit by its devices (a derelict) is only as bright
/// inside as its lights: its cabins' fill light goes with how many are on.
pub fn dim_dark_cabins(
    ships: Query<(&ShipHull, Option<&Devices>, &Children)>,
    mut probes: Query<(Option<&mut EnvironmentMapLight>, Option<&mut GeneratedEnvironmentMapLight>), With<LightProbe>>,
) {
    for (hull, devices, children) in &ships {
        let design = hull.design();
        let rooms: Vec<usize> = design
            .devices
            .iter()
            .enumerate()
            .filter(|(_, d)| matches!(d.kind, DeviceKind::Lights { flash: false, .. }))
            .map(|(i, _)| i)
            .collect();
        if rooms.is_empty() {
            continue;
        }
        let lit = rooms.iter().filter(|&&i| devices.is_some_and(|d| d.0.get(i).is_some_and(|s| s.live))).count();
        let intensity = DARK_FILL + (LIT_FILL - DARK_FILL) * lit as f32 / rooms.len() as f32;
        for &child in children {
            let Ok((map, generated)) = probes.get_mut(child) else { continue };
            if let Some(mut map) = map
                && map.intensity != intensity
            {
                map.intensity = intensity;
            }
            if let Some(mut generated) = generated
                && generated.intensity != intensity
            {
                generated.intensity = intensity;
            }
        }
    }
}

/// A dark ship's fill light (cd/m²), and with every light on (as the home
/// ship's cabins).
const DARK_FILL: f32 = 6.0;
const LIT_FILL: f32 = 150.0;

/// Draws `lines` of text, left aligned, as large as fits a face of `size`
/// (m): light `fg` on `bg`.
fn text_image(lines: &[String], size: Vec2, fg: [u8; 4], bg: [u8; 4]) -> Image {
    const TEXEL: usize = 3;
    let lines: Vec<String> = if lines.len() == 1 { wrap(&lines[0], size) } else { lines.to_vec() };
    let cols = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1).max(1);
    // In font pixels: 6 per glyph, and each line gets an equal share of the
    // height (9 at least).
    let per_line = ((size.y / lines.len() as f32) / (size.x / (cols * 6 + 2) as f32)).max(9.0) as usize;
    let (fw, fh) = (cols * 6 + 2, lines.len() * per_line);
    let (w, h) = (fw * TEXEL, fh * TEXEL);
    let mut px = vec![bg; w * h];
    for (row, line) in lines.iter().enumerate() {
        let y0 = row * per_line + (per_line - 7) / 2;
        for (i, c) in line.chars().enumerate() {
            for (gy, bits) in glyph(c).iter().enumerate() {
                for gx in 0..5 {
                    if bits & (0x10 >> gx) == 0 {
                        continue;
                    }
                    let (fx, fy) = (1 + i * 6 + gx, y0 + gy);
                    for ty in 0..TEXEL {
                        for tx in 0..TEXEL {
                            px[(fy * TEXEL + ty) * w + fx * TEXEL + tx] = fg;
                        }
                    }
                }
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        px.concat(),
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}
