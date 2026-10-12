//! Ship designs, as data: what a ship is built from, what's fitted to it,
//! and who crews it.
//!
//! A design (`assets/ships/<ship>/design.ron`) places modules
//! (`assets/ships/<path>.ron`, any `.ron` that isn't a design), each in its
//! own frame, turned about +Y and moved into the ship's. A module holds
//! geometry (boxes, and walls, spans and ramps that expand to boxes), the
//! controls fitted to it, its interior light probes and ceiling lights, its
//! robot posts, and named markers (the boarders' `entry`, the `objective`).
//! The ship is everything its modules bring, in ship coordinates (metres,
//! +Y up, the bow toward −Z).
//!
//! Every file is embedded in the binary at build time (see `build.rs`), so
//! every peer has the same ships without loading anything, and the
//! simulation stays deterministic. [`check`] judges a design against what
//! play needs of it (see docs/ships.md).

use std::collections::HashMap;
use std::sync::LazyLock;

use bevy::prelude::*;
use serde::Deserialize;

use super::SimMaterial;
use super::bots::{self, Gait, Robot};
use super::devices::{Device, DeviceFile};
use super::mission::ScenarioKind;
use super::nav::NavGrid;
use super::ship::ControlAction;

include!(concat!(env!("OUT_DIR"), "/ship_files.rs"));

#[derive(Deserialize)]
#[serde(rename = "Design", deny_unknown_fields)]
struct DesignFile {
    /// What the HUD calls it.
    name: String,
    /// An enemy: its hull is hostile, and it's an enemy on the HUD.
    hostile: bool,
    /// Its model (under `assets/`), drawn in place of its boxes.
    model: Option<String>,
    shield: Shield,
    /// Laser turret mounts (each gets a base box under it).
    turrets: Vec<Vec3>,
    modules: Vec<Placement>,
}

/// A ship's shield: an ellipsoid around the hull.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Shield {
    pub center: Vec3,
    pub radii: Vec3,
}

/// A module, placed in a ship: moved to `at`, after turning `turn` degrees
/// about +Y (counterclockwise seen from above).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Placement {
    module: String,
    #[serde(default)]
    at: Vec3,
    #[serde(default)]
    turn: f32,
}

#[derive(Deserialize)]
#[serde(rename = "Module", deny_unknown_fields)]
struct ModuleFile {
    #[serde(default)]
    parts: Vec<Part>,
    #[serde(default)]
    controls: Vec<ControlSpec>,
    #[serde(default)]
    cabins: Vec<Cabin>,
    #[serde(default)]
    lights: Vec<Vec3>,
    #[serde(default)]
    posts: Vec<Post>,
    #[serde(default)]
    markers: HashMap<String, Vec3>,
    #[serde(default)]
    devices: Vec<DeviceFile>,
}

/// A piece of a module's geometry, in its frame.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
enum Part {
    /// A box: centre and full size.
    Box {
        center: Vec3,
        size: Vec3,
        #[serde(default)]
        material: Material,
    },
    /// A box from corner to corner.
    Span {
        lo: Vec3,
        hi: Vec3,
        #[serde(default)]
        material: Material,
    },
    /// A wall across the ship (in x, at `z`), `thickness` thick, with
    /// doorways: (from x, to x, top of the opening).
    WallAcross {
        z: f32,
        x: (f32, f32),
        y: (f32, f32),
        thickness: f32,
        #[serde(default)]
        doors: Vec<(f32, f32, f32)>,
    },
    /// A wall along the ship (in z, at `x`); doorways are (from z, to z,
    /// top).
    WallAlong {
        x: f32,
        z: (f32, f32),
        y: (f32, f32),
        thickness: f32,
        #[serde(default)]
        doors: Vec<(f32, f32, f32)>,
    },
    /// A ramp from the middle of its foot to the middle of its top edge,
    /// its walking surface on that line.
    Ramp {
        foot: Vec3,
        top: Vec3,
        width: f32,
        thickness: f32,
    },
}

#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub enum Material {
    /// Structure.
    #[default]
    Hull,
    /// Fittings and furniture.
    Trim,
}

/// A button: an action, and the box (centre, half size) a player clicks.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ControlSpec {
    action: ControlAction,
    center: Vec3,
    half_size: Vec3,
}

