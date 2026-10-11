//! Ships: greybox hulls, the controls and turrets fitted to them, and the
//! crew ship's drift through the asteroid field.
//!
//! In the field the world frame is chosen so the crew's ship is (on average)
//! at rest and the asteroid field streams past it. Physics is
//! Galilean-invariant, so this is identical to the ship flying through a
//! static field, but keeps every coordinate near the origin and keeps
//! relative velocities near the ship small for collision sweeps. Ships in a
//! fight fly scripted paths instead (see [`super::flight`]).

use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bodies::{BodyInertia, PreviousVelocity};
use super::mission::ScenarioKind;
use super::tuning::Tuning;
use super::{SimMaterial, SimTick, TICK_HZ};

/// The crew's ship: where players come aboard, and where the mission wall is.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HomeShip;

/// A ship drifting with the field: a slow roll, a gentle yaw wobble, and an
/// optional forward lurch.
#[derive(Component, Default)]
pub struct Drift {
    /// Tick the current forward lurch started on.
    lurch_start: Option<u32>,
}

/// Orders for drifting ships, from whoever is allowed to give them. Consumed
/// by the authoritative simulation only: a ship is not something a client
/// predicts, so these never need replaying.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub enum ShipCommand {
    /// A one-second forward lurch.
    Lurch,
}

const LURCH_SECONDS: f32 = 1.0;
const WOBBLE_SECONDS: u32 = 20;

/// Ship-local forward axis.
pub const SHIP_FORWARD: Vec3 = Vec3::NEG_Z;

/// Which hull a ship has. Its geometry, controls and turrets all follow
/// from this.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum HullKind {
    /// The crew's ship.
    #[default]
    Home,
    /// The enemy in a boarding action: a big, slow, two-decked ship crewed
    /// by robots, its shield console on the bridge at the far end from its
    /// hangar door.
    Carrier,
}

impl HullKind {
    /// The ship's shield: an ellipsoid around the hull (center and radii,
    /// in ship coordinates).
    pub fn shield(self) -> (Vec3, Vec3) {
        match self {
            HullKind::Home => (Vec3::new(0.0, 2.0, 0.0), Vec3::new(9.0, 7.5, 19.0)),
            HullKind::Carrier => (Vec3::new(0.0, 3.2, 0.0), Vec3::new(11.0, 8.0, 36.0)),
        }
    }

    /// Where its turrets sit, in ship coordinates: one on top, one under
    /// the keel. A turret's beam leaves from its mount.
    pub fn turret_mounts(self) -> [Vec3; 2] {
        match self {
            HullKind::Home => [Vec3::new(0.0, 4.6, -4.0), Vec3::new(0.0, -0.8, -6.0)],
            HullKind::Carrier => [Vec3::new(0.0, 7.1, -8.0), Vec3::new(0.0, -0.8, 4.0)],
        }
    }

    /// The name on the HUD.
    pub fn name(self) -> &'static str {
        match self {
            HullKind::Home => "SHIP",
            HullKind::Carrier => "CARRIER",
        }
    }
}

/// One structural box of the hull: center, full size, and turn, in ship
/// coordinates.
struct HullBox {
    center: Vec3,
    size: Vec3,
    rotation: Quat,
    material: SimMaterial,
}

const fn hull(center: Vec3, size: Vec3) -> HullBox {
    HullBox {
        center,
        size,
        rotation: Quat::IDENTITY,
        material: SimMaterial::Hull,
    }
}

const fn trim(center: Vec3, size: Vec3) -> HullBox {
    HullBox {
        center,
        size,
        rotation: Quat::IDENTITY,
        material: SimMaterial::Trim,
    }
}

/// A box spanning `lo` to `hi`.
fn span(lo: Vec3, hi: Vec3) -> HullBox {
    hull((lo + hi) / 2.0, hi - lo)
}

/// A wall across the ship (in x, at `z`), from `x0` to `x1` and `y0` to
/// `y1`, `t` thick, with doorways: (from x, to x, top of the opening).
fn wall_across(z: f32, (x0, x1): (f32, f32), (y0, y1): (f32, f32), t: f32, doors: &[(f32, f32, f32)]) -> Vec<HullBox> {
    let mut boxes = Vec::new();
    let mut x = x0;
    let mut doors = doors.to_vec();
    doors.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (a, b, top) in doors {
        if a > x {
            boxes.push(span(Vec3::new(x, y0, z - t / 2.0), Vec3::new(a, y1, z + t / 2.0)));
        }
        if top < y1 {
            boxes.push(span(Vec3::new(a, top, z - t / 2.0), Vec3::new(b, y1, z + t / 2.0)));
        }
        x = b;
    }
    if x < x1 {
        boxes.push(span(Vec3::new(x, y0, z - t / 2.0), Vec3::new(x1, y1, z + t / 2.0)));
    }
    boxes
}

