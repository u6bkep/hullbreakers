//! Where a walker can go aboard a ship: a grid of standing places baked from
//! the hull's layout, and paths through it.
//!
//! A walker is a capsule on mag-boots that keeps to floors (robots don't
//! walk walls). The grid samples the ship's floor plan every [`CELL`]
//! metres, in the ship's frame: wherever a vertical line through the layout
//! leaves a solid top with room above it for the walker to stand, and no
//! wall within its radius, is a place it can stand. Neighbouring places
//! join where the step between them is small (a ramp, a sill), so decks
//! join where a ramp joins them. Paths are A* over the places, then pulled
//! straight wherever a straight walk stays on them.
//!
//! The grid comes from the same boxes the collider is built from
//! ([`super::design::ShipDesign::boxes`]), so it is exactly the ship that's
//! simulated.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use bevy::prelude::*;

use super::design::{ShipDesign, design};

/// Grid spacing (m).
pub const CELL: f32 = 0.5;
/// The walker the grid is baked for: the biggest that walks (a Gunner),
/// with a little room to spare.
const RADIUS: f32 = 0.42;
const HEIGHT: f32 = 2.0;
/// The most a floor may rise from one place to the next (m): sills and
/// ramps, not crates.
const STEP: f32 = 0.3;
/// Within this of the floor, solid things don't count as in the way (a
/// lip, the slope of a ramp under the walker's edge).
const FOOT: f32 = 0.35;
/// How far below a goal its floor may be (m): under a deck's height.
const GOAL_DROP: f32 = 2.6;
/// How far the floor between two linked places may stray from a straight
/// line between them (m).
const SMOOTH: f32 = 0.1;
/// How much of its radius out a walker needs floor under it.
const SUPPORT: f32 = 0.6;

/// A place to stand: the cell it's in, and its floor's height.
#[derive(Clone, Copy, Debug)]
struct Node {
    i: i32,
    k: i32,
    y: f32,
}

/// The standing places aboard one kind of hull, in its frame.
pub struct NavGrid {
    /// The x and z of cell (0, 0)'s corner.
    origin: Vec2,
    size: (i32, i32),
    nodes: Vec<Node>,
    /// Each column's nodes, lowest first: `columns[i + k * size.0]`.
    columns: Vec<Vec<u32>>,
    links: Vec<Vec<u32>>,
    /// Which connected region each node is in.
    region: Vec<u32>,
}

/// An oriented box: centre, half size, and the rotation to the ship.
struct Solid {
    center: Vec3,
    half: Vec3,
    to_local: Quat,
    /// Its extent in x and z, for skipping it quickly.
    lo: Vec2,
    hi: Vec2,
}

