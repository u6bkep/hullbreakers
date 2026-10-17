//! Scenes: a place the crew jumps into, laid out from one seed.
//!
//! A scene is sites (designs set at rest round our ship, each with
//! something to do: an [`Encounter`]), loot adrift between them, and a rock
//! field grown round it all. Everything is a pure function of a
//! [`SceneSeed`] ([`plan`]): the authority builds the plan's world
//! (`scenarios::scene`), and every peer grows the same rocks from the same
//! seed ([`rock_places`]), as the race's field is grown. Nothing of the
//! layout is sent.
//!
//! What can be in a scene is whatever designs declare a [`SceneUse`]
//! (`scene:` in `design.ron`; see docs/scenes.md): the planner chooses and
//! places; the designs (and the agents who build them) decide what a site
//! is and how its puzzle is wired. The scene's size is one of its levers: a
//! tight scene is a couple of sites a short flight apart; a vast one is a
//! long way across a big field.

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::asteroids::{RockPlace, random_unit};
use super::design::{ShipDesign, designs};
use super::items::CargoKind;

/// How far a scene spreads: the distance to its sites, how many there are,
/// how big its field is, and how long the crew has to get aboard once the
/// main objective is done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SceneSize {
    /// A couple of sites within a hundred metres: a short, dense scene.
    Tight,
    /// Sites a few hundred metres out: rope and jetpack flights between.
    #[default]
    Wide,
    /// Sites a kilometre and more out, across a big field: an expedition.
    Vast,
}

impl SceneSize {
    pub const ALL: [SceneSize; 3] = [SceneSize::Tight, SceneSize::Wide, SceneSize::Vast];

    pub fn id(self) -> &'static str {
        match self {
            SceneSize::Tight => "tight",
            SceneSize::Wide => "wide",
            SceneSize::Vast => "vast",
        }
    }

    pub fn from_id(id: &str) -> Option<SceneSize> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// How far from our ship (m) a site is set.
    pub fn reach(self) -> (f32, f32) {
        match self {
            SceneSize::Tight => (60.0, 110.0),
            SceneSize::Wide => (200.0, 450.0),
            SceneSize::Vast => (800.0, 1600.0),
        }
    }

    /// How many sites, at most (as many as the catalogue allows).
    pub fn sites(self) -> (usize, usize) {
        match self {
            SceneSize::Tight => (1, 2),
            SceneSize::Wide => (2, 3),
            SceneSize::Vast => (3, 4),
        }
    }

    /// Rocks in its field.
    pub fn rocks(self) -> usize {
        match self {
            SceneSize::Tight => 400,
            SceneSize::Wide => 1500,
            SceneSize::Vast => 5000,
        }
    }

    /// Loot adrift between the sites.
    pub fn drifting(self) -> usize {
        match self {
            SceneSize::Tight => 2,
            SceneSize::Wide => 4,
            SceneSize::Vast => 7,
        }
    }

    /// Seconds to be aboard once the main objective is done: the jump
    /// window. A flight home from the far site and some.
    pub fn window(self) -> f32 {
        match self {
            SceneSize::Tight => 90.0,
            SceneSize::Wide => 180.0,
            SceneSize::Vast => 360.0,
        }
    }
}

/// What a scene is made from: the seed and the size. Everything else is
/// derived ([`plan`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SceneSeed {
    pub seed: u64,
    pub size: SceneSize,
}

impl SceneSeed {
    /// A scene of `seed`, its size rolled from it too.
    pub fn roll(seed: u64) -> SceneSeed {
        let mut rng = ChaCha8Rng::seed_from_u64(seed ^ 0x51ce_0000);
        let size = SceneSize::ALL[rng.random_range(0..SceneSize::ALL.len())];
        SceneSeed { seed, size }
    }