/// A wall along the ship (in z, at `x`), likewise; doorways are (from z,
/// to z, top).
fn wall_along(x: f32, (z0, z1): (f32, f32), (y0, y1): (f32, f32), t: f32, doors: &[(f32, f32, f32)]) -> Vec<HullBox> {
    // The same as a wall across, turned: swap x and z.
    wall_across(x, (z0, z1), (y0, y1), t, doors)
        .into_iter()
        .map(|mut b| {
            b.center = Vec3::new(b.center.z, b.center.y, b.center.x);
            b.size = Vec3::new(b.size.z, b.size.y, b.size.x);
            b
        })
        .collect()
}

fn hull_layout(kind: HullKind) -> Vec<HullBox> {
    let mut boxes = match kind {
        HullKind::Home => home_layout(),
        HullKind::Carrier => carrier_layout(),
    };
    // Turret bases, where the turrets are mounted.
    for mount in kind.turret_mounts() {
        boxes.push(trim(mount - Vec3::Y * mount.y.signum() * 0.25, Vec3::new(1.0, 0.5, 1.0)));
    }
    boxes
}

/// The crew's ship. Interior deck surface is y = 0, ceiling y = 3, interior
/// half-width 3.7, hull runs z = -15 (bow) to z = +15 (stern, open cargo
/// door).
fn home_layout() -> Vec<HullBox> {
    let t = 0.3;
    let v = Vec3::new;
    vec![
        // Floor, ceiling, side walls.
        hull(v(0.0, -t / 2.0, 0.0), v(8.0, t, 30.0)),
        hull(v(0.0, 3.0 + t / 2.0, 0.0), v(8.0, t, 30.0)),
        hull(v(-3.85, 1.5, 0.0), v(t, 3.0, 30.0)),
        hull(v(3.85, 1.5, 0.0), v(t, 3.0, 30.0)),
        // Bow wall with a window slot between y = 1.2 and y = 2.0.
        hull(v(0.0, 0.6, -14.85), v(7.4, 1.2, t)),
        hull(v(0.0, 2.5, -14.85), v(7.4, 1.0, t)),
        // Stern: open cargo door, 3 m wide by 2.4 m high.
        hull(v(-2.6, 1.5, 14.85), v(2.2, 3.0, t)),
        hull(v(2.6, 1.5, 14.85), v(2.2, 3.0, t)),
        hull(v(0.0, 2.7, 14.85), v(3.0, 0.6, t)),
        // Mid bulkhead with a doorway 1.2 m wide by 2.2 m high.
        hull(v(-2.15, 1.5, 0.0), v(3.1, 3.0, t)),
        hull(v(2.15, 1.5, 0.0), v(3.1, 3.0, t)),
        hull(v(0.0, 2.6, 0.0), v(1.2, 0.8, t)),
        // Cargo crates in the aft hold.
        trim(v(-2.6, 0.6, 9.0), v(1.2, 1.2, 1.2)),
        trim(v(-2.6, 0.5, 10.4), v(1.0, 1.0, 1.4)),
        trim(v(2.4, 0.9, 6.0), v(1.6, 1.8, 1.6)),
        // Bridge console.
        trim(v(0.0, 0.5, -13.0), v(4.0, 1.0, 0.8)),
        // Dorsal spine: a raised walkway along the top.
        trim(v(0.0, 3.3 + 0.4, 2.0), v(2.0, 0.8, 18.0)),
        // Antenna mast.
        trim(v(0.0, 3.3 + 2.0, -9.0), v(0.3, 4.0, 0.3)),
        // Engine nacelles on pylons.
        hull(v(-5.4, 1.5, 11.0), v(1.6, 1.6, 8.0)),
        hull(v(5.4, 1.5, 11.0), v(1.6, 1.6, 8.0)),
        trim(v(-4.3, 1.5, 11.0), v(0.6, 0.4, 3.0)),
        trim(v(4.3, 1.5, 11.0), v(0.6, 0.4, 3.0)),
        // The mission wall: a panel on the port side of the bridge.
        trim(v(-3.65, 1.5, -7.0), v(0.1, 1.4, 4.4)),
        // The weapons locker's door, on the starboard wall of the hold.
        trim(v(3.65, 1.4, 9.0), v(0.1, 2.0, 1.4)),
    ]
}