/// An interior volume, for an interior light probe.
#[derive(Deserialize, Clone, Copy, Debug)]
#[serde(deny_unknown_fields)]
pub struct Cabin {
    pub center: Vec3,
    pub size: Vec3,
}

/// Where a robot is posted: a walker stands there (its feet), a flyer
/// holds station there, a mount is bolted there (on a hull surface); facing
/// `facing` degrees about +Y from −Z.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Post {
    pub robot: Robot,
    pub at: Vec3,
    #[serde(default)]
    pub facing: f32,
}

impl Post {
    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.facing.to_radians())
    }
}

/// One box of a hull: centre, full size, turn, and paint, in ship
/// coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HullBox {
    pub center: Vec3,
    pub size: Vec3,
    pub rotation: Quat,
    pub material: SimMaterial,
}

/// A ship, put together: everything its modules bring, in ship coordinates.
#[derive(Debug)]
pub struct ShipDesign {
    /// Its name among designs: its directory under `assets/ships`.
    pub id: String,
    pub name: String,
    pub hostile: bool,
    pub model: Option<String>,
    pub shield: Shield,
    pub turrets: Vec<Vec3>,
    pub boxes: Vec<HullBox>,
    /// Buttons: (action, centre, half size), including every device a
    /// player presses (as [`ControlAction::Device`]).
    pub controls: Vec<(ControlAction, Vec3, Vec3)>,
    pub cabins: Vec<Cabin>,
    pub lights: Vec<Vec3>,
    pub posts: Vec<Post>,
    pub markers: HashMap<String, Vec3>,
    /// Devices (see [`super::devices`]), and the order they're evaluated
    /// in (each after whatever its conditions read).
    pub devices: Vec<Device>,
    pub device_order: Vec<u16>,
}

impl ShipDesign {
    /// The index of the device called `id`.
    pub fn device_index(&self, id: &str) -> Option<usize> {
        self.devices.iter().position(|d| d.id == id)
    }

    /// The device called `id`; every design a scenario uses has the
    /// devices it reads.
    pub fn device(&self, id: &str) -> usize {
        self.device_index(id)
            .unwrap_or_else(|| panic!("ship design {:?} has no device {id:?}", self.id))
    }

    /// A named point; every design that's used for something has the
    /// markers that use needs (see [`check`]).
    pub fn marker(&self, name: &str) -> Vec3 {
        *self
            .markers
            .get(name)
            .unwrap_or_else(|| panic!("ship design {:?} has no marker {name:?}", self.id))
    }
}

static DESIGNS: LazyLock<HashMap<String, ShipDesign>> = LazyLock::new(|| {
    let files: HashMap<&str, &str> = SHIP_FILES.iter().copied().collect();
    files
        .iter()
        .filter_map(|(path, text)| Some((path.strip_suffix("/design")?, *text)))
        .map(|(id, text)| {
            let design = assemble(id, text, &files).unwrap_or_else(|e| panic!("ship design {id:?}: {e}"));
            (id.to_string(), design)
        })
        .collect()
});

/// The design called `id`.
pub fn design(id: &str) -> &'static ShipDesign {
    DESIGNS.get(id).unwrap_or_else(|| panic!("no ship design {id:?} (assets/ships/{id}/design.ron)"))
}

/// Every design, by id.
pub fn designs() -> impl Iterator<Item = &'static ShipDesign> {
    let mut all: Vec<_> = DESIGNS.values().collect();
    all.sort_by(|a, b| a.id.cmp(&b.id));
    all.into_iter()
}