    /// `HULLBREAKERS_SCENE`'s spelling: `<seed>` or `<seed>:<size>`.
    pub fn parse(text: &str) -> Result<SceneSeed, String> {
        let (seed, size) = match text.split_once(':') {
            Some((seed, size)) => (seed, Some(size)),
            None => (text, None),
        };
        let seed: u64 = seed.trim().parse().map_err(|_| format!("scene seed {seed:?} isn't a number"))?;
        Ok(match size {
            None => SceneSeed::roll(seed),
            Some(size) => SceneSeed {
                seed,
                size: SceneSize::from_id(size.trim()).ok_or_else(|| {
                    let known: Vec<&str> = SceneSize::ALL.iter().map(|s| s.id()).collect();
                    format!("unknown scene size {size:?} ({})", known.join(", "))
                })?,
            },
        })
    }

    pub fn name(self) -> String {
        format!("Scene {} ({})", self.seed, self.size.id())
    }
}

/// Something to do at a site. A design says which of these it can host
/// (its [`SceneUse`]); the planner picks one per site.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Encounter {
    /// Bring a piece of cargo home. It lies at the design's `objective`
    /// marker from the start, or appears there once the device
    /// `released_by` is live (a clamp, a vault's lock).
    Salvage {
        cargo: CargoKind,
        released_by: Option<String>,
    },
    /// Bring the device called `device` live (restart a reactor, route the
    /// power, open the way).
    Restore { device: String },
    /// Clear the site of its robots.
    Clear,
    /// Survivors adrift round the site (a wreck), on their air: tow them
    /// home. `pods` of them.
    Rescue { pods: u8 },
}

impl Encounter {
    /// What it's called on the board.
    pub fn label(&self, site: &str) -> String {
        match self {
            Encounter::Salvage { cargo, .. } => format!("{}: {}", site, cargo.name().to_lowercase()),
            Encounter::Restore { device } => format!("{site}: restore {device}"),
            Encounter::Clear => format!("{site}: clear it"),
            Encounter::Rescue { pods } => format!("{site}: {pods} adrift"),
        }
    }

    /// Whether it's a prize worth making a scene's main objective.
    pub fn is_prize(&self) -> bool {
        matches!(self, Encounter::Salvage { .. } | Encounter::Rescue { .. })
    }
}

/// What a design offers a scene: `scene:` in its `design.ron`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename = "Site", deny_unknown_fields)]
pub struct SceneUse {
    /// The encounters it can host; the planner picks one.
    pub encounters: Vec<Encounter>,
    /// Robots stand at its posts (at the scene's threat).
    #[serde(default)]
    pub crewed: bool,
    /// How far (m) from its origin rocks and other sites keep. Default:
    /// its hull's reach and some.
    #[serde(default)]
    pub clearance: Option<f32>,
}

/// A site in a plan.
#[derive(Clone, Debug, PartialEq)]
pub struct SitePlan {
    /// Its design's id.
    pub design: &'static str,
    /// What the board calls it (its design's name, numbered if two share
    /// one).
    pub label: String,
    pub at: Vec3,
    pub turn: Quat,
    pub encounter: Encounter,
    pub crewed: bool,
    /// Rocks and other sites keep this far (m) from `at`.
    pub clearance: f32,
}

/// Loot adrift in the open.
#[derive(Clone, Debug, PartialEq)]
pub struct DriftPlan {
    pub at: Vec3,
    pub velocity: Vec3,
    /// How well its crate rolls (0-100).
    pub luck: u32,
}

/// A scene, laid out: what [`plan`] makes of a seed.
#[derive(Clone, Debug, PartialEq)]
pub struct ScenePlan {
    pub seed: SceneSeed,
    pub sites: Vec<SitePlan>,
    /// Which site's encounter is the main objective; the rest are sides.
    pub main: usize,
    pub drifting: Vec<DriftPlan>,
    /// How dangerous crewed sites are (1-3; see `bots::Threat`).
    pub threat: u8,
    /// Seconds to be aboard once the main is done.
    pub window: f32,
}

impl ScenePlan {
    pub fn site_name(&self, i: usize) -> &str {
        &self.sites[i].label
    }
}

