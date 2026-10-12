//! Devices: the interactive things fitted to a ship (or a site) beyond
//! one-shot buttons, and the simple logic that wires them together.
//!
//! A design places devices as data (`devices: [...]` in a module, see
//! docs/ships.md): levers, keys (timed switches), power junctions, doors,
//! lamps, boards of labelled lamps, room lights, conduits, and gates to fly
//! through. Each has a **power** condition over the others' outputs ("this
//! door opens when both keys are turned", "this junction is fed by way 2 of
//! that one"), and an output of its own:
//!
//! | Kind | Pressed | Output (when powered) |
//! |---|---|---|
//! | `Lever` | flips it | on while thrown |
//! | `Pull` | pulls it, once and for good | on once pulled |
//! | `Key` | turns it | on for `hold` seconds (`solo` seconds for a crew of one) |
//! | `Junction` | turns it to its next way | powered; way *k* is live when it's set to *k* |
//! | `Door` | (not pressable) | open; a `latch` door stays open once opened |
//! | `Bridge` | (not pressable) | extended: a floor panel (a gangway) that's solid only then |
//! | `Sensor` | (not pressable) | on while a player stands in its box (so a door can stay open over a doorway) |
//! | `Lamp`, `Board`, `Lights`, `Conduit`, `Gate` | (not pressable) | lit |
//!
//! A press works whether or not the device has power: a lever thrown, a
//! junction turned or a key turned while dead takes effect when the power
//! comes (a key only while its time lasts). It doesn't work with full
//! hands: someone carrying bulky cargo can't work a switch.
//!
//! The authority keeps the state of a ship's devices in its [`Devices`]
//! (replicated): it applies presses (the players' `Pressed` events, through
//! the usual interact path), then evaluates every device in dependency
//! order once a tick, so a whole chain settles in one tick. Every peer builds
//! the closed doors into the ship's collider from that state (see
//! [`fit_doors`]); the client draws the rest (see `client::devices`).

use std::collections::HashMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bots::Robot;
use super::events::PlayerEvent;
use super::player::Player;
use super::ship::{ControlAction, ShipHull};
use super::{SimTick, TICK_HZ};

/// A device as a module file gives it, in the module's frame.
#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct DeviceFile {
    /// Its name among the ship's devices, for other devices' conditions
    /// and for scenarios.
    pub id: String,
    /// What a player sees it called (by default, its id).
    #[serde(default)]
    pub label: String,
    pub kind: KindFile,
    /// Its middle: of the box a player presses, the doorway a door fills,
    /// the gate's opening. Lights and conduits give their own points.
    #[serde(default)]
    pub at: Vec3,
    /// The way its face looks: out of the wall it's mounted on (or a gate's
    /// way through).
    #[serde(default = "up")]
    pub face: Vec3,
    /// When it has power (by default, always).
    #[serde(default)]
    pub power: Cond,
    /// How it starts: a lever 1 (thrown) or 0, a junction on this way.
    #[serde(default)]
    pub start: u8,
    /// A hit on the ship near it can knock it out of its setting (a lever
    /// thrown back, a junction turned to another way); see
    /// [`Device::knock`].
    #[serde(default)]
    pub fault: bool,
}

fn up() -> Vec3 {
    Vec3::Y
}

#[derive(Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub enum KindFile {
    Lever,
    Pull,
    Key {
        hold: f32,
        #[serde(default)]
        solo: Option<f32>,
    },
    Junction {
        ways: u8,
    },
    Door {
        size: Vec3,
        #[serde(default)]
        latch: bool,
    },
    Bridge {
        size: Vec3,
    },
    Sensor {
        size: Vec3,
    },
    Lamp,
    Board {
        size: Vec2,
        rows: Vec<(String, Cond)>,
    },
    Lights {
        at: Vec<Vec3>,
        #[serde(default = "white")]
        color: (f32, f32, f32),
        /// An alarm: flashes while it's on.
        #[serde(default)]
        flash: bool,
    },
    Conduit {
        path: Vec<Vec3>,
    },
    Gate {
        size: Vec2,
    },
}