pub(crate) fn assemble(id: &str, text: &str, files: &HashMap<&str, &str>) -> Result<ShipDesign, String> {
    let file: DesignFile = ron::from_str(text).map_err(|e| format!("design.ron: {e}"))?;
    let mut ship = ShipDesign {
        id: id.to_string(),
        name: file.name,
        hostile: file.hostile,
        model: file.model,
        shield: file.shield,
        turrets: file.turrets.clone(),
        boxes: Vec::new(),
        controls: Vec::new(),
        cabins: Vec::new(),
        lights: Vec::new(),
        posts: Vec::new(),
        markers: HashMap::new(),
        devices: Vec::new(),
        device_order: Vec::new(),
    };
    let mut conds = Vec::new();
    let hull = if file.hostile { SimMaterial::HostileHull } else { SimMaterial::Hull };
    for place in &file.modules {
        let text = files.get(place.module.as_str()).ok_or_else(|| format!("no module {:?}", place.module))?;
        let module: ModuleFile = ron::from_str(text).map_err(|e| format!("{}.ron: {e}", place.module))?;
        let turn = Quat::from_rotation_y(place.turn.to_radians());
        let to_ship = |p: Vec3| place.at + turn * p;
        // An axis-aligned half size, turned, as the axis-aligned box round it.
        let turn_extent = |h: Vec3| Mat3::from_quat(turn).abs() * h;
        for part in &module.parts {
            for b in expand(part) {
                ship.boxes.push(HullBox {
                    center: to_ship(b.center),
                    size: b.size,
                    rotation: turn * b.rotation,
                    material: match b.material {
                        SimMaterial::Hull => hull,
                        other => other,
                    },
                });
            }
        }
        for c in &module.controls {
            ship.controls.push((c.action, to_ship(c.center), turn_extent(c.half_size)));
        }
        for c in &module.cabins {
            ship.cabins.push(Cabin {
                center: to_ship(c.center),
                size: turn_extent(c.size),
            });
        }
        ship.lights.extend(module.lights.iter().map(|&l| to_ship(l)));
        for p in &module.posts {
            ship.posts.push(Post {
                at: to_ship(p.at),
                facing: p.facing + place.turn,
                ..*p
            });
        }
        for (name, &at) in &module.markers {
            if ship.markers.insert(name.clone(), to_ship(at)).is_some() {
                return Err(format!("marker {name:?} placed twice"));
            }
        }
        for d in &module.devices {
            let (device, rows) = super::devices::place(d, &to_ship, turn, &turn_extent);
            conds.push((d.power.clone(), rows));
            ship.devices.push(device);
        }
    }
    ship.device_order = super::devices::wire(&mut ship.devices, conds).map_err(|e| format!("devices: {e}"))?;
    for (i, device) in ship.devices.iter().enumerate() {
        if device.pressable() {
            ship.controls.push((ControlAction::Device(i as u16), device.at, device.press_box()));
        }
        // A gate's frame is structure: something to grapple and to hit.
        for (center, size, rotation) in device.gate_frame() {
            ship.boxes.push(HullBox {
                center,
                size,
                rotation,
                material: SimMaterial::Trim,
            });
        }
    }
    // Turret bases, where the turrets are mounted.
    for &mount in &file.turrets {
        ship.boxes.push(HullBox {
            center: mount - Vec3::Y * mount.y.signum() * 0.25,
            size: Vec3::new(1.0, 0.5, 1.0),
            rotation: Quat::IDENTITY,
            material: SimMaterial::Trim,
        });
    }
    Ok(ship)
}

/// The boxes a part is made of, in its module's frame.
fn expand(part: &Part) -> Vec<HullBox> {
    let paint = |m: Material| match m {
        Material::Hull => SimMaterial::Hull,
        Material::Trim => SimMaterial::Trim,
    };
    let span = |lo: Vec3, hi: Vec3, material: SimMaterial| HullBox {
        center: (lo + hi) / 2.0,
        size: hi - lo,
        rotation: Quat::IDENTITY,
        material,
    };
    match *part {
        Part::Box { center, size, material } => vec![HullBox {
            center,
            size,
            rotation: Quat::IDENTITY,
            material: paint(material),
        }],
        Part::Span { lo, hi, material } => vec![span(lo, hi, paint(material))],
        Part::WallAcross { z, x, y, thickness, ref doors } => wall_across(z, x, y, thickness, doors),
        Part::WallAlong { x, z, y, thickness, ref doors } => {
            // A wall across, turned: swap x and z.
            wall_across(x, z, y, thickness, doors)
                .into_iter()
                .map(|mut b| {
                    b.center = Vec3::new(b.center.z, b.center.y, b.center.x);
                    b.size = Vec3::new(b.size.z, b.size.y, b.size.x);
                    b
                })
                .collect()
        }
        Part::Ramp { foot, top, width, thickness } => {
            // Tipped up from the horizontal by its slope, then turned to
            // face its way up.
            let run = Vec2::new(top.x - foot.x, top.z - foot.z);
            let rise = top.y - foot.y;
            let slope = (rise / run.length()).atan();
            let along = (run.length_squared() + rise * rise).sqrt();
            let heading = (-run.x).atan2(-run.y);
            let rotation = Quat::from_rotation_y(heading) * Quat::from_rotation_x(slope);
            let normal = rotation * Vec3::Y;
            vec![HullBox {
                center: (foot + top) / 2.0 - normal * (thickness / 2.0),
                size: Vec3::new(width, thickness, along),
                rotation,
                material: SimMaterial::Hull,
            }]
        }
    }
}

