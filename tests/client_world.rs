//! The world as a network client has it: bodies are colliders posed from
//! their replicated state (no rigid bodies, no transform sync), and only the
//! local player is simulated. Prediction runs `player_tick` in this world.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::player::{Mode, Player, new_player};
use hullbreakers::sim::ship::{HullKind, ShipHull};
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::{PlayerSimPlugin, SimSharedPlugin};

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
            ShipHull(HullKind::Home),
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


/// Another player, as a client sees them (replicated, not simulated here),
/// can be hit by a ray: what grappling them needs.
#[test]
fn a_remote_player_can_be_aimed_at() {
    let (mut app, ship) = client_app();
    // Not simulated here: no PlayerInput.
    let remote = app.world_mut().spawn(new_player(ship, 2)).id();
    app.world_mut().entity_mut(remote).remove::<PlayerInput>();
    for _ in 0..5 {
        app.update();
    }
    let world = app.world_mut();
    let others: Vec<Entity> = world
        .query_filtered::<Entity, With<Player>>()
        .iter(world)
        .filter(|e| *e != remote)
        .collect();
    let world_players_except = |_: Entity| others.clone();
    let target = world.get::<Position>(remote).unwrap().0;
    let mut state = bevy::ecs::system::SystemState::<SpatialQuery>::new(world);
    let sq = state.get(world).unwrap();
    let from = target + Vec3::new(3.0, 0.0, 0.0);
    let filter = SpatialQueryFilter::default().with_excluded_entities(
        world_players_except(remote),
    );
    let hit = sq.cast_ray(from, Dir3::NEG_X, 10.0, true, &filter);
    assert_eq!(hit.map(|h| h.entity), Some(remote), "ray from {from} to {target}: {hit:?}");
}