fn white() -> (f32, f32, f32) {
    (1.0, 0.92, 0.8)
}

/// A condition over other devices, by id, as a design writes it.
#[derive(Deserialize, Clone, Debug, Default)]
pub enum Cond {
    #[default]
    Always,
    Never,
    /// That device's output: a thrown lever, a turned key, a powered
    /// junction, an open door, a lit lamp.
    On(String),
    /// A junction powered and set to that way.
    Port(String, u8),
    All(Vec<Cond>),
    Any(Vec<Cond>),
    Not(Box<Cond>),
}

/// A condition, resolved to device indices.
#[derive(Clone, Debug, PartialEq)]
pub enum Logic {
    Always,
    Never,
    On(u16),
    Port(u16, u8),
    All(Vec<Logic>),
    Any(Vec<Logic>),
    Not(Box<Logic>),
}

impl Logic {
    pub fn eval(&self, states: &[DeviceState]) -> bool {
        match self {
            Logic::Always => true,
            Logic::Never => false,
            Logic::On(i) => states[*i as usize].live,
            Logic::Port(i, way) => {
                let s = states[*i as usize];
                s.live && s.set == *way
            }
            Logic::All(all) => all.iter().all(|l| l.eval(states)),
            Logic::Any(any) => any.iter().any(|l| l.eval(states)),
            Logic::Not(not) => !not.eval(states),
        }
    }

    fn depends_on(&self, out: &mut Vec<u16>) {
        match self {
            Logic::Always | Logic::Never => {}
            Logic::On(i) | Logic::Port(i, _) => out.push(*i),
            Logic::All(l) | Logic::Any(l) => l.iter().for_each(|l| l.depends_on(out)),
            Logic::Not(l) => l.depends_on(out),
        }
    }
}

/// What a device is, in ship coordinates.
#[derive(Clone, Debug, PartialEq)]
pub enum DeviceKind {
    Lever,
    /// A handle pulled once, for good (a lever that stays thrown).
    Pull,
    Key { hold: f32, solo: Option<f32> },
    Junction { ways: u8 },
    /// A door filling the box `size` (full size, axis-aligned in the ship)
    /// round the device's `at`.
    Door { size: Vec3, latch: bool },
    /// A floor panel filling the box `size` (as a door's) while it's live:
    /// a gangway, extended.
    Bridge { size: Vec3 },
    /// The box `size` round `at` (as a door's): live while a player (not a
    /// robot) is in it, and powered.
    Sensor { size: Vec3 },
    Lamp,
    Board { size: Vec2, rows: Vec<(String, Logic)> },
    /// Room lights at these points; an alarm flashes.
    Lights { at: Vec<Vec3>, color: Vec3, flash: bool },
    Conduit { path: Vec<Vec3> },
    /// An opening `size` across (in the face's right and up), to fly
    /// through along the face.
    Gate { size: Vec2 },
}

/// A device fitted to a ship, in ship coordinates.
#[derive(Clone, Debug)]
pub struct Device {
    pub id: String,
    pub label: String,
    pub kind: DeviceKind,
    pub at: Vec3,
    /// Unit, out of its mounting surface.
    pub face: Vec3,
    pub power: Logic,
    pub start: u8,
    /// Whether hits can knock it (see [`Device::knock`]).
    pub fault: bool,
}

/// How thick a gate's frame is (m), and how deep.
pub const GATE_FRAME: f32 = 0.5;

impl Device {
    /// Whether it's out of the setting it starts in (knocked, or worked).
    pub fn displaced(&self, state: &DeviceState) -> bool {
        state.set != self.start
    }