/// Our ship sits at the origin; nothing is set within this of it.
pub const HOME_CLEARANCE: f32 = 60.0;

/// The designs a scene can use, in a fixed order (by id), with what they
/// offer.
pub fn catalogue() -> Vec<(&'static ShipDesign, &'static SceneUse)> {
    designs().filter_map(|d| Some((d, d.scene.as_ref()?))).collect()
}

/// A design's reach from its origin (m): the farthest corner of its hull.
pub fn hull_reach(design: &ShipDesign) -> f32 {
    design
        .boxes
        .iter()
        .map(|b| b.center.length() + b.size.length() / 2.0)
        .fold(0.0, f32::max)
}

/// The scene of a seed: the same on every peer and every run.
pub fn plan(seed: SceneSeed) -> ScenePlan {
    let mut rng = ChaCha8Rng::seed_from_u64(seed.seed);
    let size = seed.size;
    let catalogue = catalogue();
    // Which sites, and what to do at each: a prize for the main, then
    // sides from the rest of the catalogue (each design once).
    let (least, most) = size.sites();
    let count = rng.random_range(least..=most).min(catalogue.len());
    let mut order: Vec<usize> = (0..catalogue.len()).collect();
    shuffle(&mut rng, &mut order);
    // The main site first: one with a prize to offer, if any has one.
    if let Some(k) = order.iter().position(|&i| catalogue[i].1.encounters.iter().any(Encounter::is_prize)) {
        order.swap(0, k);
    }
    let mut sites: Vec<SitePlan> = Vec::new();
    for (n, &i) in order.iter().take(count).enumerate() {
        let (design, use_) = catalogue[i];
        let encounter = if n == 0 {
            let prizes: Vec<&Encounter> = use_.encounters.iter().filter(|e| e.is_prize()).collect();
            if prizes.is_empty() {
                use_.encounters[rng.random_range(0..use_.encounters.len())].clone()
            } else {
                prizes[rng.random_range(0..prizes.len())].clone()
            }
        } else {
            use_.encounters[rng.random_range(0..use_.encounters.len())].clone()
        };
        let clearance = use_.clearance.unwrap_or(hull_reach(design) + 25.0);
        // Placed: at the size's reach, clear of the others. The nearer the
        // main, the better, so it's placed first and the sides fan out.
        let (lo, hi) = size.reach();
        let mut at = Vec3::ZERO;
        for _ in 0..200 {
            let dir = random_unit(&mut rng);
            // Mostly round us, not above and below: a flat-ish scene reads
            // from the deck.
            let dir = Vec3::new(dir.x, dir.y * 0.35, dir.z).normalize_or(Vec3::Z);
            at = dir * rng.random_range(lo..hi);
            let clear = sites.iter().all(|s| s.at.distance(at) >= s.clearance + clearance + 40.0);
            if clear {
                break;
            }
        }
        // Turned so its entry faces us: the way in is the side we see.
        let turn = match design.markers.get("entry") {
            Some(entry) if entry.xz().length() > 1.0 => {
                let out = Vec3::new(entry.x, 0.0, entry.z).normalize();
                let home = Vec3::new(-at.x, 0.0, -at.z).normalize_or(Vec3::Z);
                Quat::from_rotation_arc(out, home)
            }
            _ => Quat::from_rotation_y(rng.random_range(0.0..std::f32::consts::TAU)),
        };
        let label = {
            let same = sites.iter().filter(|s| s.design == design.id).count();
            if same == 0 { design.name.clone() } else { format!("{} {}", design.name, same + 1) }
        };
        sites.push(SitePlan {
            design: &design.id,
            label,
            at,
            turn,
            encounter,
            crewed: use_.crewed,
            clearance,
        });
    }
    // Loot adrift: along the way to the sites, off the line, and a few out
    // past them.
    let mut drifting = Vec::new();
    for k in 0..size.drifting() {
        let site = &sites[k % sites.len().max(1)];
        let along = if sites.is_empty() { Vec3::Z * 100.0 } else { site.at * rng.random_range(0.3..1.3) };
        let off = random_unit(&mut rng) * rng.random_range(25.0..80.0);
        let at = along + off;
        if at.length() < HOME_CLEARANCE || sites.iter().any(|s| s.at.distance(at) < s.clearance) {
            continue;
        }
        drifting.push(DriftPlan {
            at,
            velocity: random_unit(&mut rng) * rng.random_range(0.0..0.3),
            // Farther out, better.
            luck: ((at.length() / size.reach().1) * 80.0).clamp(10.0, 90.0) as u32,
        });
    }
    ScenePlan {
        seed,
        sites,
        main: 0,
        drifting,
        threat: rng.random_range(1..=3),
        window: size.window(),
    }
}