/// A wall across (in x, at `z`), from `x0` to `x1` and `y0` to `y1`, `t`
/// thick, with doorways: (from x, to x, top of the opening).
fn wall_across(z: f32, (x0, x1): (f32, f32), (y0, y1): (f32, f32), t: f32, doors: &[(f32, f32, f32)]) -> Vec<HullBox> {
    let span = |lo: Vec3, hi: Vec3| HullBox {
        center: (lo + hi) / 2.0,
        size: hi - lo,
        rotation: Quat::IDENTITY,
        material: SimMaterial::Hull,
    };
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

/// What play needs of a design, and where it falls short: one line per
/// shortfall (empty when it passes). See docs/ships.md.
pub fn check(ship: &ShipDesign) -> Vec<String> {
    let mut problems = Vec::new();
    // A site with nothing to walk to (a course of gates) has no floor plan
    // to check, and may be far too big for one.
    let walked = ship.hostile || !ship.posts.is_empty() || !ship.controls.is_empty() || ship.markers.contains_key("entry");
    if !walked {
        return problems;
    }
    let grid = walk_grid(ship);
    // Robots stand where a walker can, and every walker's post is reachable
    // from wherever boarders come in; flyers hold station clear of the hull.
    let entry = ship.markers.get("entry").copied();
    let entry_node = entry.and_then(|e| grid.standing_on(e));
    let reachable = |p: Vec3| entry.is_some_and(|e| grid.path(e, p).is_some_and(|path| path.last().is_some_and(|end| near(*end, p, 0.8))));
    for (i, post) in ship.posts.iter().enumerate() {
        match post.robot.gait() {
            Gait::Walker => {
                let on = grid.standing_on(post.at).map(|n| grid.point(n));
                if !on.is_some_and(|p| near(p, post.at, 0.6)) {
                    problems.push(format!("post {i} ({:?} at {}): not a place a walker can stand", post.robot, post.at));
                } else if entry_node.is_some() && !reachable(post.at) {
                    problems.push(format!("post {i} ({:?} at {}): can't be walked to from the entry", post.robot, post.at));
                }
            }
            Gait::Flyer => {
                if ship.boxes.iter().any(|b| inside(b, post.at, 1.0)) {
                    problems.push(format!("post {i} ({:?} at {}): inside the hull", post.robot, post.at));
                }
            }
            Gait::Mount | Gait::Crawler => {
                if bots::mount(ship, post.at).is_none() {
                    problems.push(format!("post {i} ({:?} at {}): not on an open hull surface", post.robot, post.at));
                }
            }
        }
    }
    if ship.hostile {
        // A boarding target: a way in, and something to do there that can
        // be reached on foot.
        match entry {
            None => problems.push("hostile, but no \"entry\" marker (where boarders come in)".into()),
            Some(e) if entry_node.is_none() => problems.push(format!("the entry ({e}) isn't a place a walker can stand")),
            _ => {}
        }
        match ship.markers.get("objective") {
            None => problems.push("hostile, but no \"objective\" marker".into()),
            Some(_) if !ship.controls.iter().any(|(a, ..)| *a == ControlAction::DisableShields) => {
                problems.push("hostile, but no DisableShields control".into())
            }
            _ => {}
        }
    }
    // Every control within reach of somewhere to stand (that the boarders
    // can get to, on an enemy).
    for (action, center, half) in &ship.controls {
        let standing = grid.places_near(*center, CONTROL_REACH + half.max_element());
        let usable = standing.iter().any(|&p| !ship.hostile || entry_node.is_none() || reachable(p));
        if !usable {
            problems.push(format!("control {action:?} at {center}: nowhere to stand within reach"));
        }
    }
    // Loot crates (`loot…` markers) stand on a floor the boarders can walk to.
    let mut loot: Vec<_> = ship.markers.iter().filter(|(name, _)| name.starts_with("loot")).collect();
    loot.sort_by(|a, b| a.0.cmp(b.0));
    for (name, &at) in loot {
        let on = grid.standing_on(at).map(|n| grid.point(n));
        if !on.is_some_and(|p| near(p, at, 0.6)) {
            problems.push(format!("loot marker {name:?} at {at}: not on a floor"));
        } else if entry_node.is_some() && !reachable(at) {
            problems.push(format!("loot marker {name:?} at {at}: can't be walked to from the entry"));
        }
    }
    // A site to explore (with its doors open): its objective can be walked
    // to from its entry.
    if !ship.hostile
        && let (Some(e), Some(&o)) = (entry, ship.markers.get("objective"))
        && !reachable(o)
    {
        problems.push(format!("the objective ({o}) can't be walked to from the entry ({e}), even with every door open"));
    }
    // Cable runs along surfaces, a few centimetres off them (or through
    // them, as through a bulkhead), never across open space.
    for d in &ship.devices {
        let super::devices::DeviceKind::Conduit { path } = &d.kind else { continue };
        for leg in path.windows(2) {
            let steps = (leg[0].distance(leg[1]) / 0.25).ceil().max(1.0) as usize;
            let off = (0..=steps)
                .map(|k| leg[0].lerp(leg[1], k as f32 / steps as f32))
                .find(|&p| !ship.boxes.iter().any(|b| surface_distance(b, p) <= CABLE_REACH));
            if let Some(p) = off {
                problems.push(format!("conduit {:?}: the run from {} to {} leaves the surfaces at {p}", d.id, leg[0], leg[1]));
                break;
            }
        }
    }
    if ship.id == super::ship::HOME {
        // The crew's ship: the mission wall and the locker.
        for kind in ScenarioKind::ALL {
            if !ship.controls.iter().any(|(a, ..)| *a == ControlAction::Scenario(kind)) {
                problems.push(format!("no button for scenario {kind:?}"));
            }
        }
    }
    problems
}

/// The walking grid a crew can make of a design: every bridge out, every
/// door open (doors aren't boxes). Robots' own grids are the design's as
/// built, so they never cross a bridge.
pub fn walk_grid(ship: &ShipDesign) -> NavGrid {
    let mut boxes = ship.boxes.clone();
    boxes.extend(ship.devices.iter().filter_map(|d| match d.kind {
        super::devices::DeviceKind::Bridge { size } => Some(HullBox {
            center: d.at,
            size,
            rotation: Quat::IDENTITY,
            material: SimMaterial::Hull,
        }),
        _ => None,
    }));
    NavGrid::for_boxes(&boxes)
}

/// How far from where they stand a player can press a control (m, from
/// their feet to the button's middle).
const CONTROL_REACH: f32 = 2.0;

fn near(a: Vec3, b: Vec3, across: f32) -> bool {
    Vec2::new(a.x - b.x, a.z - b.z).length() < across && (a.y - b.y).abs() < 0.5
}

/// How far a cable may run off a surface (m).
const CABLE_REACH: f32 = 0.12;

/// How far `p` is from box `b` (0 inside it).
fn surface_distance(b: &HullBox, p: Vec3) -> f32 {
    let local = b.rotation.inverse() * (p - b.center);
    (local.abs() - b.size / 2.0).max(Vec3::ZERO).length()
}

/// Whether `p` is inside box `b` grown by `margin`.
fn inside(b: &HullBox, p: Vec3, margin: f32) -> bool {
    let local = b.rotation.inverse() * (p - b.center);
    (local.abs() - b.size / 2.0).max_element() < margin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_design_meets_what_play_needs_of_it() {
        let mut failures = Vec::new();
        for ship in designs() {
            for problem in check(ship) {
                failures.push(format!("{}: {problem}", ship.id));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert!(designs().count() >= 2);
    }

    #[test]
    fn the_check_finds_posts_nobody_can_reach_and_a_missing_objective() {
        // A 10 m deck, open above, with a sealed 3 m closet in one corner.
        let files = HashMap::from([(
            "m",
            "Module(parts: [ \
               Span(lo: (-5.0, -0.3, -5.0), hi: (5.0, 0.0, 5.0)), \
               WallAcross(z: 1.0, x: (1.0, 5.0), y: (0.0, 3.0), thickness: 0.2), \
               WallAlong(x: 1.0, z: (1.0, 5.0), y: (0.0, 3.0), thickness: 0.2), \
             ], \
             posts: [ \
               (robot: Gunner, at: (-3.0, 0.0, -3.0)), \
               (robot: Gunner, at: (3.0, 0.0, 3.0)), \
               (robot: Gunner, at: (0.0, 2.0, 0.0)), \
               (robot: Swarmer, at: (0.0, -0.1, 0.0)), \
               (robot: Turret, at: (0.0, 5.0, 0.0)), \
             ], \
             markers: {\"entry\": (-4.0, 0.0, 0.0)})",
        )]);
        let text = "Design(name: \"T\", hostile: true, model: None, \
                    shield: (center: (0.0, 0.0, 0.0), radii: (1.0, 1.0, 1.0)), turrets: [], \
                    modules: [(module: \"m\")])";
        let problems = check(&assemble("t", text, &files).unwrap());
        let has = |what: &str| problems.iter().any(|p| p.contains(what));
        assert!(has("post 1 (Gunner at [3, 0, 3]): can't be walked to"), "{problems:#?}");
        assert!(has("post 2 (Gunner at [0, 2, 0]): not a place"), "{problems:#?}");
        assert!(has("post 3 (Swarmer"), "{problems:#?}");
        assert!(has("post 4 (Turret"), "{problems:#?}");
        assert!(has("no \"objective\" marker"), "{problems:#?}");
        assert!(!problems.iter().any(|p| p.contains("post 0")), "{problems:#?}");
    }

    #[test]
    fn the_check_finds_cable_run_across_open_space() {
        // A deck with a wall; one cable along the wall's foot, one up into
        // the air.
        let files = HashMap::from([(
            "m",
            "Module(parts: [ \
               Span(lo: (-5.0, -0.3, -5.0), hi: (5.0, 0.0, 5.0)), \
               WallAcross(z: 0.0, x: (-5.0, 5.0), y: (0.0, 3.0), thickness: 0.2), \
             ], \
             controls: [(action: Lurch, center: (0.0, 1.0, 0.2), half_size: (0.1, 0.1, 0.1))], \
             devices: [ \
               (id: \"along\", kind: Conduit(path: [(-4.0, 0.03, 0.15), (4.0, 0.03, 0.15)])), \
               (id: \"astray\", kind: Conduit(path: [(0.0, 0.03, 2.0), (0.0, 2.0, 2.0)])), \
             ])",
        )]);
        let text = "Design(name: \"T\", hostile: false, model: None, \
                    shield: (center: (0.0, 0.0, 0.0), radii: (1.0, 1.0, 1.0)), turrets: [], \
                    modules: [(module: \"m\")])";
        let problems = check(&assemble("t", text, &files).unwrap());
        assert!(problems.iter().any(|p| p.contains("conduit \"astray\"")), "{problems:#?}");
        assert!(!problems.iter().any(|p| p.contains("conduit \"along\"")), "{problems:#?}");
    }

    #[test]
    fn a_turned_module_turns_everything_in_it() {
        let files = HashMap::from([(
            "m",
            "Module(parts: [Box(center: (1.0, 0.0, 0.0), size: (2.0, 1.0, 4.0))], \
             controls: [(action: Lurch, center: (1.0, 0.0, 0.0), half_size: (0.1, 0.2, 0.3))], \
             markers: {\"x\": (0.0, 0.0, -1.0)})",
        )]);
        let text = "Design(name: \"T\", hostile: false, model: None, \
                    shield: (center: (0.0, 0.0, 0.0), radii: (1.0, 1.0, 1.0)), turrets: [], \
                    modules: [(module: \"m\", at: (10.0, 0.0, 0.0), turn: 90.0)])";
        let ship = assemble("t", text, &files).unwrap();
        let b = ship.boxes[0];
        // A quarter turn counterclockwise from above takes +X to −Z.
        assert!(b.center.distance(Vec3::new(10.0, 0.0, -1.0)) < 1e-5, "{}", b.center);
        assert!((b.rotation * Vec3::X).distance(Vec3::NEG_Z) < 1e-5);
        assert!(ship.controls[0].2.distance(Vec3::new(0.3, 0.2, 0.1)) < 1e-5);
        assert!(ship.marker("x").distance(Vec3::new(9.0, 0.0, 0.0)) < 1e-5);
    }
}