    /// A hit knocks it out of its starting setting: a lever is thrown the
    /// other way, a junction turned to another of its ways (`roll` picks
    /// which). Whether it moved.
    pub fn knock(&self, state: &mut DeviceState, roll: u32) -> bool {
        match self.kind {
            DeviceKind::Lever if state.set == self.start => {
                state.set ^= 1;
                true
            }
            DeviceKind::Junction { ways } if ways > 1 && state.set == self.start => {
                state.set = (self.start + 1 + (roll % (ways as u32 - 1)) as u8) % ways;
                true
            }
            _ => false,
        }
    }

    /// The box (full size, axis-aligned in the ship, round `at`) it fills
    /// in the ship's collider while `state` holds it there: a shut door, an
    /// extended bridge.
    pub fn solid(&self, state: Option<&DeviceState>) -> Option<Vec3> {
        let live = state.is_some_and(|s| s.live);
        match self.kind {
            DeviceKind::Door { size, .. } if !live => Some(size),
            DeviceKind::Bridge { size } if live => Some(size),
            _ => None,
        }
    }

    /// Whether `p` (ship coordinates) is in a sensor's box.
    pub fn senses(&self, p: Vec3) -> bool {
        match self.kind {
            DeviceKind::Sensor { size } => (p - self.at).abs().cmple(size / 2.0).all(),
            _ => false,
        }
    }

    /// Whether a player presses it.
    pub fn pressable(&self) -> bool {
        matches!(
            self.kind,
            DeviceKind::Lever | DeviceKind::Pull | DeviceKind::Key { .. } | DeviceKind::Junction { .. }
        )
    }

    /// The axes of its face: right, up, and out.
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        face_basis(self.face)
    }

    /// The box a player presses (half size, axis-aligned in the ship,
    /// centred on `at`).
    pub fn press_box(&self) -> Vec3 {
        // Across, up and out of its face.
        let local = match self.kind {
            DeviceKind::Lever | DeviceKind::Pull => Vec3::new(0.18, 0.3, 0.12),
            DeviceKind::Key { .. } => Vec3::new(0.18, 0.18, 0.1),
            DeviceKind::Junction { .. } => Vec3::new(0.35, 0.3, 0.12),
            _ => Vec3::ZERO,
        };
        let (r, u, n) = self.basis();
        r.abs() * local.x + u.abs() * local.y + n.abs() * local.z
    }

    /// Whether the straight move from `a` to `b` (ship coordinates) goes
    /// through this gate's opening, either way.
    pub fn passed_through(&self, a: Vec3, b: Vec3) -> bool {
        let DeviceKind::Gate { size } = self.kind else {
            return false;
        };
        let (r, u, n) = self.basis();
        let (da, db) = ((a - self.at).dot(n), (b - self.at).dot(n));
        if da.signum() == db.signum() || da == db {
            return false;
        }
        let p = a + (b - a) * (da / (da - db)) - self.at;
        p.dot(r).abs() <= size.x / 2.0 && p.dot(u).abs() <= size.y / 2.0
    }

    /// A gate's frame, as boxes (centre, full size along the face's right,
    /// up and out, and the turn to those axes) in the ship.
    pub fn gate_frame(&self) -> Vec<(Vec3, Vec3, Quat)> {
        let DeviceKind::Gate { size } = self.kind else {
            return Vec::new();
        };
        let (r, u, n) = self.basis();
        let turn = Quat::from_mat3(&Mat3::from_cols(r, u, n));
        let t = GATE_FRAME;
        let across = Vec3::new(size.x + 2.0 * t, t, t);
        let side = Vec3::new(t, size.y, t);
        vec![
            (self.at + u * (size.y + t) / 2.0, across, turn),
            (self.at - u * (size.y + t) / 2.0, across, turn),
            (self.at + r * (size.x + t) / 2.0, side, turn),
            (self.at - r * (size.x + t) / 2.0, side, turn),
        ]
    }
}

