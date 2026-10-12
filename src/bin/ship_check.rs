//! Checks ship designs against what play needs of them, and draws their
//! decks (see docs/ships.md):
//!
//!   cargo run --bin ship_check [design ...]     (default: every design)
//!
//! Exits non-zero if any design falls short.

use bevy::prelude::*;
use hullbreakers::sim::bots::Gait;
use hullbreakers::sim::devices::DeviceKind;
use hullbreakers::sim::design::{ShipDesign, check, design, designs};
use hullbreakers::sim::nav::NavGrid;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ships: Vec<&ShipDesign> = if args.is_empty() { designs().collect() } else { args.iter().map(|a| design(a)).collect() };
    let mut failed = false;
    for ship in ships {
        failed |= report(ship);
    }
    std::process::exit(if failed { 1 } else { 0 });
}

/// Prints one design's report; whether it fell short.
fn report(ship: &ShipDesign) -> bool {
    let grid = NavGrid::for_design(ship);
    println!("== {} ({}) ==", ship.id, ship.name);
    let (lo, hi) = ship.boxes.iter().fold((Vec3::MAX, Vec3::MIN), |(lo, hi), b| {
        let reach = Mat3::from_quat(b.rotation).abs() * (b.size / 2.0);
        (lo.min(b.center - reach), hi.max(b.center + reach))
    });
    let size = hi - lo;
    println!(
        "{} boxes, {:.1} wide x {:.1} tall x {:.1} long (m); {} standing places",
        ship.boxes.len(),
        size.x,
        size.y,
        size.z,
        grid.len()
    );
    let count = |gait: Gait| ship.posts.iter().filter(|p| p.robot.gait() == gait).count();
    println!(
        "posts: {} walkers, {} flyers, {} mounts; {} controls, {} lights",
        count(Gait::Walker),
        count(Gait::Flyer),
        count(Gait::Mount),
        ship.controls.len(),
        ship.lights.len()
    );
    let entry = ship.markers.get("entry").copied();
    if let (Some(entry), Some(objective)) = (entry, ship.markers.get("objective")) {
        match grid.path(entry, *objective) {
            Some(path) => {
                let mut length = 0.0;
                let mut at = entry;
                for p in &path {
                    length += at.distance(*p);
                    at = *p;
                }
                println!(
                    "entry to objective: {length:.0} m walked, {} legs ({:.0} s at a walk, 3 m/s)",
                    path.len(),
                    length / 3.0
                );
            }
            None => println!("entry to objective: no way"),
        }
    }
    // A plan of each deck: the floor heights there are places at.
    let mut floors: Vec<f32> = Vec::new();
    for b in &ship.boxes {
        let top = b.center.y + b.size.y / 2.0;
        if b.rotation == Quat::IDENTITY && b.size.x * b.size.z > 40.0 && !floors.iter().any(|f| (f - top).abs() < 1.0) {
            floors.push(top);
        }
    }
    floors.sort_by(f32::total_cmp);
    let mut marks: Vec<(Vec3, char)> = Vec::new();
    marks.extend(ship.lights.iter().map(|l| (*l - Vec3::Y * 2.5, '*')));
    marks.extend(ship.controls.iter().map(|c| (c.1 - Vec3::Y * 1.0, 'C')));
    // Doors (drawn open: the plan is with every door open).
    marks.extend(ship.devices.iter().filter(|d| matches!(d.kind, DeviceKind::Door { .. })).map(|d| (d.at - Vec3::Y * 1.1, 'D')));
    marks.extend(ship.posts.iter().filter(|p| p.robot.gait() == Gait::Walker).map(|p| (p.at, 'G')));
    for (name, at) in &ship.markers {
        let c = match name.as_str() {
            "entry" => 'E',
            "objective" => 'O',
            _ => continue,
        };
        marks.push((*at - Vec3::Y * 0.5, c));
    }
    println!("legend: . reachable from the entry, , not, G walker post, E entry, O objective, C control (or device), D door, * light; bow at the top, port on the left");
    for (i, &floor) in floors.iter().enumerate() {
        let next = floors.get(i + 1).copied().unwrap_or(f32::INFINITY);
        println!("-- floors from y = {floor:.2} --");
        print!("{}", grid.plan(floor - 0.5, next - 0.5, entry, &marks));
    }
    let problems = check(ship);
    if problems.is_empty() {
        println!("check: ok\n");
    } else {
        println!("check: {} problem(s)", problems.len());
        for p in &problems {
            println!("  - {p}");
        }
        println!();
    }
    !problems.is_empty()
}