fn shuffle(rng: &mut ChaCha8Rng, order: &mut [usize]) {
    for i in (1..order.len()).rev() {
        let j = rng.random_range(0..=i);
        order.swap(i, j);
    }
}

/// Rocks keep this far from what's in the scene: the site's clearance and
/// the rock's reach (a rope's swing round it).
const REACH: f32 = 2.5;

/// Where a scene's `count` rocks are: handholds strung along the way out to
/// each site (something to reel by every few seconds), a cluster round
/// each site (cover on the approach, and a wall it sits in), and the rest
/// scattered through the scene's box. All still, knockable, clear of the
/// sites, of our ship, of the loot adrift and of each other.
pub fn rock_places(seed: SceneSeed, count: usize) -> Vec<RockPlace> {
    let plan = plan(seed);
    let mut rng = ChaCha8Rng::seed_from_u64(seed.seed ^ 0xc0c5);
    let mut placed = Placed::default();
    let keep_out = |at: Vec3, radius: f32| {
        at.length() >= HOME_CLEARANCE + radius * REACH
            && plan.sites.iter().all(|s| s.at.distance(at) >= s.clearance + radius * REACH)
            && plan.drifting.iter().all(|d| d.at.distance(at) >= 12.0 + radius)
    };
    // Handholds: every 35 m along each leg, off to a side.
    for site in &plan.sites {
        let d = site.at;
        let (side, up) = d.normalize_or(Vec3::Z).any_orthonormal_pair();
        let steps = (d.length() / 35.0) as usize;
        for i in 1..steps {
            if rng.random::<f32>() < 0.3 {
                continue;
            }
            let angle = rng.random_range(0.0..std::f32::consts::TAU);
            let off = rng.random_range(18.0..70.0);
            let radius = rng.random_range(1.5..6.0);
            let at = d * (i as f32 / steps as f32) + (side * angle.cos() + up * angle.sin()) * off;
            if placed.rocks.len() < count && keep_out(at, radius) && placed.clear(at, radius) {
                placed.push(at, radius);
            }
        }
    }
    // A cluster round each site.
    for site in &plan.sites {
        let around = ((count / 5) / plan.sites.len().max(1)).max(8);
        for _ in 0..around * 10 {
            if placed.rocks.len() >= count {
                break;
            }
            let radius = 1.0 * 7.0f32.powf(rng.random::<f32>().powi(2));
            let at = site.at + random_unit(&mut rng) * rng.random_range(site.clearance + 10.0..site.clearance + 120.0);
            if keep_out(at, radius) && placed.clear(at, radius) {
                placed.push(at, radius);
                if placed.rocks.len() >= around * (plan.sites.len()) + count / 10 {
                    break;
                }
            }
        }
    }
    // The rest, through the box round everything and its margin.
    let reach = seed.size.reach().1 + 150.0;
    let (lo, hi) = (Vec3::new(-reach, -reach * 0.5, -reach), Vec3::new(reach, reach * 0.5, reach));
    for _ in 0..count * 30 {
        if placed.rocks.len() >= count {
            break;
        }
        let radius = 1.0 * 9.0f32.powf(rng.random::<f32>().powi(2));
        let at = Vec3::new(rng.random_range(lo.x..hi.x), rng.random_range(lo.y..hi.y), rng.random_range(lo.z..hi.z));
        if keep_out(at, radius) && placed.clear(at, radius) {
            placed.push(at, radius);
        }
    }
    placed.rocks.into_iter().map(|(at, radius)| RockPlace::still(at, radius, false)).collect()
}