/// Right, up and out for a face looking along `n` (up is +Y, or −Z for a
/// face looking up or down).
pub fn face_basis(n: Vec3) -> (Vec3, Vec3, Vec3) {
    let n = n.normalize_or(Vec3::Y);
    let up = if n.y.abs() > 0.9 { Vec3::NEG_Z } else { Vec3::Y };
    let right = up.cross(n).normalize();
    let up = n.cross(right);
    (right, up, n)
}

/// Turns a module's devices into the ship's: `to_ship` places a point,
/// `turn` turns a direction, `extent` turns an axis-aligned half size.
pub fn place(
    file: &DeviceFile,
    to_ship: impl Fn(Vec3) -> Vec3,
    turn: Quat,
    extent: impl Fn(Vec3) -> Vec3,
) -> (Device, Vec<(String, Cond)>) {
    let mut rows = Vec::new();
    let kind = match &file.kind {
        KindFile::Lever => DeviceKind::Lever,
        KindFile::Pull => DeviceKind::Pull,
        KindFile::Key { hold, solo } => DeviceKind::Key { hold: *hold, solo: *solo },
        KindFile::Junction { ways } => DeviceKind::Junction { ways: (*ways).max(1) },
        KindFile::Door { size, latch } => DeviceKind::Door {
            size: extent(*size / 2.0) * 2.0,
            latch: *latch,
        },
        KindFile::Bridge { size } => DeviceKind::Bridge {
            size: extent(*size / 2.0) * 2.0,
        },
        KindFile::Sensor { size } => DeviceKind::Sensor {
            size: extent(*size / 2.0) * 2.0,
        },
        KindFile::Lamp => DeviceKind::Lamp,
        KindFile::Board { size, rows: r } => {
            rows = r.clone();
            DeviceKind::Board {
                size: *size,
                rows: Vec::new(),
            }
        }
        KindFile::Lights { at, color, flash } => DeviceKind::Lights {
            at: at.iter().map(|&p| to_ship(p)).collect(),
            color: Vec3::new(color.0, color.1, color.2),
            flash: *flash,
        },
        KindFile::Conduit { path } => DeviceKind::Conduit {
            path: path.iter().map(|&p| to_ship(p)).collect(),
        },
        KindFile::Gate { size } => DeviceKind::Gate { size: *size },
    };
    let device = Device {
        id: file.id.clone(),
        label: if file.label.is_empty() { file.id.clone() } else { file.label.clone() },
        kind,
        at: to_ship(file.at),
        face: (turn * file.face).normalize_or(Vec3::Y),
        power: Logic::Never,
        start: file.start,
        fault: file.fault,
    };
    (device, rows)
}

/// Resolves every device's conditions to indices (`conds[i]` is device
/// `i`'s power, and its board rows if it has any), and orders the devices
/// so each comes after everything its conditions read.
pub fn wire(devices: &mut [Device], conds: Vec<(Cond, Vec<(String, Cond)>)>) -> Result<Vec<u16>, String> {
    let index: HashMap<String, u16> = devices.iter().enumerate().map(|(i, d)| (d.id.clone(), i as u16)).collect();
    if index.len() != devices.len() {
        return Err("two devices share an id".into());
    }
    let resolve = |c: &Cond| resolve(c, &index, devices);
    let mut wired = Vec::new();
    for (power, rows) in &conds {
        let rows: Result<Vec<_>, String> = rows.iter().map(|(l, c)| Ok((l.clone(), resolve(c)?))).collect();
        wired.push((resolve(power)?, rows?));
    }
    for (device, (power, rows)) in devices.iter_mut().zip(wired) {
        device.power = power;
        if let DeviceKind::Board { rows: r, .. } = &mut device.kind {
            *r = rows;
        }
    }
    for d in devices.iter() {
        let settings = match d.kind {
            DeviceKind::Lever | DeviceKind::Pull => 2,
            DeviceKind::Junction { ways } => ways,
            _ => 1,
        };
        if d.start >= settings {
            return Err(format!("device {:?} can't start at {} (it has {settings} settings)", d.id, d.start));
        }
    }
    // Kahn's algorithm, lowest index first so the order is stable.
    let n = devices.len();
    let deps: Vec<Vec<u16>> = devices
        .iter()
        .map(|d| {
            let mut out = Vec::new();
            d.power.depends_on(&mut out);
            out
        })
        .collect();
    let mut done = vec![false; n];
    let mut order = Vec::new();
    while order.len() < n {
        let next = (0..n).find(|&i| !done[i] && deps[i].iter().all(|&j| done[j as usize] || j as usize == i));
        let Some(i) = next else {
            let stuck: Vec<&str> = (0..n).filter(|&i| !done[i]).map(|i| devices[i].id.as_str()).collect();
            return Err(format!("devices power each other in a loop: {}", stuck.join(", ")));
        };
        if deps[i].contains(&(i as u16)) {
            return Err(format!("device {:?} is powered by itself", devices[i].id));
        }
        done[i] = true;
        order.push(i as u16);
    }
    Ok(order)
}

