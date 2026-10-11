//! Guns: firing, recoil, and what bullets do to what they hit.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::events::PlayerEvent;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::player::{Mode, Player, new_player};
use hullbreakers::sim::ship::HomeShip;
use hullbreakers::sim::shots::Team;
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

/// The local player (the one with input).
fn shooter(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, With<PlayerInput>>().single(world).unwrap()
}

fn set_mode(app: &mut App, entity: Entity, mode: Mode) {
    app.world_mut().get_mut::<Player>(entity).unwrap().mode = mode;
}

fn eva_at(position: Vec3) -> Mode {
    Mode::Eva {
        position,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    }
}

/// Floats the shooter far from the ship, assist off, facing −Z.
fn float_shooter(app: &mut App) -> Entity {
    let me = shooter(app);
    set_mode(app, me, eva_at(Vec3::new(0.0, 0.0, 200.0)));
    app.world_mut().get_mut::<Player>(me).unwrap().flight_assist = false;
    me
}

fn hold_trigger(app: &mut App, me: Entity, held: bool) {
    app.world_mut().get_mut::<PlayerInput>(me).unwrap().main.held = held;
}

#[test]
fn firing_pushes_a_floating_shooter_back() {
    let mut app = app();
    let me = float_shooter(&mut app);
    hold_trigger(&mut app, me, true);
    ticks(&mut app, 60);
    let player = app.world().get::<Player>(me).unwrap().clone();
    let fired = player
        .events
        .iter()
        .filter(|(_, e)| matches!(e, PlayerEvent::Fired { .. }))
        .count();
    assert!(fired >= 14, "fired {fired}");
    let Mode::Eva { velocity, .. } = player.mode else {
        panic!("not floating");
    };
    // Recoil pushes back along +Z.
    assert!(velocity.z > 0.3, "recoil {velocity}");
}

fn spawn_target(app: &mut App, team: Team, at: Vec3) -> Entity {
    let ship = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap()
    };
    let tuning = Tuning::default();
    let target = app
        .world_mut()
        .spawn((new_player(ship, 1), Vitals::full(&tuning), team))
        .id();
    set_mode(app, target, eva_at(at));
    ticks(app, 2);
    target
}

#[test]
fn bullets_hurt_the_other_side_and_pass_through_our_own() {
    let mut app = app();
    let me = float_shooter(&mut app);
    let eye = Tuning::default().eye_height - Tuning::default().capsule_height / 2.0;
    // Both straight ahead, a teammate in front of a robot.
    let friend = spawn_target(&mut app, Team::Crew, Vec3::new(0.0, eye, 190.0));
    let robot = spawn_target(&mut app, Team::Robots, Vec3::new(0.0, eye, 175.0));
    // The rifle, braced (stance 3): two shots.
    app.world_mut().get_mut::<PlayerInput>(me).unwrap().stance = 2;
    ticks(&mut app, 30);
    hold_trigger(&mut app, me, true);
    ticks(&mut app, 10);
    hold_trigger(&mut app, me, false);
    ticks(&mut app, 30);
    let health = |app: &App, e| app.world().get::<Vitals>(e).unwrap().health;
    let full = Tuning::default().player_health;
    assert_eq!(health(&app, friend), full, "hit a teammate");
    assert!(health(&app, robot) < full - 30.0, "robot only down to {}", health(&app, robot));
}
