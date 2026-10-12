//! Robots: they see the crew, fight them, and die.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::bots::{Affixes, Robot, new_robot};
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::ship::HomeShip;
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::vitals::Vitals;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
    ));
    app.add_plugins(PhysicsPlugins::default());
    // ROBOT_TUNING=path: run with saved tuning (default: code defaults).
    let tuning = std::env::var("ROBOT_TUNING")
        .map(|p| Tuning::load_or_default(std::path::Path::new(&p)))
        .unwrap_or_default();
    app.insert_resource(tuning);
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

fn ship(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap()
}

fn crew(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, (With<PlayerInput>, Without<Robot>)>()
        .single(world)
        .unwrap()
}

fn health(app: &App, e: Entity) -> f32 {
    app.world().get::<Vitals>(e).unwrap().health
}

fn spawn(app: &mut App, kind: Robot, mode: Mode, post: Vec3) -> Entity {
    let ship = ship(app);
    let tuning = Tuning::default();
    let robot = app.world_mut().spawn(new_robot(kind, Affixes::NONE, mode, ship, post, &tuning, 0)).id();
    ticks(app, 1);
    robot
}

fn eva(position: Vec3) -> Mode {
    Mode::Eva {
        position,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    }
}

fn gunner_on_deck(app: &mut App, feet: Vec3) -> Entity {
    let ship = ship(app);
    let mode = Mode::Booted {
        body: ship,
        feet,
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    spawn(app, Robot::Gunner, mode, feet)
}

#[test]
fn a_gunner_shoots_crew_it_can_see() {
    let mut app = app();
    // In the aft hold with the crew member (who stands at z = 5), facing
    // forward, away: it has to turn round first.
    gunner_on_deck(&mut app, Vec3::new(0.0, 0.01, 12.0));
    let me = crew(&mut app);
    let full = health(&app, me);
    ticks(&mut app, 60 * 6);
    assert!(health(&app, me) < full - 15.0, "crew still at {}", health(&app, me));
}

#[test]
fn a_gunner_hunts_through_a_doorway() {
    let mut app = app();
    // On the bridge, the crew member aft of the bulkhead, seen only through
    // its doorway: strafing loses it, so it has to come and find it.
    gunner_on_deck(&mut app, Vec3::new(0.0, 0.01, -10.0));
    let me = crew(&mut app);
    let full = health(&app, me);
    ticks(&mut app, 60 * 15);
    assert!(health(&app, me) < full - 15.0, "crew still at {}", health(&app, me));
}

#[test]
fn a_swarmer_closes_in_and_hurts() {
    let mut app = app();
    let me = crew(&mut app);
    // The crew member floats beside the ship; the swarmer starts well off.
    let start = Vec3::new(40.0, 2.0, 0.0);
    app.world_mut().get_mut::<Player>(me).unwrap().mode = eva(Vec3::new(10.0, 2.0, 0.0));
    let swarmer = spawn(&mut app, Robot::Swarmer, eva(start), start);
    let full = health(&app, me);
    let mut closest = f32::MAX;
    for _ in 0..60 * 10 {
        ticks(&mut app, 1);
        let at = |app: &App, e| match app.world().get::<Player>(e).unwrap().mode {
            Mode::Eva { position, .. } => position,
            _ => Vec3::ZERO,
        };
        closest = closest.min(at(&app, swarmer).distance(at(&app, me)));
    }
    assert!(closest < 15.0, "never closer than {closest} m");
    assert!(health(&app, me) < full, "never hurt");
}

#[test]
fn a_robot_out_of_health_is_destroyed() {
    let mut app = app();
    let start = Vec3::new(40.0, 2.0, 0.0);
    let swarmer = spawn(&mut app, Robot::Swarmer, eva(start), start);
    // As damage leaves it: empty, and just hurt.
    *app.world_mut().get_mut::<Vitals>(swarmer).unwrap() = Vitals {
        health: 0.0,
        since_hurt: 0.0,
    };
    // Marked a wreck (so every peer knows it was destroyed), stilled, and
    // gone the tick after.
    ticks(&mut app, 1);
    let robot = app.world().entity(swarmer);
    assert!(robot.contains::<hullbreakers::sim::bots::Wrecked>(), "not marked a wreck");
    assert!(!robot.contains::<PlayerInput>(), "still driven");
    ticks(&mut app, 1);
    assert!(app.world().get_entity(swarmer).is_err(), "still here");
}

#[test]
fn a_gunner_stays_on_its_feet() {
    let mut app = app();
    let ship = ship(&mut app);
    let feet = Vec3::new(0.0, 0.01, -11.0);
    let mode = Mode::Booted {
        body: ship,
        feet,
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::from_rotation_y(std::f32::consts::PI),
        pitch: 0.0,
    };
    let gunner = spawn(&mut app, Robot::Gunner, mode, feet);
    // Twenty seconds of fighting the crew member on deck, strafing across
    // the cabin, without leaving the floor (or climbing a wall).
    for i in 0..60 * 20 {
        ticks(&mut app, 1);
        let p = app.world().get::<Player>(gunner).unwrap();
        if !matches!(p.mode, Mode::Booted { up, .. } if up.y > 0.99) {
            let events: Vec<_> = p.events.iter().collect();
            panic!("left the floor after {i} ticks: {:?} {events:?}", p.mode);
        }
    }
}

/// A carrier, holding still well clear of our ship.
fn carrier(app: &mut App) -> Entity {
    use hullbreakers::sim::ship::ShipHull;
    let carrier = app
        .world_mut()
        .spawn((
            ShipHull::new("carrier"),
            RigidBody::Kinematic,
            Position(Vec3::new(0.0, 0.0, 300.0)),
            Rotation::default(),
        ))
        .id();
    ticks(app, 2);
    carrier
}

fn booted(body: Entity, feet: Vec3, facing: f32) -> Mode {
    Mode::Booted {
        body,
        feet,
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::from_rotation_y(facing),
        pitch: 0.0,
    }
}

fn spawn_on(app: &mut App, home: Entity, feet: Vec3, post: Vec3, index: u32) -> Entity {
    let tuning = Tuning::default();
    app.world_mut()
        .spawn(new_robot(Robot::Gunner, Affixes::NONE, booted(home, feet, 0.0), home, post, &tuning, index))
        .id()
}

fn feet(app: &App, e: Entity) -> Vec3 {
    match app.world().get::<Player>(e).unwrap().mode {
        Mode::Booted { feet, .. } => feet,
        Mode::Eva { position, .. } => position,
    }
}

#[test]
fn a_gunner_hears_gunfire_on_the_deck_above_and_goes_up_the_ramp_to_it() {
    let upper = hullbreakers::sim::design::design("carrier").marker("upper_deck").y;
    use hullbreakers::sim::weapons::Loadout;
    let mut app = app();
    let carrier = carrier(&mut app);
    // The crew member in the upper corridor, shooting at a wall; the Gunner
    // right below, in the lower corridor, where it can't see them. The way
    // up is aft through the hangar and up its ramp: some 70 m.
    let me = crew(&mut app);
    {
        let mut p = app.world_mut().get_mut::<Player>(me).unwrap();
        p.mode = booted(carrier, Vec3::new(0.0, upper + 0.01, -12.0), 0.0);
        p.loadout = Loadout::default();
    }
    app.world_mut().get_mut::<PlayerInput>(me).unwrap().main.held = true;
    let gunner = spawn_on(&mut app, carrier, Vec3::new(0.0, 0.01, -8.0), Vec3::new(0.0, 0.0, -8.0), 0);
    let full = health(&app, me);
    let (mut lowest, mut highest) = (full, 0.0f32);
    for _ in 0..60 * 45 {
        ticks(&mut app, 1);
        lowest = lowest.min(health(&app, me));
        highest = highest.max(feet(&app, gunner).y);
        if lowest < full - 15.0 {
            break;
        }
    }
    assert!(highest > upper - 0.1, "never got upstairs (highest {highest}, now at {})", feet(&app, gunner));
    assert!(lowest < full - 15.0, "never hurt them (at {lowest}); gunner at {}", feet(&app, gunner));
}

#[test]
fn a_gunner_with_nothing_to_fight_walks_back_to_its_post() {
    let mut app = app();
    let carrier = carrier(&mut app);
    // In a lower side room; its post is in the hangar, round two corners.
    let post = Vec3::new(2.5, 0.0, 20.0);
    let gunner = spawn_on(&mut app, carrier, Vec3::new(-4.0, 0.01, -10.0), post, 0);
    ticks(&mut app, 60 * 25);
    let at = feet(&app, gunner);
    assert!(Vec2::new(at.x - post.x, at.z - post.z).length() < 1.5, "at {at}");
}

#[test]
fn a_gunner_that_spots_the_crew_calls_the_others() {
    use hullbreakers::sim::bots::Brain;
    let mut app = app();
    let carrier = carrier(&mut app);
    // The crew member in the hangar, in sight of one Gunner at the
    // hangar's bulkhead door; another in a lower side room can't see them.
    let me = crew(&mut app);
    app.world_mut().get_mut::<Player>(me).unwrap().mode = booted(carrier, Vec3::new(0.0, 0.01, 24.0), 0.0);
    let spotter = spawn_on(&mut app, carrier, Vec3::new(0.0, 0.01, 12.0), Vec3::new(0.0, 0.0, 12.0), 0);
    let other = spawn_on(&mut app, carrier, Vec3::new(4.0, 0.01, 6.0), Vec3::new(4.0, 0.0, 6.0), 1);
    ticks(&mut app, 30);
    let world = app.world();
    assert_eq!(world.get::<Brain>(spotter).unwrap().target, Some(me));
    assert_eq!(world.get::<Brain>(other).unwrap().target, Some(me), "not told");
}

#[test]
fn a_gunner_that_loses_the_crew_searches_where_it_saw_them_then_goes_home() {
    use hullbreakers::sim::bots::Brain;
    let mut app = app();
    let carrier = carrier(&mut app);
    let home_ship = ship(&mut app);
    let me = crew(&mut app);
    // Seen down the lower corridor, ahead of the Gunner...
    app.world_mut().get_mut::<Player>(me).unwrap().mode = booted(carrier, Vec3::new(0.0, 0.01, -16.0), 0.0);
    let post = Vec3::new(0.0, 0.0, -2.0);
    let gunner = spawn_on(&mut app, carrier, Vec3::new(0.0, 0.01, -2.0), post, 0);
    ticks(&mut app, 30);
    assert_eq!(app.world().get::<Brain>(gunner).unwrap().target, Some(me));
    // ...then gone, back aboard our own ship, far off: it can't know where.
    app.world_mut().get_mut::<Player>(me).unwrap().mode = booted(home_ship, Vec3::new(0.0, 0.01, 5.0), 0.0);
    let mut closest = f32::MAX;
    let mut forgot = None;
    for i in 0..60 * 30 {
        ticks(&mut app, 1);
        closest = closest.min(feet(&app, gunner).distance(Vec3::new(0.0, 0.0, -16.0)));
        if forgot.is_none() && app.world().get::<Brain>(gunner).unwrap().target.is_none() {
            forgot = Some(i);
        }
    }
    assert!(closest < 2.0, "never went to look (closest {closest} m)");
    assert!(forgot.is_some(), "never gave up");
    let at = feet(&app, gunner);
    assert!(Vec2::new(at.x - post.x, at.z - post.z).length() < 1.5, "not back at its post: at {at}");
}

// ---------------------------------------------------------------------------
// Affixes

use hullbreakers::sim::bots::{Affix, Brain, Condition};
use hullbreakers::sim::events::{Detach, PlayerEvent};
use hullbreakers::sim::vitals::Damage;

fn spawn_with(app: &mut App, kind: Robot, affixes: Affixes, mode: Mode, post: Vec3) -> Entity {
    let ship = ship(app);
    let tuning = Tuning::default();
    let robot = app.world_mut().spawn(new_robot(kind, affixes, mode, ship, post, &tuning, 0)).id();
    ticks(app, 1);
    robot
}

/// A hit of `amount` on `target`, on its weak spot or not, from nobody.
fn hit(app: &mut App, target: Entity, amount: f32, weak: bool) {
    app.world_mut().write_message(Damage {
        target,
        amount,
        source: None,
        weak,
        from: None,
    });
    ticks(app, 1);
}

fn condition(app: &App, e: Entity) -> Condition {
    *app.world().get::<Condition>(e).unwrap()
}

/// Far off the ship, where nothing will come of it fighting.
const ADRIFT: Vec3 = Vec3::new(60.0, 2.0, 0.0);

/// Holds a robot still and out of the fight (its affixes still work).
fn idle(app: &mut App, robot: Entity) {
    app.world_mut().get_mut::<Brain>(robot).unwrap().dormant = true;
    app.world_mut().get_mut::<Player>(robot).unwrap().flight_assist = false;
}

#[test]
fn armour_turns_body_hits_but_not_weak_spot_hits() {
    let mut app = app();
    let gunner = spawn_with(&mut app, Robot::Gunner, Affixes::of(&[Affix::Armoured]), eva(ADRIFT), ADRIFT);
    let full = health(&app, gunner);
    hit(&mut app, gunner, 20.0, false);
    let body = full - health(&app, gunner);
    assert!((body - 6.0).abs() < 1e-3, "a 20 body hit took {body}");
    hit(&mut app, gunner, 20.0, true);
    let weak = full - body - health(&app, gunner);
    assert!((weak - 20.0).abs() < 1e-3, "a 20 weak-spot hit took {weak}");
}

#[test]
fn a_shell_takes_everything_until_broken_then_comes_back() {
    let mut app = app();
    let gunner = spawn_with(&mut app, Robot::Gunner, Affixes::of(&[Affix::Shielded]), eva(ADRIFT), ADRIFT);
    let full = health(&app, gunner);
    assert_eq!(condition(&app, gunner).shell, 16);
    // 120 health, so a 72-point shell. Weak-spot hits are stopped too.
    hit(&mut app, gunner, 30.0, true);
    hit(&mut app, gunner, 30.0, false);
    assert_eq!(health(&app, gunner), full, "hurt through the shell");
    let left = condition(&app, gunner).shell_fraction();
    assert!((left - 12.0 / 72.0).abs() <= 1.0 / 16.0, "shell at {left}");
    // The hit that breaks it is spent on it; the next one gets through.
    hit(&mut app, gunner, 30.0, false);
    assert_eq!(health(&app, gunner), full);
    assert_eq!(condition(&app, gunner).shell, 0);
    hit(&mut app, gunner, 30.0, false);
    assert!(health(&app, gunner) < full - 29.0);
    // Left alone, it comes back.
    ticks(&mut app, 60 * 6);
    assert_eq!(condition(&app, gunner).shell, 16, "never came back");
}

#[test]
fn an_overclocked_robot_overheats_vents_and_is_all_weak_spot_while_it_does() {
    let mut app = app();
    // On the bridge facing the crew member aft: it fights, firing.
    let ship = ship(&mut app);
    let feet = Vec3::new(0.0, 0.01, -11.0);
    let mode = Mode::Booted {
        body: ship,
        feet,
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::from_rotation_y(std::f32::consts::PI),
        pitch: 0.0,
    };
    let gunner = spawn_with(&mut app, Robot::Gunner, Affixes::of(&[Affix::Overclocked]), mode, feet);
    let mut vented = false;
    for _ in 0..60 * 20 {
        ticks(&mut app, 1);
        if condition(&app, gunner).venting {
            vented = true;
            break;
        }
    }
    assert!(vented, "never overheated");
    assert!(condition(&app, gunner).heat >= 15);
    // Venting: a body hit counts two and a half times, as a weak-spot hit;
    // it neither shoots nor walks.
    let before = health(&app, gunner);
    hit(&mut app, gunner, 10.0, false);
    assert!((before - health(&app, gunner) - 25.0).abs() < 1e-3, "took {}", before - health(&app, gunner));
    // (It takes a moment to come to a stop.)
    ticks(&mut app, 20);
    let fired_before = newest_shot(&app, gunner);
    let at = feet_of(&app, gunner);
    ticks(&mut app, 60);
    assert_eq!(newest_shot(&app, gunner), fired_before, "fired while venting");
    assert!(feet_of(&app, gunner).distance(at) < 0.05, "walked while venting");
    // Then it's back in the fight.
    ticks(&mut app, 60 * 3);
    assert!(!condition(&app, gunner).venting, "still venting");
}

/// The tick of the newest shot in `robot`'s log, if any.
fn newest_shot(app: &App, robot: Entity) -> Option<u32> {
    let p = app.world().get::<Player>(robot).unwrap();
    p.events.iter().filter(|(_, e)| matches!(e, PlayerEvent::Fired { .. })).map(|(at, _)| at).last()
}

fn feet_of(app: &App, e: Entity) -> Vec3 {
    match app.world().get::<Player>(e).unwrap().mode {
        Mode::Booted { feet, .. } => feet,
        Mode::Eva { position, .. } => position,
    }
}

fn velocity(app: &App, e: Entity) -> Vec3 {
    match app.world().get::<Player>(e).unwrap().mode {
        Mode::Booted { velocity, .. } | Mode::Eva { velocity, .. } => velocity,
    }
}

fn kill(app: &mut App, e: Entity) {
    app.world_mut().get_mut::<Vitals>(e).unwrap().health = 0.0;
}

#[test]
fn a_volatile_robot_blows_up_hurting_and_shoving_by_distance_and_mass() {
    let mut app = app();
    let me = crew(&mut app);
    // The crew member and a Gunner either side of a volatile Swarmer, 2 m
    // off; another Gunner 4 m off.
    let bomb = spawn_with(&mut app, Robot::Swarmer, Affixes::of(&[Affix::Volatile]), eva(ADRIFT), ADRIFT);
    {
        let mut p = app.world_mut().get_mut::<Player>(me).unwrap();
        p.mode = eva(ADRIFT + Vec3::X * 2.0);
        p.flight_assist = false;
    }
    let near = spawn_with(&mut app, Robot::Gunner, Affixes::NONE, eva(ADRIFT - Vec3::X * 2.0), ADRIFT);
    let far = spawn_with(&mut app, Robot::Gunner, Affixes::NONE, eva(ADRIFT + Vec3::Z * 4.0), ADRIFT);
    // All held still where they were put.
    for (e, at) in [(bomb, ADRIFT), (near, ADRIFT - Vec3::X * 2.0), (far, ADRIFT + Vec3::Z * 4.0)] {
        idle(&mut app, e);
        app.world_mut().get_mut::<Player>(e).unwrap().mode = eva(at);
    }
    let (crew_full, near_full, far_full) = (health(&app, me), health(&app, near), health(&app, far));
    let v0 = velocity(&app, me);
    kill(&mut app, bomb);
    ticks(&mut app, 3);
    assert!(app.world().get_entity(bomb).is_err());
    let (crew_hurt, near_hurt, far_hurt) = (crew_full - health(&app, me), near_full - health(&app, near), far_full - health(&app, far));
    // A Swarmer's blast: 45 at its heart, nothing at 4.5 m.
    assert!((crew_hurt - 45.0 * (1.0 - 2.0 / 4.5)).abs() < 1.0, "crew took {crew_hurt}");
    assert!((near_hurt - crew_hurt).abs() < 1.0, "the Gunner as near took {near_hurt}");
    assert!(far_hurt < near_hurt / 2.0, "the far Gunner took {far_hurt}");
    // Shoved out from it, the light crew member much harder than the
    // Gunner.
    let kick = velocity(&app, me) - v0;
    assert!(kick.x > 1.0, "crew kicked {kick}");
    let gunner_kick = velocity(&app, near);
    assert!(gunner_kick.x < 0.0 && gunner_kick.length() < kick.length() / 1.5, "Gunner kicked {gunner_kick}, crew {kick}");
}

#[test]
fn volatile_robots_set_each_other_off() {
    let mut app = app();
    let me = crew(&mut app);
    // A volatile Gunner, and a volatile Swarmer 3 m from it; the crew member
    // 3.5 m beyond the Swarmer, out of the Gunner's reach (6 m) but in the
    // Swarmer's.
    let gunner = spawn_with(&mut app, Robot::Gunner, Affixes::of(&[Affix::Volatile]), eva(ADRIFT), ADRIFT);
    let swarmer = spawn_with(&mut app, Robot::Swarmer, Affixes::of(&[Affix::Volatile]), eva(ADRIFT + Vec3::X * 3.0), ADRIFT);
    idle(&mut app, gunner);
    idle(&mut app, swarmer);
    {
        let mut p = app.world_mut().get_mut::<Player>(me).unwrap();
        p.mode = eva(ADRIFT + Vec3::X * 6.5);
        p.flight_assist = false;
    }
    let full = health(&app, me);
    kill(&mut app, gunner);
    ticks(&mut app, 6);
    assert!(app.world().get_entity(swarmer).is_err(), "the Swarmer survived the Gunner's blast");
    assert!(health(&app, me) < full, "the Swarmer didn't go up");
}

#[test]
fn a_blast_knocks_the_crew_off_their_boots_but_not_through_a_wall() {
    let mut app = app();
    let carrier = carrier(&mut app);
    let me = crew(&mut app);
    // The crew member in the carrier's lower corridor; one volatile Gunner
    // 2 m along it, another in the side room beyond the corridor's wall.
    app.world_mut().get_mut::<Player>(me).unwrap().mode = booted(carrier, Vec3::new(0.0, 0.01, -12.0), 0.0);
    let tuning = Tuning::default();
    let volatile = Affixes::of(&[Affix::Volatile]);
    let room = app
        .world_mut()
        .spawn(new_robot(Robot::Gunner, volatile, booted(carrier, Vec3::new(4.0, 0.01, -12.0), 0.0), carrier, Vec3::ZERO, &tuning, 0))
        .id();
    let corridor = app
        .world_mut()
        .spawn(new_robot(Robot::Gunner, volatile, booted(carrier, Vec3::new(0.0, 0.01, -14.0), 0.0), carrier, Vec3::ZERO, &tuning, 1))
        .id();
    ticks(&mut app, 1);
    idle(&mut app, room);
    idle(&mut app, corridor);
    let full = health(&app, me);
    kill(&mut app, room);
    ticks(&mut app, 3);
    assert_eq!(health(&app, me), full, "hurt through the wall");
    assert!(matches!(app.world().get::<Player>(me).unwrap().mode, Mode::Booted { .. }));
    kill(&mut app, corridor);
    ticks(&mut app, 3);
    assert!(health(&app, me) < full - 20.0, "at {}", health(&app, me));
    let p = app.world().get::<Player>(me).unwrap();
    assert!(matches!(p.mode, Mode::Eva { .. }), "still on its feet: {:?}", p.mode);
    assert!(p.events.iter().any(|(_, e)| e == PlayerEvent::Detached(Detach::Blasted)));
}

#[test]
fn a_leader_rallies_robots_near_it_while_it_lives() {
    let mut app = app();
    let leader = spawn_with(&mut app, Robot::Gunner, Affixes::of(&[Affix::Leader]), eva(ADRIFT), ADRIFT);
    let near = spawn_with(&mut app, Robot::Gunner, Affixes::NONE, eva(ADRIFT + Vec3::X * 8.0), ADRIFT);
    let far = spawn_with(&mut app, Robot::Swarmer, Affixes::NONE, eva(ADRIFT + Vec3::X * 40.0), ADRIFT);
    for e in [leader, near, far] {
        idle(&mut app, e);
    }
    ticks(&mut app, 2);
    assert!(condition(&app, near).rallied);
    assert!(!condition(&app, far).rallied);
    assert!(!condition(&app, leader).rallied, "rallied by itself");
    kill(&mut app, leader);
    ticks(&mut app, 3);
    assert!(!condition(&app, near).rallied, "rallied by a dead leader");
}

#[test]
fn a_marksman_winds_up_visibly_before_every_shot() {
    let mut app = app();
    let ship = ship(&mut app);
    let feet = Vec3::new(0.0, 0.01, -11.0);
    let mode = Mode::Booted {
        body: ship,
        feet,
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::from_rotation_y(std::f32::consts::PI),
        pitch: 0.0,
    };
    let marksman = spawn_with(&mut app, Robot::Gunner, Affixes::of(&[Affix::Marksman]), mode, feet);
    let me = crew(&mut app);
    let full = health(&app, me);
    // Every shot comes after a wind-up, seen as it winds, locked at the end.
    let mut wound = 0u32;
    let mut locked = false;
    let mut shots = 0;
    let mut last = newest_shot(&app, marksman);
    for _ in 0..60 * 12 {
        ticks(&mut app, 1);
        let c = condition(&app, marksman);
        if c.winding > 0 {
            wound += 1;
            locked |= c.locked;
        }
        // (Old shots age out of the log: only a newer one is a new shot.)
        let now = newest_shot(&app, marksman);
        if now.is_some_and(|t| last.is_none_or(|l| t > l)) {
            last = now;
            assert!(wound as f32 >= 0.9 * 60.0 && locked, "shot after only {wound} ticks of wind-up (locked: {locked})");
            wound = 0;
            locked = false;
            shots += 1;
        }
    }
    assert!(shots >= 2, "only {shots} shots");
    assert!(health(&app, me) < full, "never hit");
}

#[test]
fn a_grappler_hooks_the_crew_off_their_feet_reels_them_in_and_lets_go() {
    let mut app = app();
    let carrier = carrier(&mut app);
    let me = crew(&mut app);
    // The crew member in the hangar, in the grappler's sight through the
    // bulkhead door, 12 m off.
    app.world_mut().get_mut::<Player>(me).unwrap().mode = booted(carrier, Vec3::new(0.0, 0.01, 24.0), 0.0);
    let tuning = Tuning::default();
    let start = Vec3::new(0.0, 0.01, 12.0);
    let grappler = app
        .world_mut()
        .spawn(new_robot(Robot::Grappler, Affixes::NONE, booted(carrier, start, 0.0), carrier, start, &tuning, 0))
        .id();
    let mut yanked = false;
    let mut closest = f32::MAX;
    let mut hooked = false;
    let mut let_go = false;
    let mut grounded = true;
    for _ in 0..60 * 8 {
        ticks(&mut app, 1);
        let p = app.world().get::<Player>(me).unwrap();
        yanked |= p.events.iter().any(|(_, e)| matches!(e, PlayerEvent::Detached(Detach::Yanked(_))));
        let g = app.world().get::<Player>(grappler).unwrap();
        grounded &= matches!(g.mode, Mode::Booted { .. });
        hooked |= g.grapple.is_some_and(|g| g.target == me);
        let_go |= hooked && g.grapple.is_none();
        // (Pulled off, they're in EVA, in the world's frame: the carrier
        // is at rest 300 m down +Z.)
        let crew_at = match app.world().get::<Player>(me).unwrap().mode {
            Mode::Booted { feet, .. } => feet,
            Mode::Eva { position, .. } => position - Vec3::new(0.0, 0.0, 300.0),
        };
        closest = closest.min(crew_at.distance(start));
        if let_go {
            break;
        }
    }
    assert!(hooked, "never hooked them");
    assert!(yanked, "never pulled them off their boots");
    assert!(closest < 6.0, "reeled them only to {closest} m");
    assert!(let_go, "never let go");
    // Its own boots held throughout.
    assert!(grounded, "the grappler pulled itself off its feet");
}

#[test]
fn a_hull_turret_stays_bolted_down_and_hits_crew_crossing_open_space() {
    let mut app = app();
    let carrier = carrier(&mut app);
    let me = crew(&mut app);
    // On the carrier's roof aft; the crew member crossing 50 m above, side
    // on, drifting at 6 m/s with no assist: it has to lead them.
    let roof = Vec3::new(3.0, 6.6, 26.0);
    let up = hullbreakers::sim::bots::mount(hullbreakers::sim::design::design("carrier"), roof).unwrap();
    assert_eq!(up, Vec3::Y);
    let carrier_at = Vec3::new(0.0, 0.0, 300.0);
    {
        let mut p = app.world_mut().get_mut::<Player>(me).unwrap();
        p.mode = Mode::Eva {
            position: carrier_at + roof + Vec3::new(-40.0, 50.0, 10.0),
            velocity: Vec3::new(6.0, 0.0, 0.0),
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
        p.flight_assist = false;
    }
    let tuning = Tuning::default();
    let feet = roof + up * 0.01;
    let turret = app
        .world_mut()
        .spawn(new_robot(Robot::Turret, Affixes::NONE, booted(carrier, feet, 0.0), carrier, feet, &tuning, 0))
        .id();
    let full = health(&app, me);
    let mut hurt = false;
    for _ in 0..60 * 12 {
        ticks(&mut app, 1);
        if health(&app, me) < full {
            hurt = true;
            break;
        }
    }
    assert!(hurt, "never hit them");
    assert!(feet_of(&app, turret).distance(feet) < 0.02, "turret moved to {}", feet_of(&app, turret));
}

#[test]
fn a_destroyed_robot_drops_loot_once_and_a_turret_none() {
    use hullbreakers::sim::pickups::Pickup;
    let mut app = app();
    let carrier = carrier(&mut app);
    let tuning = Tuning::default();
    let feet = Vec3::new(0.0, 0.01, -12.0);
    let gunner = app
        .world_mut()
        .spawn(new_robot(Robot::Gunner, Affixes::NONE, booted(carrier, feet, 0.0), carrier, feet, &tuning, 0))
        .id();
    let roof = Vec3::new(3.0, 6.61, 26.0);
    let turret = app
        .world_mut()
        .spawn(new_robot(Robot::Turret, Affixes::NONE, booted(carrier, roof, 0.0), carrier, roof, &tuning, 1))
        .id();
    ticks(&mut app, 2);
    let pickups = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Pickup>().iter(world).count()
    };
    let before = pickups(&mut app);
    kill(&mut app, gunner);
    kill(&mut app, turret);
    ticks(&mut app, 4);
    assert!(app.world().get_entity(gunner).is_err() && app.world().get_entity(turret).is_err());
    assert_eq!(pickups(&mut app), before + 1, "one drop, the gunner's");
}