fn resolve(c: &Cond, index: &HashMap<String, u16>, devices: &[Device]) -> Result<Logic, String> {
    let id = |name: &str| index.get(name).copied().ok_or_else(|| format!("no device {name:?}"));
    Ok(match c {
        Cond::Always => Logic::Always,
        Cond::Never => Logic::Never,
        Cond::On(name) => Logic::On(id(name)?),
        Cond::Port(name, way) => {
            let i = id(name)?;
            match devices[i as usize].kind {
                DeviceKind::Junction { ways } if *way < ways => Logic::Port(i, *way),
                DeviceKind::Junction { ways } => return Err(format!("junction {name:?} has {ways} ways, not {}", way + 1)),
                _ => return Err(format!("{name:?} isn't a junction")),
            }
        }
        Cond::All(all) => Logic::All(all.iter().map(|c| resolve(c, index, devices)).collect::<Result<_, _>>()?),
        Cond::Any(any) => Logic::Any(any.iter().map(|c| resolve(c, index, devices)).collect::<Result<_, _>>()?),
        Cond::Not(not) => Logic::Not(Box::new(resolve(not, index, devices)?)),
    })
}

/// One device's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceState {
    /// A lever: 1 thrown, 0 not. A junction: its way.
    pub set: u8,
    /// A key: turned until this tick.
    pub until: u32,
    /// Its power condition holds.
    pub powered: bool,
    /// Its output (see the module docs).
    pub live: bool,
}

/// The state of every device on a ship, in its design's order. The
/// authority's, replicated.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Devices(pub Vec<DeviceState>);

impl Devices {
    /// As the design starts them, settled.
    pub fn new(design: &super::design::ShipDesign) -> Devices {
        let mut states: Vec<DeviceState> = design
            .devices
            .iter()
            .map(|d| DeviceState {
                set: d.start,
                ..default()
            })
            .collect();
        step(design, &mut states, 0);
        Devices(states)
    }

    /// The output of the device called `id` (false if there's none).
    pub fn live(&self, design: &super::design::ShipDesign, id: &str) -> bool {
        design.device_index(id).is_some_and(|i| self.0[i].live)
    }
}

/// Evaluates every device's power and output at `tick`, in dependency
/// order.
pub fn step(design: &super::design::ShipDesign, states: &mut [DeviceState], tick: u32) {
    for &i in &design.device_order {
        let i = i as usize;
        let device = &design.devices[i];
        let powered = device.power.eval(states);
        let s = &mut states[i];
        s.powered = powered;
        s.live = match device.kind {
            DeviceKind::Lever | DeviceKind::Pull => powered && s.set == 1,
            DeviceKind::Key { .. } => powered && tick < s.until,
            DeviceKind::Sensor { .. } => powered && s.set == 1,
            DeviceKind::Door { latch: true, .. } => s.live || powered,
            _ => powered,
        };
    }
}