impl Solid {
    /// Where the vertical line through (x, z) is inside it: a y interval.
    fn column(&self, x: f32, z: f32) -> Option<(f32, f32)> {
        if x < self.lo.x || x > self.hi.x || z < self.lo.y || z > self.hi.y {
            return None;
        }
        let origin = self.to_local * (Vec3::new(x, 0.0, z) - self.center);
        let dir = self.to_local * Vec3::Y;
        let (mut t0, mut t1) = (f32::NEG_INFINITY, f32::INFINITY);
        for axis in 0..3 {
            let (o, d, h) = (origin[axis], dir[axis], self.half[axis]);
            if d.abs() < 1e-6 {
                if o.abs() > h {
                    return None;
                }
            } else {
                let (a, b) = ((-h - o) / d, (h - o) / d);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        (t0 < t1).then_some((t0, t1))
    }
}

/// The y intervals that are solid along the vertical line through (x, z),
/// merged and in order.
fn solid_column(solids: &[Solid], x: f32, z: f32) -> Vec<(f32, f32)> {
    let mut spans: Vec<(f32, f32)> = solids.iter().filter_map(|s| s.column(x, z)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for (a, b) in spans {
        match merged.last_mut() {
            Some(last) if a <= last.1 + 0.02 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// The floor at `p`: the top of the solid under the vertical line through
/// it that's nearest its height.
fn surface(solids: &[Solid], p: Vec3) -> Option<f32> {
    solid_column(solids, p.x, p.z)
        .into_iter()
        .map(|(_, top)| top)
        .filter(|top| (top - p.y).abs() <= STEP)
        .min_by(|a, b| (a - p.y).abs().total_cmp(&(b - p.y).abs()))
}

/// Whether anything solid is along the vertical line through (x, z)
/// between `y0` and `y1`.
fn blocked(solids: &[Solid], x: f32, z: f32, y0: f32, y1: f32) -> bool {
    solids.iter().any(|s| s.column(x, z).is_some_and(|(a, b)| a < y1 && b > y0))
}

/// The eight ways to a neighbouring cell, and the cost of each.
const AROUND: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)];

impl NavGrid {
    pub fn for_design(ship: &ShipDesign) -> NavGrid {
        NavGrid::for_boxes(&ship.boxes)
    }

    /// The grid over these boxes (a design's, and perhaps some of its
    /// devices').
    pub fn for_boxes(boxes: &[super::design::HullBox]) -> NavGrid {
        let solids: Vec<Solid> = boxes
            .iter()
            .map(|b| {
                let (center, size, rotation) = (b.center, b.size, b.rotation);
                let half = size / 2.0;
                // The box's reach in x and z, whatever its turn.
                let m = Mat3::from_quat(rotation);
                let reach = m.x_axis.abs() * half.x + m.y_axis.abs() * half.y + m.z_axis.abs() * half.z;
                Solid {
                    center,
                    half,
                    to_local: rotation.inverse(),
                    lo: Vec2::new(center.x - reach.x, center.z - reach.z),
                    hi: Vec2::new(center.x + reach.x, center.z + reach.z),
                }
            })
            .collect();
        let lo = solids.iter().fold(Vec2::splat(f32::MAX), |l, s| l.min(s.lo));
        let hi = solids.iter().fold(Vec2::splat(f32::MIN), |h, s| h.max(s.hi));
        // Cell corners on the half metre, so cell centres fall between the
        // layout's (mostly round) edges rather than on them.
        let origin = (lo / CELL).floor() * CELL;
        let size = (((hi.x - origin.x) / CELL).ceil() as i32, ((hi.y - origin.y) / CELL).ceil() as i32);
        let mut grid = NavGrid {
            origin,
            size,
            nodes: Vec::new(),
            columns: vec![Vec::new(); (size.0 * size.1) as usize],
            links: Vec::new(),
            region: Vec::new(),
        };
        // Places: solid tops with room above, and no wall within reach.
        let around: Vec<Vec2> = (0..8)
            .map(|a| {
                let angle = a as f32 * std::f32::consts::FRAC_PI_4;
                Vec2::new(angle.cos(), angle.sin()) * RADIUS
            })
            .collect();
        for k in 0..size.1 {
            for i in 0..size.0 {
                let c = grid.center(i, k);
                let spans = solid_column(&solids, c.x, c.y);
                for (n, &(_, top)) in spans.iter().enumerate() {
                    let ceiling = spans.get(n + 1).map_or(f32::INFINITY, |s| s.0);
                    if ceiling - top < HEIGHT {
                        continue;
                    }
                    let clear = around
                        .iter()
                        .all(|o| !blocked(&solids, c.x + o.x, c.y + o.y, top + FOOT, top + HEIGHT));
                    // Not at the brink of a drop (a ramp's edge): floor
                    // most of the way out to its radius.
                    let supported = around.iter().all(|o| {
                        let o = *o * SUPPORT;
                        blocked(&solids, c.x + o.x, c.y + o.y, top - FOOT, top - 0.01)
                    });
                    if clear && supported {
                        let id = grid.nodes.len() as u32;
                        grid.nodes.push(Node { i, k, y: top });
                        grid.columns[(i + k * size.0) as usize].push(id);
                    }
                }
            }
        }
        // Links: to a neighbouring place a small step up or down, over
        // floor that runs smoothly between them (not off a ramp's side,
        // which is a ledge the boots can't climb). A diagonal needs both
        // cells it cuts between.
        let smooth = |a: Vec3, b: Vec3| {
            (1..4).all(|q| {
                let p = a.lerp(b, q as f32 / 4.0);
                surface(&solids, p).is_some_and(|y| (y - p.y).abs() <= SMOOTH)
            })
        };
        grid.links = (0..grid.nodes.len())
            .map(|id| {
                let n = grid.nodes[id];
                let here = grid.point(id as u32);
                AROUND
                    .iter()
                    .filter_map(|&(di, dk)| {
                        let to = grid.level(n.i + di, n.k + dk, n.y)?;
                        let diagonal = di != 0 && dk != 0;
                        if diagonal && (grid.level(n.i + di, n.k, n.y).is_none() || grid.level(n.i, n.k + dk, n.y).is_none()) {
                            return None;
                        }
                        smooth(here, grid.point(to)).then_some(to)
                    })
                    .collect()
            })
            .collect();
        // Only both ways: a link one way only (its diagonal's side cells
        // judged from one end's height) would make regions lie.
        let both: Vec<Vec<u32>> = (0..grid.links.len())
            .map(|n| grid.links[n].iter().copied().filter(|&m| grid.links[m as usize].contains(&(n as u32))).collect())
            .collect();
        grid.links = both;
        // Regions: who can reach whom.
        grid.region = vec![u32::MAX; grid.nodes.len()];
        let mut next = 0;
        for start in 0..grid.nodes.len() {
            if grid.region[start] != u32::MAX {
                continue;
            }
            let mut stack = vec![start as u32];
            grid.region[start] = next;
            while let Some(n) = stack.pop() {
                for &m in &grid.links[n as usize] {
                    if grid.region[m as usize] == u32::MAX {
                        grid.region[m as usize] = next;
                        stack.push(m);
                    }
                }
            }
            next += 1;
        }
        grid
    }

    fn center(&self, i: i32, k: i32) -> Vec2 {
        self.origin + (Vec2::new(i as f32, k as f32) + 0.5) * CELL
    }

    fn cell_of(&self, p: Vec3) -> (i32, i32) {
        let c = (Vec2::new(p.x, p.z) - self.origin) / CELL;
        (c.x.floor() as i32, c.y.floor() as i32)
    }

    fn column(&self, i: i32, k: i32) -> &[u32] {
        if i < 0 || k < 0 || i >= self.size.0 || k >= self.size.1 {
            return &[];
        }
        &self.columns[(i + k * self.size.0) as usize]
    }

    /// The place in cell (i, k) within a step of height `y`.
    fn level(&self, i: i32, k: i32, y: f32) -> Option<u32> {
        self.column(i, k).iter().copied().find(|&n| (self.nodes[n as usize].y - y).abs() <= STEP)
    }

    /// Where a node's walker stands.
    pub fn point(&self, node: u32) -> Vec3 {
        let n = self.nodes[node as usize];
        let c = self.center(n.i, n.k);
        Vec3::new(c.x, n.y, c.y)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// The place a walker with its feet at `feet` is standing on: in its
    /// cell, or the nearest one about (it may stand closer to a wall than
    /// the grid allows).
    pub fn standing_on(&self, feet: Vec3) -> Option<u32> {
        self.floor_under(feet, 1.0)
    }

    /// The place under `p` (at most `drop` below it, or a step above), in
    /// its cell or the nearest one about.
    fn floor_under(&self, p: Vec3, drop: f32) -> Option<u32> {
        let feet = p;
        let (ci, ck) = self.cell_of(feet);
        let mut best: Option<(f32, u32)> = None;
        for r in 0..=4 {
            for k in ck - r..=ck + r {
                for i in ci - r..=ci + r {
                    if (i - ci).abs().max((k - ck).abs()) != r {
                        continue;
                    }
                    for &n in self.column(i, k) {
                        let p = self.point(n);
                        // A floor below the feet, or barely above them.
                        if p.y > feet.y + STEP || p.y < feet.y - drop {
                            continue;
                        }
                        let d = p.distance_squared(feet);
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, n));
                        }
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        best.map(|(_, n)| n)
    }

    /// A floor plan of the places with floors between heights `y0` and
    /// `y1`, bow at the top, port on the left, one character a cell: `.`
    /// where a walker can get from `from`, `,` places it can't, and `marks`
    /// (points and their letters) over them.
    pub fn plan(&self, y0: f32, y1: f32, from: Option<Vec3>, marks: &[(Vec3, char)]) -> String {
        let region = from.and_then(|f| self.standing_on(f)).map(|n| self.region[n as usize]);
        let mut rows: Vec<Vec<char>> = (0..self.size.1)
            .map(|k| {
                (0..self.size.0)
                    .map(|i| {
                        let here = self.column(i, k).iter().find(|&&n| (y0..y1).contains(&self.nodes[n as usize].y));
                        match here {
                            None => ' ',
                            Some(&n) if Some(self.region[n as usize]) == region => '.',
                            Some(_) => ',',
                        }
                    })
                    .collect()
            })
            .collect();
        for &(p, c) in marks {
            let (i, k) = self.cell_of(p);
            if (y0..y1).contains(&p.y) && i >= 0 && k >= 0 && i < self.size.0 && k < self.size.1 {
                rows[k as usize][i as usize] = c;
            }
        }
        let mut out = String::new();
        for row in rows {
            out.extend(row.iter());
            out.push('\n');
        }
        out
    }

    /// Where a walker could stand within `reach` of `p` (m).
    pub fn places_near(&self, p: Vec3, reach: f32) -> Vec<Vec3> {
        let (ci, ck) = self.cell_of(p);
        let r = (reach / CELL).ceil() as i32 + 1;
        let mut out = Vec::new();
        for k in ck - r..=ck + r {
            for i in ci - r..=ci + r {
                out.extend(self.column(i, k).iter().map(|&n| self.point(n)).filter(|q| q.distance(p) <= reach));
            }
        }
        out
    }

    /// The node in `region` nearest `p`, anywhere: searched outward a ring
    /// of cells at a time, until no further ring could hold a nearer one.
    fn nearest_in(&self, region: u32, p: Vec3) -> Option<u32> {
        let (ci, ck) = self.cell_of(p);
        let mut best: Option<(f32, u32)> = None;
        let rings = self.size.0.max(self.size.1) + ci.abs().max(ck.abs());
        for r in 0..=rings {
            // Everything in ring r is at least (r − 1) cells away across.
            if best.is_some_and(|(d, _)| ((r - 1) as f32 * CELL).powi(2) > d) {
                break;
            }
            for k in ck - r..=ck + r {
                // The ring's edge: whole rows top and bottom, the two ends
                // between.
                let edge = (k - ck).abs() == r;
                let row: Vec<i32> = if edge { (ci - r..=ci + r).collect() } else { vec![ci - r, ci + r] };
                for i in row {
                    for &n in self.column(i, k) {
                        if self.region[n as usize] != region {
                            continue;
                        }
                        let d = self.point(n).distance_squared(p);
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, n));
                        }
                    }
                }
            }
        }
        best.map(|(_, n)| n)
    }

    /// A path from where a walker stands (`feet`) toward `goal`, as points
    /// to walk through in turn, the last at or as near `goal` as can be
    /// reached. `None` if the walker isn't on the grid at all.
    pub fn path(&self, feet: Vec3, goal: Vec3) -> Option<Vec<Vec3>> {
        let start = self.standing_on(feet)?;
        let region = self.region[start as usize];
        // The goal's own place, if it's on one we can reach; otherwise the
        // nearest we can.
        // (The goal may be a body's middle, or someone jetpacking: the floor
        // under it, within a deck's height.)
        let end = self
            .floor_under(goal, GOAL_DROP)
            .filter(|&n| self.region[n as usize] == region)
            .or_else(|| self.nearest_in(region, goal))?;
        let nodes = self.astar(start, end)?;
        Some(self.pull(&nodes))
    }

    fn astar(&self, start: u32, end: u32) -> Option<Vec<u32>> {
        #[derive(PartialEq)]
        struct Open(f32, u32);
        impl Eq for Open {}
        impl Ord for Open {
            fn cmp(&self, other: &Self) -> Ordering {
                other.0.total_cmp(&self.0).then(self.1.cmp(&other.1))
            }
        }
        impl PartialOrd for Open {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.cmp(other))
            }
        }
        let goal = self.point(end);
        let mut cost: HashMap<u32, f32> = HashMap::from([(start, 0.0)]);
        let mut from: HashMap<u32, u32> = HashMap::new();
        let mut open = BinaryHeap::from([Open(self.point(start).distance(goal), start)]);
        while let Some(Open(_, n)) = open.pop() {
            if n == end {
                let mut path = vec![n];
                let mut at = n;
                while let Some(&prev) = from.get(&at) {
                    path.push(prev);
                    at = prev;
                }
                path.reverse();
                return Some(path);
            }
            let here = self.point(n);
            let c = cost[&n];
            for &m in &self.links[n as usize] {
                let there = self.point(m);
                let next = c + here.distance(there);
                if cost.get(&m).is_none_or(|&old| next < old) {
                    cost.insert(m, next);
                    from.insert(m, n);
                    open.push(Open(next + there.distance(goal), m));
                }
            }
        }
        None
    }

    /// Whether a straight walk from `a` to `b` stays on places, a step at a
    /// time.
    pub fn walkable(&self, a: Vec3, b: Vec3) -> bool {
        let (i, k) = self.cell_of(a);
        let Some(mut at) = self.level(i, k, a.y) else {
            return false;
        };
        let steps = (Vec2::new(b.x - a.x, b.z - a.z).length() / (CELL * 0.5)).ceil().max(1.0) as i32;
        for s in 1..=steps {
            let p = a.lerp(b, s as f32 / steps as f32);
            let (i, k) = self.cell_of(p);
            let n = self.nodes[at as usize];
            if (n.i, n.k) == (i, k) {
                continue;
            }
            // Into the next cell over, by a link.
            match self.links[at as usize].iter().copied().find(|&m| {
                let m = self.nodes[m as usize];
                (m.i, m.k) == (i, k)
            }) {
                Some(next) => at = next,
                None => return false,
            }
        }
        // On the same floor as `b` (not under or over it).
        (self.nodes[at as usize].y - b.y).abs() <= STEP
    }

    /// A path through `nodes`, with every corner it can cut cut.
    fn pull(&self, nodes: &[u32]) -> Vec<Vec3> {
        let points: Vec<Vec3> = nodes.iter().map(|&n| self.point(n)).collect();
        let mut out = Vec::new();
        let mut at = 0;
        while at + 1 < points.len() {
            // The furthest point straight on from here.
            let mut next = at + 1;
            for j in (at + 2..points.len()).rev() {
                if self.walkable(points[at], points[j]) {
                    next = j;
                    break;
                }
            }
            out.push(points[next]);
            at = next;
        }
        if out.is_empty() {
            out.extend(points.last());
        }
        out
    }
}

/// Authority: each ship design's grid, baked the first time a walker
/// needs it, for each state of its doors and bridges it's needed in (which
/// of its devices are solid just now: shut doors block the way, extended
/// bridges are floor; see `devices::DoorsBuilt`). A state is baked once and
/// kept, so a door opening and shutting costs one bake each way.
#[derive(Resource, Default)]
pub struct NavGrids(HashMap<(String, Vec<bool>), NavGrid>);

impl NavGrids {
    /// The grid of design `id` with the devices marked in `solid` (by
    /// index, as `DoorsBuilt` has them) in its way or underfoot; `None` for
    /// the design as built.
    pub fn get(&mut self, id: &str, solid: Option<&[bool]>) -> &NavGrid {
        let solid = solid.map(|s| s.to_vec()).unwrap_or_default();
        self.0.entry((id.to_string(), solid)).or_insert_with_key(|(id, solid)| {
            let ship = design(id);
            let mut boxes = ship.boxes.clone();
            boxes.extend(ship.devices.iter().zip(solid).filter(|(_, s)| **s).filter_map(|(d, _)| match d.kind {
                super::devices::DeviceKind::Door { size, .. } | super::devices::DeviceKind::Bridge { size } => Some(super::design::HullBox {
                    center: d.at,
                    size,
                    rotation: Quat::IDENTITY,
                    material: super::SimMaterial::Hull,
                }),
                _ => None,
            }));
            let grid = NavGrid::for_boxes(&boxes);
            info!("nav: {id} has {} standing places (with {} devices in place)", grid.len(), solid.iter().filter(|s| **s).count());
            grid
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carrier() -> NavGrid {
        NavGrid::for_design(design("carrier"))
    }

    #[test]
    fn the_bridge_is_reached_from_the_hangar_by_the_ramp() {
        let grid = carrier();
        let ship = design("carrier");
        let upper = ship.marker("upper_deck").y;
        let start = ship.marker("entry") + Vec3::new(0.0, 0.0, -2.0);
        let goal = Vec3::new(0.0, upper, ship.marker("objective").z + 1.5);
        let path = grid.path(start, goal).expect("on the grid");
        let end = *path.last().unwrap();
        assert!(end.distance(goal) < 0.8, "ends at {end}, not by the console");
        // It goes by the ramp, up the port side of the hangar.
        assert!(path.iter().any(|p| p.x < -3.5 && p.z > 15.0 && p.y < 1.0), "{path:?}");
        assert!(end.y > upper - 0.1);
        // Every leg is a straight walk on the grid.
        let mut from = grid.point(grid.standing_on(start).unwrap());
        for &p in &path {
            assert!(grid.walkable(from, p), "{from} -> {p}");
            from = p;
        }
    }

    #[test]
    fn doorways_are_passable_and_walls_are_not() {
        let grid = carrier();
        // Lower corridor to a side room through its door (z −7 to −5.4).
        let path = grid.path(Vec3::new(0.0, 0.0, -6.0), Vec3::new(-4.0, 0.0, -10.0)).unwrap();
        assert!(path.last().unwrap().distance(Vec3::new(-4.0, 0.0, -10.0)) < 0.8);
        // Through the wall it can't go straight.
        assert!(!grid.walkable(Vec3::new(0.0, 0.0, -10.0), Vec3::new(-4.0, 0.0, -10.0)));
        // The side rooms are split at z = 0: aft to fore of the port rooms
        // goes out through the corridor.
        let path = grid.path(Vec3::new(-4.0, 0.0, 3.0), Vec3::new(-4.0, 0.0, -3.0)).unwrap();
        assert!(path.iter().any(|p| p.x > -2.0), "{path:?}");
    }

    #[test]
    fn crates_are_in_the_way() {
        let grid = carrier();
        // The crate at (3, 0.75, 20), 1.5 m on a side.
        assert!(grid.standing_on(Vec3::new(3.0, 0.0, 20.0)).is_none_or(|n| grid.point(n).distance(Vec3::new(3.0, 0.0, 20.0)) > 0.9));
        assert!(!grid.walkable(Vec3::new(3.0, 0.0, 22.5), Vec3::new(3.0, 0.0, 17.5)));
    }
}

