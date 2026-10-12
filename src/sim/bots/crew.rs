//! Crewing a ship: which robot stands at each of its design's posts, with
//! what affixes, for a threat level.
//!
//! A design's posts say what kind of robot the place is for: a walker's
//! post on a deck, a flyer's station off the hull, a mount on a hull
//! surface. [`compose_crew`] decides who's there, from the threat and a
//! seed:
//!
//! - **Threat 0** is the plain crew: Gunners and Swarmers, no affixes, and
//!   no one at the mounts.
//! - From **threat 1** the mounts are manned (Turrets), and some walkers'
//!   posts get a Grappler instead of a Gunner: one at threat 1–2, two at
//!   3–4, three from 5.
//! - From **threat 2** Crawlers come out on the hull: one at threat 2, and
//!   one more each level up, as far as the design has crawlers' posts.
//! - From **threat 2** a Leader stands at the walker's post deepest in the
//!   ship (furthest from the boarders' entry): the one guarding the
//!   objective.
//! - Each level of threat is two affixes, scattered over the crew (only
//!   ones a kind can have, never two that clash); a robot carries at most
//!   one below threat 4, two from there.

use std::f32::consts::PI;

use bevy::prelude::*;
use rand::seq::SliceRandom;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use super::{Affix, Affixes, Brain, Gait, Robot, new_robot};
use crate::sim::design::{Post, ShipDesign};
use crate::sim::flight::Flight;
use crate::sim::player::Mode;
use crate::sim::tuning::Tuning;

/// How dangerous a ship's crew is (see the module's doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Threat(pub u8);

/// A robot at a post.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Posting {
    pub robot: Robot,
    pub affixes: Affixes,
    pub post: Post,
}

/// Who stands at each of `design`'s posts at `threat`, rolled from `seed`
/// (the same seed always gives the same crew). Mounts left unmanned have no
/// posting.
pub fn compose_crew(design: &ShipDesign, threat: Threat, seed: u64) -> Vec<Posting> {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let t = threat.0;
    let crawlers = (t as usize).saturating_sub(1);
    let mut crew: Vec<Posting> = design
        .posts
        .iter()
        .filter(|post| post.robot.gait() != Gait::Mount || t >= 1)
        .filter({
            let mut seen = 0;
            move |post| {
                if post.robot.gait() != Gait::Crawler {
                    return true;
                }
                seen += 1;
                seen <= crawlers
            }
        })
        .map(|post| Posting {
            robot: post.robot,
            affixes: Affixes::NONE,
            post: *post,
        })
        .collect();
    let walkers: Vec<usize> = (0..crew.len()).filter(|&i| crew[i].robot.gait() == Gait::Walker).collect();
    // The leader guards the objective: the walker's post furthest from the
    // way in.
    let entry = design.markers.get("entry").copied().unwrap_or(Vec3::ZERO);
    let deepest = walkers
        .iter()
        .copied()
        .max_by(|&a, &b| crew[a].post.at.distance(entry).total_cmp(&crew[b].post.at.distance(entry)));
    if t >= 2
        && let Some(i) = deepest
    {
        crew[i].robot = Robot::Gunner;
        crew[i].affixes = Affixes::of(&[Affix::Leader]);
    }
    // Grapplers at some of the other walkers' posts.
    let grapplers = match t {
        0 => 0,
        1..=2 => 1,
        3..=4 => 2,
        _ => 3,
    };
    let mut others: Vec<usize> = walkers.iter().copied().filter(|&i| Some(i) != deepest || t < 2).collect();
    others.shuffle(&mut rng);
    for &i in others.iter().take(grapplers) {
        crew[i].robot = Robot::Grappler;
    }
    // Affixes, scattered.
    let most = if t >= 4 { 2 } else { 1 };
    let mut points = 2 * t as usize;
    let mut tries = 0;
    while points > 0 && tries < 200 {
        tries += 1;
        let i = rng.random_range(0..crew.len());
        let p = &mut crew[i];
        if p.affixes.len() >= most {
            continue;
        }
        let affix = Affix::ALL[rng.random_range(0..Affix::ALL.len())];
        // One leader to a ship.
        if affix == Affix::Leader || !p.affixes.takes(affix, p.robot) {
            continue;
        }
        p.affixes = p.affixes.with(affix);
        points -= 1;
    }
    crew
}

