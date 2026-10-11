//! Robots: they see the crew, fight them, and die.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::bots::{Robot, new_robot};
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
    let robot = app.world_mut().spawn(new_robot(kind, mode, ship, post, &tuning, 0)).id();
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
    ticks(&mut app, 2);
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