/// Applies a press to a device, at `tick`, for a crew of `crew`.
pub fn press(device: &Device, state: &mut DeviceState, tick: u32, crew: usize) {
    match device.kind {
        DeviceKind::Lever => state.set ^= 1,
        DeviceKind::Pull => state.set = 1,
        DeviceKind::Key { hold, solo } => {
            let seconds = if crew <= 1 { solo.unwrap_or(hold) } else { hold };
            state.until = tick + (seconds * TICK_HZ as f32).round() as u32;
        }
        DeviceKind::Junction { ways } => state.set = (state.set + 1) % ways,
        _ => {}
    }
}

/// Authority: applies this tick's presses of devices, then settles every
/// ship's devices. Only writes a ship's state when it changes, so it's
/// replicated only then.
pub fn run_devices(
    tick: Res<SimTick>,
    tuning: Res<super::tuning::Tuning>,
    bodies: super::bodies::Bodies,
    players: Query<(&Player, Has<Robot>)>,
    mut ships: Query<(Entity, &ShipHull, &mut Devices)>,
) {
    let crew = players.iter().filter(|(_, robot)| !robot).count();
    for (ship, hull, mut devices) in &mut ships {
        let design = hull.design();
        let mut states = devices.0.clone();
        // Sensors: whether a crew member's middle is in each one.
        if design.devices.iter().any(|d| matches!(d.kind, DeviceKind::Sensor { .. }))
            && let Some(frame) = bodies.frame(ship)
        {
            let inside: Vec<Vec3> = players
                .iter()
                .filter(|(_, robot)| !robot)
                .map(|(p, _)| {
                    let own = p.mode.body().and_then(|b| bodies.frame(b));
                    frame.point_to_local(p.mode.world_kinematics(&tuning, own.as_ref()).0)
                })
                .collect();
            for (i, d) in design.devices.iter().enumerate() {
                if matches!(d.kind, DeviceKind::Sensor { .. }) {
                    states[i].set = inside.iter().any(|&p| d.senses(p)) as u8;
                }
            }
        }
        for (player, _) in &players {
            // Full hands can't work a switch.
            if player.loadout.hands_full() {
                continue;
            }
            for (at, event) in player.events.iter() {
                if let PlayerEvent::Pressed {
                    body,
                    action: ControlAction::Device(i),
                } = event
                    && at == tick.0
                    && body == ship
                    && let Some(device) = design.devices.get(i as usize)
                {
                    press(device, &mut states[i as usize], tick.0, crew);
                }
            }
        }
        step(design, &mut states, tick.0);
        if states != devices.0 {
            devices.0 = states;
        }
    }
}

/// Which of a ship's doors its collider was last built with closed.
#[derive(Component, Clone, Debug, Default, PartialEq)]
pub struct DoorsBuilt(pub Vec<bool>);

/// Every peer: a ship's closed doors are part of its collider. Rebuilt
/// when a door opens or closes.
pub fn fit_doors(
    mut ships: Query<(&ShipHull, &Devices, &mut DoorsBuilt, &mut avian3d::prelude::Collider), Changed<Devices>>,
) {
    for (hull, devices, mut built, mut collider) in &mut ships {
        let design = hull.design();
        let closed = closed_doors(design, Some(devices));
        if closed != built.0 {
            *collider = super::ship::ship_collider(design, &closed);
            built.0 = closed;
        }
    }
}