/// Puts a crew for `threat` at the posts of `design` aboard `ship` (flying
/// `flight`, at `tick`, which also seeds the roll): walkers standing on its
/// decks, flyers holding station about it, turrets on its hull.
pub fn crew_ship(commands: &mut Commands, tuning: &Tuning, ship: Entity, design: &ShipDesign, flight: &Flight, tick: u32, threat: Threat) {
    let frame = flight.frame(tick, Vec3::ZERO);
    for (i, posting) in compose_crew(design, threat, tick as u64).into_iter().enumerate() {
        let post = posting.post;
        let (mode, home_post) = match posting.robot.gait() {
            Gait::Walker => {
                let feet = post.at + Vec3::Y * 0.01;
                (booted(ship, feet, Vec3::Y, post.rotation()), feet)
            }
            Gait::Mount | Gait::Crawler => {
                let Some(up) = mount(design, post.at) else {
                    warn!("{}: post {i} ({:?} at {}) is on no hull surface", design.id, post.robot, post.at);
                    continue;
                };
                let feet = post.at + up * 0.01;
                (booted(ship, feet, up, post.rotation()), feet)
            }
            Gait::Flyer => {
                let position = frame.point_to_world(post.at);
                let mode = Mode::Eva {
                    position,
                    velocity: frame.point_velocity(position),
                    orientation: frame.rotation * post.rotation(),
                    pitch: 0.0,
                };
                (mode, post.at)
            }
        };
        if !posting.affixes.is_empty() || posting.robot != post.robot {
            info!("{}: {} at {}", design.id, posting.affixes.describe(posting.robot), post.at);
        }
        commands.spawn(new_robot(posting.robot, posting.affixes, mode, ship, home_post, tuning, i as u32));
    }
}

/// Standing on `ship` at `feet` (its frame), on a surface facing `up`,
/// turned as `facing` turns −Z (about +Y; tipped over onto `up`).
fn booted(ship: Entity, feet: Vec3, up: Vec3, facing: Quat) -> Mode {
    let tip = Quat::from_rotation_arc(Vec3::Y, up);
    Mode::Booted {
        body: ship,
        feet,
        up,
        velocity: Vec3::ZERO,
        orientation: tip * facing,
        pitch: 0.0,
    }
}

/// The outward normal of the open hull surface `at` lies on (within a few
/// centimetres), if it lies on one: where a turret can be bolted.
pub fn mount(design: &ShipDesign, at: Vec3) -> Option<Vec3> {
    const ON: f32 = 0.05;
    let inside = |p: Vec3| {
        design.boxes.iter().any(|b| {
            let local = b.rotation.inverse() * (p - b.center);
            (local.abs() - b.size / 2.0).max_element() < 0.0
        })
    };
    design.boxes.iter().find_map(|b| {
        let local = b.rotation.inverse() * (at - b.center);
        let out = local.abs() - b.size / 2.0;
        // Off its faces by no more than a little, and nearest one of them.
        if out.max_element() > ON {
            return None;
        }
        let axis = (0..3).max_by(|&i, &j| out[i].total_cmp(&out[j]))?;
        if out[axis] < -ON {
            return None;
        }
        let mut n = Vec3::ZERO;
        n[axis] = local[axis].signum();
        let normal = b.rotation * n;
        // Open to space (or a room) above it, not buried in the hull.
        (!inside(at + normal * 0.3)).then_some(normal)
    })
}