/// The carrier's two decks: the lower from y = 0 to 3, the upper from
/// y = 3.3 to 6.3. Inside it's 12 m wide (x = ±6) and 60 m long (z = −30,
/// the bow, to +30, the stern).
pub mod carrier {
    use bevy::math::Vec3;

    pub const UPPER: f32 = 3.3;
    /// The bridge's shield console, its top at the button.
    pub const CONSOLE: Vec3 = Vec3::new(0.0, UPPER + 0.5, -27.0);
    /// Where boarders come in: the middle of the hangar door's sill.
    pub const HANGAR_DOOR: Vec3 = Vec3::new(0.0, 0.0, 30.0);
}

/// The carrier, from the stern in: a double-height hangar behind a 6 m
/// door; a ramp up its port wall to the upper deck. Below, a corridor runs
/// forward between side rooms to a forward room. Above, the ramp's door
/// opens into the port room, from which a corridor runs forward to the
/// bridge, where the shield console is: the deepest point of the ship.
fn carrier_layout() -> Vec<HullBox> {
    use carrier::UPPER;
    let v = Vec3::new;
    let (t, wall) = (0.3, 0.2);
    let (w, top) = (6.0, UPPER + 3.0);
    let mut boxes = vec![
        // Floor, roof, sides.
        span(v(-w - t, -t, -30.0 - t), v(w + t, 0.0, 30.0 + t)),
        span(v(-w - t, top, -30.0 - t), v(w + t, top + t, 30.0 + t)),
        span(v(-w - t, 0.0, -30.0), v(-w, top, 30.0)),
        span(v(w, 0.0, -30.0), v(w + t, top, 30.0)),
        // The deck between, forward of the hangar.
        span(v(-w, 3.0, -30.0), v(w, UPPER, 14.0)),
    ];
    // Bow: a window slit across the bridge.
    boxes.push(span(v(-w, 0.0, -30.0 - t), v(w, UPPER + 1.2, -30.0)));
    boxes.push(span(v(-w, UPPER + 2.0, -30.0 - t), v(w, top, -30.0)));
    // Stern: the hangar door, 6 m wide and 2.8 m high.
    boxes.extend(wall_across(30.0 + t / 2.0, (-w, w), (0.0, top), t, &[(-3.0, 3.0, 2.8)]));
    // Hangar's forward bulkhead: a door to the lower deck, and one to the
    // upper deck at the top of the ramp.
    boxes.extend(wall_across(14.0, (-w, w), (0.0, 3.0), t, &[(-1.0, 1.0, 2.4)]));
    boxes.extend(wall_across(14.0, (-w, w), (UPPER, top), t, &[(-5.8, -3.8, UPPER + 2.4)]));
    // The ramp, up the hangar's port wall from z = 27 (floor) to z = 15
    // (upper deck), and a landing to the door.
    let (run, rise) = (12.0f32, UPPER);
    let slope = (rise / run).atan();
    let along = (run * run + rise * rise).sqrt();
    let rotation = Quat::from_rotation_x(slope);
    let normal = rotation * Vec3::Y;
    boxes.push(HullBox {
        center: v(-4.8, rise / 2.0, 21.0) - normal * (t / 2.0),
        size: v(2.4, t, along),
        rotation,
        material: SimMaterial::Hull,
    });
    boxes.push(span(v(-w, 3.0, 14.15), v(-3.6, UPPER, 15.2)));
    // Lower deck: a corridor (x = ±2) from the hangar to a forward room
    // (z < −24), doors into the side rooms, which are split at z = 0.
    for x in [-2.0, 2.0] {
        boxes.extend(wall_along(x, (-24.0, 14.0), (0.0, 3.0), wall, &[(-7.0, -5.4, 2.2), (7.0, 8.6, 2.2)]));
    }
    boxes.extend(wall_across(0.0, (-w, -2.0), (0.0, 3.0), wall, &[]));
    boxes.extend(wall_across(0.0, (2.0, w), (0.0, 3.0), wall, &[]));
    // Upper deck: the bridge (z < −22) behind a bulkhead; a corridor
    // (x = ±2) aft of it, doors into the side rooms.
    boxes.extend(wall_across(-22.0, (-w, w), (UPPER, top), t, &[(-1.0, 1.0, UPPER + 2.2)]));
    for x in [-2.0, 2.0] {
        boxes.extend(wall_along(x, (-22.0, 14.0), (UPPER, top), wall, &[(-13.0, -11.4, UPPER + 2.2), (3.0, 4.6, UPPER + 2.2)]));
    }
    // Furniture: the bridge console; crates in the hangar and rooms.
    boxes.extend([
        trim(carrier::CONSOLE, v(4.0, 1.0, 0.8)),
        trim(v(3.0, 0.75, 20.0), v(1.5, 1.5, 1.5)),
        trim(v(4.4, 0.6, 24.5), v(1.2, 1.2, 1.2)),
        trim(v(1.5, 0.5, 26.0), v(1.0, 1.0, 1.4)),
        trim(v(-4.0, 0.6, 6.0), v(1.2, 1.2, 2.0)),
        trim(v(4.0, UPPER + 0.6, -6.0), v(1.6, 1.2, 1.2)),
    ]);
    // Outside: engines at the stern, a dorsal spine.
    boxes.extend([
        hull(v(-8.5, 3.0, 24.0), v(3.0, 3.0, 14.0)),
        hull(v(8.5, 3.0, 24.0), v(3.0, 3.0, 14.0)),
        trim(v(-6.8, 3.0, 24.0), v(0.6, 0.6, 6.0)),
        trim(v(6.8, 3.0, 24.0), v(0.6, 0.6, 6.0)),
        trim(v(0.0, top + t + 0.5, -2.0), v(3.0, 1.0, 30.0)),
    ]);
    for b in &mut boxes {
        if b.material == SimMaterial::Hull {
            b.material = SimMaterial::HostileHull;
        }
    }
    boxes
}

