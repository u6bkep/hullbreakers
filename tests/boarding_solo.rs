//! The boarding action for a lone player, robots and all: what a boarder
//! crossing on a rope meets.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::bots::Robot;
use hullbreakers::sim::combat::Beams;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::mission::{Director, ScenarioKind};
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::ship::ShipHull;
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::vitals::{HurtLog, Vitals};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, AssetPlugin::default(), bevy::mesh::MeshPlugin));
    app.add_plugins(PhysicsPlugins::default());
    app.insert_resource(Tuning::default());
    app.add_plugins(SimPlugin {
        asteroids: false,
        local_player: true,
    });
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)));
    app.finish();
    app.cleanup();
    ticks(&mut app, 4);
    app
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn me(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, (With<Player>, Without<Robot>)>().single(world).unwrap()
}

fn ship(app: &mut App, design: &str) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ShipHull)>()
        .iter(world)
        .find(|(_, h)| h.0 == design)
        .map(|(e, _)| e)
        .unwrap()
}

fn input(app: &mut App) -> Mut<'_, PlayerInput> {
    let e = me(app);
    app.world_mut().get_mut::<PlayerInput>(e).unwrap()
}

/// At the carrier's first close pass, a lone boarder floats out of our
/// hold's door, hooks the carrier and reels across, and gets there alive
/// (with the full crew aboard, they were dead in 4 s: six swarmers and three
/// hull turrets). Prints every hurt and what it came from (a robot by kind,
/// or a beam), and where they got to.
#[test]
fn alone_a_rope_crossing_at_the_first_pass_gets_there() {
    let mut app = app();
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Boarding);
    ticks(&mut app, 2);
    let (home, carrier) = (ship(&mut app, "home"), ship(&mut app, "carrier"));
    // Out by the door just before the pass.
    ticks(&mut app, 24 * 60);
    let at = |app: &mut App, e: Entity| (app.world().get::<Position>(e).unwrap().0, app.world().get::<Rotation>(e).unwrap().0);
    let (hp, hr) = at(&mut app, home);
    let home_v = app.world().get::<LinearVelocity>(home).unwrap().0;
    let door = hp + hr * Vec3::new(0.0, 1.6, 18.0);
    let (cp, cr) = at(&mut app, carrier);
    let aim = cp + cr * Vec3::new(0.0, 1.5, 10.0);
    let e = me(&mut app);
    {
        let mut p = app.world_mut().get_mut::<Player>(e).unwrap();
        p.mode = Mode::Eva {
            position: door,
            velocity: home_v,
            orientation: Transform::IDENTITY.looking_to(aim - door, Vec3::Y).rotation,
            pitch: 0.0,
        };
        p.reference_body = Some(home);
    }
    eprintln!("carrier {:.0} m from our ship, {:.0} m from the door", cp.distance(hp), aim.distance(door));
    input(&mut app).presses.main.press();
    ticks(&mut app, 2);
    let g = app.world().get::<Player>(e).unwrap().grapple;
    eprintln!("hooked: {:?}", g.map(|g| (g.target == carrier, g.length)));
    input(&mut app).reel_in = true;
    let mut seen = app.world().get::<HurtLog>(e).unwrap().count;
    let (mut closest, mut lowest) = (f32::MAX, 100.0f32);
    for tick in 0..30 * 60 {
        ticks(&mut app, 1);
        let log = app.world().get::<HurtLog>(e).unwrap().clone();
        let p = app.world().get::<Player>(e).unwrap().clone();
        let health = app.world().get::<Vitals>(e).map_or(0.0, |v| v.health);
        let here = match p.mode {
            Mode::Eva { position, .. } => position,
            Mode::Booted { .. } => Vec3::NAN,
        };
        lowest = lowest.min(health);
        if here.is_finite() {
            closest = closest.min(here.distance(at(&mut app, carrier).0));
        }
        if log.count != seen {
            let new = (log.count - seen) as usize;
            seen = log.count;
            for hurt in log.recent.iter().rev().take(new) {
                let from = hurt.from.unwrap_or(Vec3::NAN);
                let world = app.world_mut();
                let robot = world
                    .query::<(&Robot, &Position)>()
                    .iter(world)
                    .map(|(r, p)| (*r, p.0.distance(from)))
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let beam = world.resource::<Beams>().0.iter().map(|b| b.from.distance(from)).fold(f32::MAX, f32::min);
                eprintln!(
                    "{:5.2} s: hurt {:.0}{} from {from:.1}: nearest robot {robot:?}, nearest beam turret {beam:.1} m; me at {here:.1}, {:.0} m from the carrier, health {health:.0}",
                    tick as f32 / 60.0,
                    hurt.amount,
                    if hurt.kill { " (kill)" } else { "" },
                    here.distance(at(&mut app, carrier).0),
                );
            }
        }
        if health <= 0.0 || !matches!(p.mode, Mode::Eva { .. }) || tick % 60 == 0 && p.grapple.is_none() {
            eprintln!("{:.2} s: stop: {:?}, grapple {:?}", tick as f32 / 60.0, p.mode, p.grapple.map(|g| g.length));
            break;
        }
        if tick % 120 == 0 {
            let local = match p.mode {
                Mode::Eva { position, .. } => {
                    let (cp, cr) = at(&mut app, carrier);
                    cr.inverse() * (position - cp)
                }
                _ => Vec3::NAN,
            };
            eprintln!("{:.0} s: {local:.1} in the carrier's frame, rope {:?}, health {health:.0}", tick as f32 / 60.0, p.grapple.map(|g| g.length));
        }
    }
    eprintln!("closest {closest:.0} m to the carrier's origin; health at lowest {lowest:.0}");
    assert!(lowest > 50.0, "a lone boarder was down to {lowest:.0} crossing");
    assert!(closest < 15.0, "the rope didn't bring them to the carrier ({closest:.0} m)");
}