/// The gallery's robots, as a table on a wall facing the visitors: a row
/// per kind, its plain one first and then one with each affix it can have.
/// `at` is the table's middle (in `home`'s frame), and `turn` takes its
/// rows (along +X) and its face (+Z, toward the visitors) to where they
/// go. They're dormant: they hold still and don't fight back, but can be
/// shot (and their affixes work: shells, armour, blasts).
pub fn spawn_zoo(commands: &mut Commands, tuning: &Tuning, home: Entity, at: Vec3, turn: Quat) {
    let rows: Vec<Vec<Affixes>> = Robot::ALL
        .iter()
        .map(|kind| {
            std::iter::once(Affixes::NONE)
                .chain(Affix::ALL.into_iter().filter(|a| a.fits(*kind)).map(|a| Affixes::of(&[a])))
                .collect()
        })
        .collect();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(1);
    let mut i = 0;
    for (r, (kind, row)) in Robot::ALL.into_iter().zip(&rows).enumerate() {
        for (c, affixes) in row.iter().copied().enumerate() {
            i += 1;
            let place = Vec3::new(
                (c as f32 - (columns as f32 - 1.0) / 2.0) * ZOO_COLUMN,
                ((rows.len() as f32 - 1.0) / 2.0 - r as f32) * ZOO_ROW,
                0.0,
            );
            let post = at + turn * place;
            // Facing the visitors, turned a little so they show a side.
            let mode = Mode::Eva {
                position: post,
                velocity: Vec3::ZERO,
                orientation: turn * Quat::from_rotation_y(PI * 0.85),
                pitch: 0.0,
            };
            let mut robot = commands.spawn(new_robot(kind, affixes, mode, home, post, tuning, i as u32));
            robot.insert(Name::new(affixes.describe(kind)));
            robot.queue(|mut entity: EntityWorldMut| {
                if let Some(mut brain) = entity.get_mut::<Brain>() {
                    brain.dormant = true;
                }
            });
        }
    }
}

/// The gallery's robot table: room per column and per row (m).
const ZOO_COLUMN: f32 = 3.0;
const ZOO_ROW: f32 = 2.8;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::design::design;

    #[test]
    fn a_crew_is_rolled_the_same_from_the_same_seed() {
        let carrier = design("carrier");
        assert_eq!(compose_crew(carrier, Threat(3), 7), compose_crew(carrier, Threat(3), 7));
        assert_ne!(compose_crew(carrier, Threat(3), 7), compose_crew(carrier, Threat(3), 8));
    }

    #[test]
    fn threat_zero_is_the_plain_crew() {
        let carrier = design("carrier");
        let crew = compose_crew(carrier, Threat(0), 1);
        assert!(crew.iter().all(|p| p.affixes.is_empty() && p.robot == p.post.robot));
        assert!(crew.iter().all(|p| p.robot.gait() != Gait::Mount), "turrets at threat 0");
        assert!(crew.iter().all(|p| p.robot != Robot::Crawler), "crawlers at threat 0");
    }

    #[test]
    fn threat_brings_affixes_grapplers_turrets_and_a_leader_by_the_objective() {
        let carrier = design("carrier");
        for seed in 0..20 {
            let crew = compose_crew(carrier, Threat(3), seed);
            let affixes: usize = crew.iter().map(|p| p.affixes.len()).sum();
            // Six scattered, and the leader.
            assert_eq!(affixes, 7, "seed {seed}: {crew:#?}");
            assert!(crew.iter().all(|p| p.affixes.len() <= 1));
            assert!(crew.iter().all(|p| p.affixes.iter().all(|a| a.fits(p.robot))));
            assert_eq!(crew.iter().filter(|p| p.robot == Robot::Grappler).count(), 2);
            // Two crawlers out on the hull at threat 3.
            assert_eq!(crew.iter().filter(|p| p.robot == Robot::Crawler).count(), 2);
            assert!(crew.iter().any(|p| p.robot == Robot::Turret));
            let leaders: Vec<_> = crew.iter().filter(|p| p.affixes.has(Affix::Leader)).collect();
            assert_eq!(leaders.len(), 1);
            // On the bridge, where the shield console is.
            assert!(leaders[0].post.at.z < -22.0, "leader at {}", leaders[0].post.at);
        }
        let crew = compose_crew(carrier, Threat(5), 3);
        assert!(crew.iter().any(|p| p.affixes.len() == 2), "{crew:#?}");
    }

    #[test]
    fn mounts_are_found_on_open_hull_faces() {
        let carrier = design("carrier");
        // On the roof, facing up; under the keel, facing down.
        assert_eq!(mount(carrier, Vec3::new(3.0, 6.6, 26.0)), Some(Vec3::Y));
        assert_eq!(mount(carrier, Vec3::new(0.0, -0.3, 20.0)), Some(Vec3::NEG_Y));
        // In mid-air; inside the hull's skin; on the upper deck's floor
        // (open to the room above it, so it is a surface).
        assert_eq!(mount(carrier, Vec3::new(0.0, 12.0, 0.0)), None);
        assert_eq!(mount(carrier, Vec3::new(0.0, -0.15, 0.0)), None);
        assert_eq!(mount(carrier, Vec3::new(0.0, 3.3, 0.0)), Some(Vec3::Y));
    }
}