/// Rocks placed so far, in a grid of cells for the clearance test.
#[derive(Default)]
struct Placed {
    rocks: Vec<(Vec3, f32)>,
    cells: std::collections::HashMap<IVec3, Vec<usize>>,
}

const CELL: f32 = 50.0;

impl Placed {
    fn cell(at: Vec3) -> IVec3 {
        (at / CELL).floor().as_ivec3()
    }

    fn clear(&self, at: Vec3, radius: f32) -> bool {
        let c = Self::cell(at);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = self.cells.get(&(c + IVec3::new(dx, dy, dz))) else { continue };
                    for &i in list {
                        let (p, r) = self.rocks[i];
                        if p.distance(at) < (r + radius) * 1.6 + 4.0 {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    fn push(&mut self, at: Vec3, radius: f32) {
        self.cells.entry(Self::cell(at)).or_default().push(self.rocks.len());
        self.rocks.push((at, radius));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_always_plans_the_same_scene() {
        for seed in [0u64, 1, 42, 0xdead_beef] {
            let s = SceneSeed::roll(seed);
            assert_eq!(plan(s), plan(s));
            assert_eq!(rock_places(s, 300), rock_places(s, 300));
        }
    }

    #[test]
    fn every_size_lays_out_clear_sites_and_rocks() {
        for size in SceneSize::ALL {
            for seed in 0..12u64 {
                let s = SceneSeed { seed, size };
                let p = plan(s);
                assert!(!p.sites.is_empty(), "{s:?}: no sites");
                assert!(p.sites.len() <= size.sites().1);
                let (lo, hi) = size.reach();
                for (i, a) in p.sites.iter().enumerate() {
                    let d = a.at.length();
                    assert!(d >= lo - 1.0 && d <= hi + 1.0, "{s:?}: site {i} at {d} m, not in {lo}..{hi}");
                    for b in &p.sites[i + 1..] {
                        assert!(a.at.distance(b.at) >= a.clearance + b.clearance, "{s:?}: sites overlap");
                    }
                }
                assert!(p.sites[p.main].encounter.is_prize() || !catalogue().iter().any(|(_, u)| u.encounters.iter().any(Encounter::is_prize)));
                let rocks = rock_places(s, 400);
                assert!(rocks.len() >= 300, "{s:?}: only {} rocks placed", rocks.len());
                for r in &rocks {
                    assert!(r.at.length() >= HOME_CLEARANCE, "{s:?}: a rock on our ship");
                    for site in &p.sites {
                        assert!(site.at.distance(r.at) >= site.clearance, "{s:?}: a rock in {}", site.label);
                    }
                }
            }
        }
    }

    #[test]
    fn sites_face_our_ship() {
        for seed in 0..20u64 {
            let p = plan(SceneSeed::roll(seed));
            for site in &p.sites {
                let design = super::super::design::design(site.design);
                let Some(entry) = design.markers.get("entry") else { continue };
                if entry.xz().length() < 1.0 {
                    continue;
                }
                let entry_world = site.at + site.turn * *entry;
                // The entry is nearer us than the site's origin is.
                assert!(entry_world.length() < site.at.length(), "{seed}: {}'s entry faces away", site.label);
            }
        }
    }

    #[test]
    fn seeds_are_spelt_with_a_size() {
        assert_eq!(SceneSeed::parse("7:vast").unwrap(), SceneSeed { seed: 7, size: SceneSize::Vast });
        assert_eq!(SceneSeed::parse("7").unwrap().seed, 7);
        assert!(SceneSeed::parse("x").is_err());
        assert!(SceneSeed::parse("7:huge").is_err());
    }
}