/// The boxes a hull is built from: center, full size, turn, and paint.
pub fn hull_pieces(kind: HullKind) -> Vec<(Vec3, Vec3, Quat, SimMaterial)> {
    hull_layout(kind)
        .into_iter()
        .map(|b| (b.center, b.size, b.rotation, b.material))
        .collect()
}

/// Something on a ship a player can press (by clicking it, within reach).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlAction {
    /// Leave whatever is going on and start this scenario.
    Scenario(ScenarioKind),
    /// Lurch the ship forward.
    Lurch,
    /// Drop this ship's shield.
    DisableShields,
    /// Take a full set of guns and ammo.
    Armory,
}

impl ControlAction {
    /// What the button says.
    pub fn label(self) -> &'static str {
        match self {
            ControlAction::Scenario(kind) => kind.button_label(),
            ControlAction::Lurch => "Lurch (DO NOT PRESS)",
            ControlAction::DisableShields => "Disable shields",
            ControlAction::Armory => "Weapons locker",
        }
    }
}

/// A button fitted to a ship: an action, and the box (centered on the
/// entity's transform, in ship coordinates) a player clicks on.
#[derive(Component, Clone, Copy, Debug)]
pub struct Control {
    pub action: ControlAction,
    pub half_size: Vec3,
}

/// A laser turret fitted to a ship (see [`super::combat`]).
#[derive(Component, Clone, Copy, Debug)]
pub struct Turret {
    /// Which of the ship's turrets: sets its place in the firing cycle.
    pub index: u8,
}

/// The controls fitted to each kind of hull: (action, center, half size).
fn controls(kind: HullKind) -> Vec<(ControlAction, Vec3, Vec3)> {
    let v = Vec3::new;
    match kind {
        HullKind::Home => {
            // Buttons on the mission wall, sticking out of the panel.
            let button = v(0.06, 0.28, 0.4);
            let mut buttons: Vec<_> = ScenarioKind::ALL
                .iter()
                .enumerate()
                .map(|(i, kind)| {
                    (
                        ControlAction::Scenario(*kind),
                        v(-3.54, 1.75, -8.4 + 1.1 * i as f32),
                        button,
                    )
                })
                .collect();
            // Lower, and apart from the rest.
            buttons.push((ControlAction::Lurch, v(-3.54, 1.05, -5.4), v(0.06, 0.18, 0.25)));
            // The weapons locker, on the starboard wall of the aft hold.
            buttons.push((ControlAction::Armory, v(3.54, 1.5, 9.0), v(0.06, 0.45, 0.5)));
            buttons
        }
        // A big button on top of the bridge console.
        HullKind::Carrier => vec![(
            ControlAction::DisableShields,
            carrier::CONSOLE + v(0.0, 0.56, 0.0),
            v(0.35, 0.06, 0.3),
        )],
    }
}