/// For each device, whether it's in the ship's collider just now (a shut
/// door, an extended bridge; until the state is known, doors are shut and
/// bridges run in).
pub fn closed_doors(design: &super::design::ShipDesign, devices: Option<&Devices>) -> Vec<bool> {
    design
        .devices
        .iter()
        .enumerate()
        .map(|(i, d)| d.solid(devices.and_then(|s| s.0.get(i))).is_some())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::sim::design::{ShipDesign, assemble};

    /// A design of one module holding `devices` (RON).
    fn design(devices: &str) -> Result<ShipDesign, String> {
        let module = format!("Module(devices: [{devices}])");
        let files = HashMap::from([("m", module.as_str())]);
        let text = "Design(name: \"T\", hostile: false, model: None, \
                    shield: (center: (0.0, 0.0, 0.0), radii: (1.0, 1.0, 1.0)), turrets: [], \
                    modules: [(module: \"m\")])";
        assemble("t", text, &files)
    }

    fn press_id(design: &ShipDesign, states: &mut [DeviceState], id: &str, tick: u32, crew: usize) {
        let i = design.device(id);
        press(&design.devices[i], &mut states[i], tick, crew);
    }

    fn live(design: &ShipDesign, states: &[DeviceState], id: &str) -> bool {
        states[design.device(id)].live
    }

    #[test]
    fn a_door_opens_when_its_power_does_and_a_latch_keeps_it_open() {
        let d = design(
            "(id: \"lever\", kind: Lever), \
             (id: \"plain\", kind: Door(size: (1.0, 2.0, 0.1)), power: On(\"lever\")), \
             (id: \"latched\", kind: Door(size: (1.0, 2.0, 0.1), latch: true), power: On(\"lever\"))",
        )
        .unwrap();
        let mut s = Devices::new(&d).0;
        assert!(!live(&d, &s, "plain") && !live(&d, &s, "latched"));
        assert_eq!(closed_doors(&d, Some(&Devices(s.clone()))), vec![false, true, true]);
        press_id(&d, &mut s, "lever", 1, 1);
        step(&d, &mut s, 1);
        assert!(live(&d, &s, "plain") && live(&d, &s, "latched"));
        assert_eq!(closed_doors(&d, Some(&Devices(s.clone()))), vec![false, false, false]);
        press_id(&d, &mut s, "lever", 2, 1);
        step(&d, &mut s, 2);
        assert!(!live(&d, &s, "plain"), "an unlatched door shuts with its power");
        assert!(live(&d, &s, "latched"), "a latched door stays open");
    }

    #[test]
    fn power_follows_the_junctions_ways_in_one_tick() {
        // source -> a; a's way 1 feeds b; b's way 0 lights the lamp.
        let d = design(
            "(id: \"lamp\", kind: Lamp, power: Port(\"b\", 0)), \
             (id: \"b\", kind: Junction(ways: 2), power: Port(\"a\", 1), start: 1), \
             (id: \"a\", kind: Junction(ways: 3), power: On(\"source\")), \
             (id: \"source\", kind: Pull)",
        )
        .unwrap();
        let mut s = Devices::new(&d).0;
        assert!(!live(&d, &s, "a"), "unpowered before the source is pulled");
        press_id(&d, &mut s, "source", 1, 1);
        step(&d, &mut s, 1);
        assert!(live(&d, &s, "a") && !live(&d, &s, "b"));
        press_id(&d, &mut s, "a", 2, 1);
        step(&d, &mut s, 2);
        assert!(live(&d, &s, "b"), "a's way 1 feeds b");
        assert!(!live(&d, &s, "lamp"), "b is on its way 1");
        press_id(&d, &mut s, "b", 3, 1);
        step(&d, &mut s, 3);
        // Declared before what feeds it, and still lit the same tick.
        assert!(live(&d, &s, "lamp"));
        // A junction wraps round its ways; a pull stays pulled.
        press_id(&d, &mut s, "a", 4, 1);
        press_id(&d, &mut s, "source", 4, 1);
        step(&d, &mut s, 4);
        assert_eq!(s[d.device("a")].set, 2);
        assert!(live(&d, &s, "source") && !live(&d, &s, "b") && !live(&d, &s, "lamp"));
    }

    #[test]
    fn a_two_key_lock_opens_only_when_both_are_turned_at_once() {
        let d = design(
            "(id: \"a\", kind: Key(hold: 1.5)), \
             (id: \"b\", kind: Key(hold: 1.5, solo: Some(5.0))), \
             (id: \"vault\", kind: Door(size: (1.0, 2.0, 0.1), latch: true), power: All([On(\"a\"), On(\"b\")]))",
        )
        .unwrap();
        let hz = TICK_HZ as u32;
        // A crew of two, 2 s apart: too late.
        let mut s = Devices::new(&d).0;
        press_id(&d, &mut s, "a", 100, 2);
        for t in 100..100 + 2 * hz {
            step(&d, &mut s, t);
        }
        press_id(&d, &mut s, "b", 100 + 2 * hz, 2);
        for t in 100 + 2 * hz..100 + 4 * hz {
            step(&d, &mut s, t);
            assert!(!live(&d, &s, "vault"), "opened at tick {t}");
        }
        // One second apart: open, and it stays open after the keys spring
        // back.
        press_id(&d, &mut s, "a", 1000, 2);
        step(&d, &mut s, 1000);
        press_id(&d, &mut s, "b", 1000 + hz, 2);
        step(&d, &mut s, 1000 + hz);
        assert!(live(&d, &s, "vault"));
        step(&d, &mut s, 2000);
        assert!(!live(&d, &s, "a") && !live(&d, &s, "b") && live(&d, &s, "vault"));
        // Alone, key b holds 5 s: 4 s is in time.
        let mut s = Devices::new(&d).0;
        press_id(&d, &mut s, "b", 100, 1);
        step(&d, &mut s, 100 + 4 * hz);
        press_id(&d, &mut s, "a", 100 + 4 * hz, 1);
        step(&d, &mut s, 100 + 4 * hz);
        assert!(live(&d, &s, "vault"));
    }

    #[test]
    fn bad_wiring_is_refused() {
        let err = |devices: &str| design(devices).err().unwrap_or_default();
        assert!(err("(id: \"a\", kind: Lamp, power: On(\"nope\"))").contains("no device \"nope\""));
        assert!(err("(id: \"a\", kind: Lamp, power: On(\"b\")), (id: \"b\", kind: Lamp, power: On(\"a\"))").contains("loop"));
        assert!(err("(id: \"a\", kind: Lamp, power: Port(\"b\", 0)), (id: \"b\", kind: Lamp)").contains("isn't a junction"));
        assert!(err("(id: \"a\", kind: Lamp, power: Port(\"j\", 2)), (id: \"j\", kind: Junction(ways: 2))").contains("2 ways"));
        assert!(err("(id: \"a\", kind: Lamp), (id: \"a\", kind: Lamp)").contains("share an id"));
        assert!(err("(id: \"j\", kind: Junction(ways: 2), start: 2)").contains("can't start at 2"));
    }

    #[test]
    fn a_gate_counts_a_move_through_its_opening_only() {
        let d = design("(id: \"g\", kind: Gate(size: (4.0, 4.0)), at: (0.0, 0.0, 10.0), face: (0.0, 0.0, 1.0))").unwrap();
        let g = &d.devices[0];
        assert!(g.passed_through(Vec3::new(1.0, 1.0, 9.5), Vec3::new(1.0, 1.0, 10.5)));
        assert!(g.passed_through(Vec3::new(0.0, 0.0, 11.0), Vec3::new(0.0, 0.0, 9.0)), "either way");
        assert!(!g.passed_through(Vec3::new(3.0, 0.0, 9.5), Vec3::new(3.0, 0.0, 10.5)), "beside it");
        assert!(!g.passed_through(Vec3::new(0.0, 0.0, 8.0), Vec3::new(0.0, 0.0, 9.9)), "short of it");
        // Its frame is structure.
        assert_eq!(d.boxes.len(), 4);
    }
}