/// The carrier's crew grows with ours, to the full complement at four.
#[test]
fn the_carrier_is_crewed_for_the_boarders() {
    use hullbreakers::sim::bots::{Threat, compose_crew};
    use hullbreakers::sim::design::design;
    use hullbreakers::sim::scenarios::boarding::{CARRIER, carrier_crew};
    let carrier = design(CARRIER);
    let count = |crew: &[hullbreakers::sim::bots::Posting], robot: Robot| crew.iter().filter(|p| p.robot == robot).count();
    let mut last = 0;
    for players in 1..=4 {
        let crew = carrier_crew(carrier, players, 7);
        eprintln!(
            "{players} players: {} robots ({} gunners, {} grapplers, {} swarmers, {} turrets, {} crawlers)",
            crew.len(),
            count(&crew, Robot::Gunner),
            count(&crew, Robot::Grappler),
            count(&crew, Robot::Swarmer),
            count(&crew, Robot::Turret),
            count(&crew, Robot::Crawler)
        );
        assert!(crew.len() > last, "{players} players face no more robots than {}", players - 1);
        last = crew.len();
    }
    let sorted = |crew: Vec<hullbreakers::sim::bots::Posting>| {
        let mut crew: Vec<String> = crew.iter().map(|p| format!("{p:?}")).collect();
        crew.sort();
        crew
    };
    assert_eq!(sorted(carrier_crew(carrier, 4, 7)), sorted(compose_crew(carrier, Threat(3), 7)), "four players face the full crew");
}

/// Alone at the top of the carrier's ramp and shooting, a boarder meets its
/// guards a post at a time: the corridor's comes, the bridge's hold the
/// bridge (on their leashes: see `bots::Leash`). Before, all three walked
/// out to the ramp head together. Counts the guards shooting at the
/// boarder at once, and the closest each comes.
#[test]
fn alone_at_the_ramp_head_the_guards_come_a_post_at_a_time() {
    use hullbreakers::sim::events::PlayerEvent;
    let mut app = app();
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Boarding);
    ticks(&mut app, 2);
    let carrier = ship(&mut app, "carrier");
    let upper = hullbreakers::sim::design::design("carrier").marker("upper_deck").y;
    let e = me(&mut app);
    let head = Vec3::new(0.0, upper + 0.01, 6.0);
    {
        let mut p = app.world_mut().get_mut::<Player>(e).unwrap();
        p.mode = Mode::Booted {
            body: carrier,
            feet: head,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
        p.loadout.restock();
        p.loadout.current = 0;
    }
    // Firing up the corridor, the whole time: heard all over the deck.
    input(&mut app).main.held = true;
    let mut closest: std::collections::HashMap<Entity, f32> = Default::default();
    let mut most_at_once = 0;
    for _ in 0..20 * 60 {
        app.world_mut().get_mut::<Vitals>(e).unwrap().health = 1000.0;
        ticks(&mut app, 1);
        let world = app.world_mut();
        let tick = world.resource::<hullbreakers::sim::SimTick>().0;
        let mut shooting = 0;
        for (r, kind, p) in world.query::<(Entity, &Robot, &Player)>().iter(world) {
            let Mode::Booted { body, feet, .. } = p.mode else { continue };
            if body != carrier || *kind != Robot::Gunner {
                continue;
            }
            let d = Vec2::new(feet.x - head.x, feet.z - head.z).length();
            let c = closest.entry(r).or_insert(f32::MAX);
            *c = c.min(d);
            if p.events.iter().any(|(at, ev)| matches!(ev, PlayerEvent::Fired { .. }) && tick.wrapping_sub(at) < 60) {
                shooting += 1;
            }
        }
        most_at_once = most_at_once.max(shooting);
    }
    let mut c: Vec<f32> = closest.values().copied().collect();
    c.sort_by(f32::total_cmp);
    eprintln!("ramp head, alone: {} guards; closest each came {c:.1?} m; at most {most_at_once} shooting at once", c.len());
    assert_eq!(c.len(), 3, "the solo crew is three walkers");
    assert!(c.iter().filter(|d| **d < 15.0).count() <= 1, "more than one guard came to the ramp head: {c:.1?}");
    assert!(most_at_once <= 2, "{most_at_once} guards shooting at once");
}