/// Visual-only hull piece; the collision shape is the ship's compound collider.
#[derive(Component, Clone, Copy, Debug)]
pub struct HullPart {
    pub size: Vec3,
}

/// The ship's structure. Its geometry is built from the layout wherever the
/// component appears (see [`build_hull`]), so replicating this is replicating
/// the hull.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShipHull(pub HullKind);

/// The crew's ship, drifting with the field at the origin.
pub fn drifting_home_ship() -> impl Bundle {
    (
        Name::new("Ship"),
        HomeShip,
        Drift::default(),
        ShipHull(HullKind::Home),
        RigidBody::Kinematic,
        SleepingDisabled,
        PreviousVelocity::default(),
        Position::default(),
        Rotation::default(),
    )
}

/// Gives a new [`ShipHull`] its collider and visual parts.
///
/// The ship's collision is one compound collider on the body entity itself.
/// Avian only re-syncs *child* collider poses at the start of each physics
/// step, so child colliders would be queried a tick stale (v·dt off) by the
/// player controller, which runs after the step.
pub fn build_hull(add: On<Add, ShipHull>, hulls: Query<&ShipHull>, mut commands: Commands) {
    let kind = hulls.get(add.entity).expect("just added").0;
    let layout = hull_layout(kind);
    let collider = Collider::compound(
        layout
            .iter()
            .map(|b| {
                (
                    b.center,
                    b.rotation,
                    Collider::cuboid(b.size.x, b.size.y, b.size.z),
                )
            })
            .collect(),
    );
    commands
        .entity(add.entity)
        .insert((BodyInertia::immovable(&collider), collider, Visibility::default()))
        // A replicated hull may not have its pose yet.
        .insert_if_new(Transform::default())
        .with_children(|ship| {
            for (i, b) in layout.into_iter().enumerate() {
                ship.spawn((
                    Name::new(format!("Hull {i}")),
                    HullPart { size: b.size },
                    Transform::from_translation(b.center).with_rotation(b.rotation),
                    b.material,
                ));
            }
            for (action, center, half_size) in controls(kind) {
                ship.spawn((
                    Name::new(action.label()),
                    Control { action, half_size },
                    Transform::from_translation(center),
                ));
            }
            for (index, mount) in kind.turret_mounts().into_iter().enumerate() {
                ship.spawn((
                    Name::new(format!("Turret {index}")),
                    Turret { index: index as u8 },
                    Transform::from_translation(mount),
                ));
            }
        });
}

/// Drives drifting ships: a slow roll, a gentle yaw wobble, and an
/// optional forward lurch limited by `ship_max_accel`.
pub fn drive_ship(
    time: Res<Time>,
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut orders: MessageReader<ShipCommand>,
    mut ships: Query<(
        &mut Drift,
        &Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    let dt = time.delta_secs();
    let lurch = orders.read().any(|o| *o == ShipCommand::Lurch);
    // Wobble phase from the tick, exact however long the game runs.
    let wobble_ticks = WOBBLE_SECONDS * TICK_HZ as u32;
    let phase = (tick.0 % wobble_ticks) as f32 / wobble_ticks as f32 * std::f32::consts::TAU;
    for (mut ship, rot, mut lin, mut ang) in &mut ships {
        if lurch {
            ship.lurch_start = Some(tick.0);
        }
        let wobble_freq = std::f32::consts::TAU / WOBBLE_SECONDS as f32;
        let yaw_rate = tuning.ship_yaw_wobble_deg.to_radians() * wobble_freq * phase.cos();
        let roll_axis = rot.0 * SHIP_FORWARD;
        ang.0 = roll_axis * tuning.ship_roll_rate_deg.to_radians() + rot.0 * Vec3::Y * yaw_rate;

        let lurching = ship.lurch_start.is_some_and(|start| {
            (tick.0.wrapping_sub(start) as f32 / TICK_HZ as f32) < LURCH_SECONDS
        });
        let target = if lurching {
            rot.0 * SHIP_FORWARD * tuning.lurch_delta_v
        } else {
            Vec3::ZERO
        };
        let max_dv = tuning.ship_max_accel * dt;
        let dv = (target - lin.0).clamp_length_max(max_dv);
        lin.0 += dv;
    }
}
