//! The world as a network client has it: bodies are colliders posed from
//! their replicated state (no rigid bodies, no transform sync), and only the
//! local player is simulated. Prediction runs `player_tick` in this world.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use space_game::sim::input::PlayerInput;
use space_game::sim::player::{Mode, Player, new_player};
use space_game::sim::ship::ShipHull;
use space_game::sim::tuning::Tuning;
use space_game::sim::{PlayerSimPlugin, SimSharedPlugin};

fn client_app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
    ));
    app.add_plugins(
        PhysicsPlugins::default()
            .build()
            .disable::<PhysicsTransformPlugin>()
            .disable::<PhysicsInterpolationPlugin>(),
    );
    app.insert_resource(Tuning::default())
        .insert_resource(Gravity(Vec3::ZERO))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .add_plugins((SimSharedPlugin, PlayerSimPlugin));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    app.finish();
    app.cleanup();
    // A replicated hull as the network layer sets it up: generating
    // component, pose, velocities, and a static body (never simulated).
    let ship = app
        .world_mut()
        .spawn((
            ShipHull,
            RigidBody::Static,
            Position::default(),
            Rotation::default(),
            LinearVelocity::default(),
            AngularVelocity::default(),
        ))
        .id();
    app.world_mut().spawn(new_player(ship, 0));
    for _ in 0..3 {
        app.update();
    }
    (app, ship)
}

#[test]
fn predicted_player_walks_on_a_replicated_hull() {
    let (mut app, ship) = client_app();
    let world = app.world_mut();
    world
        .query::<&mut PlayerInput>()
        .single_mut(world)
        .unwrap()
        .movement = Vec3::Z;
    for _ in 0..60 {
        app.update();
    }
    let world = app.world_mut();
    let player = world.query::<&Player>().single(world).unwrap();
    let Mode::Booted { body, feet, .. } = player.mode else {
        panic!("fell off: {:?}", player.mode);
    };
    assert_eq!(body, ship);
    assert!(feet.z < 3.0, "didn't walk forward: feet {feet}");
}

