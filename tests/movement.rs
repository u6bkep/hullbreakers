//! Headless movement tests: the real sim, ticked with scripted commands.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use space_game::sim::SimPlugin;
use space_game::sim::frame::BodyFrame;
use space_game::sim::player::{Mode, Player, PlayerCommand};
use space_game::sim::ship::Ship;
use space_game::sim::tuning::Tuning;

fn app_with(tuning: Tuning) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
    ));
    app.add_plugins(PhysicsPlugins::default());
    app.insert_resource(tuning);
    app.add_plugins(SimPlugin { asteroids: false });
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    app.finish();
    app.cleanup();
    // Startup, plus a few ticks for colliders and mass properties to settle.
    for _ in 0..3 {
        app.update();
    }
    app
}

fn app() -> App {
    app_with(Tuning::default())
}

fn player(app: &mut App) -> Player {
    app.world_mut()
        .query::<&Player>()
        .single(app.world())
        .unwrap()
        .clone()
}

fn set_mode(app: &mut App, mode: Mode) {
    let world = app.world_mut();
    let mut p = world.query::<&mut Player>().single_mut(world).unwrap();
    p.mode = mode;
}

fn command(app: &mut App, f: impl FnOnce(&mut PlayerCommand)) {
    let world = app.world_mut();
    let mut c = world
        .query::<&mut PlayerCommand>()
        .single_mut(world)
        .unwrap();
    f(&mut c);
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn ship(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<Ship>>()
        .single(app.world())
        .unwrap()
}

fn ship_frame(app: &mut App) -> BodyFrame {
    let world = app.world_mut();
    let (pos, rot, lin, ang, com) = world
        .query_filtered::<(
            &Position,
            &Rotation,
            &LinearVelocity,
            &AngularVelocity,
            &ComputedCenterOfMass,
        ), With<Ship>>()
        .single(world)
        .unwrap();
    BodyFrame {
        position: pos.0,
        rotation: rot.0,
        center_of_mass: pos.0 + rot.0 * com.0,
        linear_velocity: lin.0,
        angular_velocity: ang.0,
    }
}

fn booted(app: &mut App, feet: Vec3, up: Vec3, forward: Vec3) -> Mode {
    let right = forward.cross(up).normalize();
    let orientation = Quat::from_mat3(&Mat3::from_cols(right, up, -forward));
    Mode::Booted {
        body: ship(app),
        feet,
        up,
        velocity: Vec3::ZERO,
        orientation,
        pitch: 0.0,
    }
}

fn booted_state(app: &mut App) -> (Entity, Vec3, Vec3, Vec3) {
    match player(app).mode {
        Mode::Booted {
            body,
            feet,
            up,
            velocity,
            ..
        } => (body, feet, up, velocity),
        Mode::Eva { .. } => panic!("expected booted, got EVA: {}", player(app).last_event),
    }
}

#[test]
fn stands_still_on_rolling_ship() {
    let mut app = app();
    ticks(&mut app, 600);
    let (_, feet, up, _) = booted_state(&mut app);
    assert!(
        feet.abs_diff_eq(Vec3::new(0.0, 0.01, 5.0), 0.05),
        "drifted to {feet}"
    );
    assert!(up.abs_diff_eq(Vec3::Y, 1e-3));
}

#[test]
fn walks_forward() {
    let mut app = app();
    command(&mut app, |c| c.movement = Vec3::Z);
    ticks(&mut app, 60);
    let (_, feet, _, velocity) = booted_state(&mut app);
    assert!(feet.z < 5.0 - 2.5 && feet.z > 5.0 - 3.1, "feet at {feet}");
    assert!((velocity.length() - 3.0).abs() < 0.1);
}

#[test]
fn jump_carries_surface_velocity() {
    let mut tuning = Tuning::default();
    tuning.ship_roll_rate_deg = 30.0;
    tuning.ship_yaw_wobble_deg = 0.0;
    let mut app = app_with(tuning.clone());
    let mode = booted(&mut app, Vec3::new(3.0, 0.01, 5.0), Vec3::Y, Vec3::NEG_Z);
    set_mode(&mut app, mode);
    ticks(&mut app, 30);
    command(&mut app, |c| c.jump = true);
    ticks(&mut app, 1);
    let Mode::Eva {
        position, velocity, ..
    } = player(&mut app).mode
    else {
        panic!("did not jump")
    };
    let frame = ship_frame(&mut app);
    let expected = frame.point_velocity(position) + frame.dir_to_world(Vec3::Y) * tuning.jump_speed;
    assert!(
        velocity.abs_diff_eq(expected, 0.1),
        "velocity {velocity} expected {expected}"
    );
}

#[test]
fn walks_over_hull_edges() {
    let mut app = app();
    // On the roof near the starboard edge, facing starboard.
    let mode = booted(&mut app, Vec3::new(2.5, 3.31, -12.0), Vec3::Y, Vec3::X);
    set_mode(&mut app, mode);
    command(&mut app, |c| c.movement = Vec3::Z);
    // Roof -> starboard side -> underside -> port side: a full loop around the hull.
    let expected = [Vec3::X, Vec3::NEG_Y, Vec3::NEG_X];
    let mut reached = 0;
    for _ in 0..300 {
        ticks(&mut app, 1);
        let (_, _, up, _) = booted_state(&mut app);
        if reached < expected.len() && up.dot(expected[reached]) > 0.95 {
            reached += 1;
        }
    }
    let (_, feet, up, _) = booted_state(&mut app);
    assert_eq!(
        reached,
        expected.len(),
        "stalled at up = {up}, feet = {feet}"
    );
}

#[test]
fn walks_up_walls() {
    let mut app = app();
    // In the aft compartment, facing the mid bulkhead away from its doorway.
    let mode = booted(&mut app, Vec3::new(-2.0, 0.01, 3.0), Vec3::Y, Vec3::NEG_Z);
    set_mode(&mut app, mode);
    command(&mut app, |c| c.movement = Vec3::Z);
    // Floor -> bulkhead -> ceiling.
    let mut saw_wall = false;
    for _ in 0..150 {
        ticks(&mut app, 1);
        saw_wall |= booted_state(&mut app).2.z > 0.95;
    }
    let (_, feet, up, _) = booted_state(&mut app);
    assert!(saw_wall, "never climbed the bulkhead");
    assert!(
        up.y < -0.95 && (feet.y - 3.0).abs() < 0.05,
        "expected to be on the ceiling, up = {up}, feet = {feet}"
    );
}

#[test]
fn lurch_rips_player_off_the_stern() {
    let mut app = app();
    // Standing on the outside of the stern, which the forward lurch pulls away.
    let mode = booted(&mut app, Vec3::new(2.6, 1.5, 15.01), Vec3::Z, Vec3::Y);
    set_mode(&mut app, mode);
    ticks(&mut app, 10);
    booted_state(&mut app);
    app.world_mut()
        .query::<&mut Ship>()
        .single_mut(app.world_mut())
        .unwrap()
        .lurch();
    ticks(&mut app, 30);
    let p = player(&mut app);
    assert!(matches!(p.mode, Mode::Eva { .. }), "still booted");
    assert!(p.last_event.contains("ripped"), "event: {}", p.last_event);
}

#[test]
fn lurch_does_not_rip_player_off_the_deck() {
    let mut app = app();
    ticks(&mut app, 10);
    app.world_mut()
        .query::<&mut Ship>()
        .single_mut(app.world_mut())
        .unwrap()
        .lurch();
    ticks(&mut app, 90);
    booted_state(&mut app);
}

#[test]
fn lands_feet_first_on_deck() {
    let mut app = app();
    let frame = ship_frame(&mut app);
    let position = frame.point_to_world(Vec3::new(0.0, 1.6, 5.0));
    let velocity = frame.point_velocity(position) + frame.dir_to_world(Vec3::NEG_Y);
    set_mode(
        &mut app,
        Mode::Eva {
            position,
            velocity,
            orientation: frame.rotation,
            pitch: 0.0,
        },
    );
    ticks(&mut app, 60);
    let (_, feet, up, _) = booted_state(&mut app);
    assert!(up.abs_diff_eq(Vec3::Y, 0.05), "up {up}");
    assert!(feet.y.abs() < 0.05, "feet {feet}");
}

#[test]
fn bounces_when_landing_head_first() {
    let mut app = app();
    let frame = ship_frame(&mut app);
    let position = frame.point_to_world(Vec3::new(0.0, 1.6, 5.0));
    let velocity = frame.point_velocity(position) + frame.dir_to_world(Vec3::NEG_Y);
    // Upside down: head toward the deck.
    let orientation = frame.rotation * Quat::from_rotation_x(std::f32::consts::PI);
    set_mode(
        &mut app,
        Mode::Eva {
            position,
            velocity,
            orientation,
            pitch: 0.0,
        },
    );
    ticks(&mut app, 60);
    let p = player(&mut app);
    let Mode::Eva {
        position, velocity, ..
    } = p.mode
    else {
        panic!("attached head first")
    };
    let rel = ship_frame(&mut app).velocity_to_local(position, velocity);
    assert!(
        rel.y > 0.0,
        "should be moving away from the deck, rel = {rel}"
    );
}

#[test]
fn grapple_reels_player_in() {
    let mut app = app();
    let frame = ship_frame(&mut app);
    let position = frame.point_to_world(Vec3::new(2.6, 1.5, 30.0));
    let velocity = frame.point_velocity(position);
    set_mode(
        &mut app,
        Mode::Eva {
            position,
            velocity,
            orientation: frame.rotation,
            pitch: 0.0,
        },
    );
    command(&mut app, |c| c.grapple = true);
    ticks(&mut app, 1);
    let grapple = player(&mut app)
        .grapple
        .expect("grapple should hit the stern");
    assert!(
        (grapple.length - 15.0).abs() < 0.5,
        "length {}",
        grapple.length
    );
    command(&mut app, |c| c.reel_in = true);
    ticks(&mut app, 90);
    let p = player(&mut app);
    let g = p.grapple.expect("rope still attached");
    assert!(g.length < 10.0, "rope length {}", g.length);
}

/// Spawns a free rock (a sphere, for predictable geometry) and returns it.
fn spawn_rock(app: &mut App, position: Vec3, radius: f32, angular: Vec3) -> Entity {
    let rock = app
        .world_mut()
        .spawn((
            Collider::sphere(radius),
            ColliderDensity(space_game::sim::asteroids::ROCK_DENSITY),
            RigidBody::Dynamic,
            SleepingDisabled,
            space_game::sim::bodies::PreviousVelocity::default(),
            AngularVelocity(angular),
            Transform::from_translation(position),
        ))
        .id();
    ticks(app, 2);
    rock
}

#[test]
fn lands_on_and_walks_around_a_spinning_rock() {
    let mut app = app();
    let rock = spawn_rock(
        &mut app,
        Vec3::new(0.0, 0.0, 80.0),
        6.0,
        Vec3::new(0.0, 0.3, 0.0),
    );
    // Feet toward the rock (+Z), drifting in slowly.
    let orientation = Quat::from_rotation_arc(Vec3::Y, Vec3::NEG_Z);
    set_mode(
        &mut app,
        Mode::Eva {
            position: Vec3::new(0.0, 0.0, 72.0),
            velocity: Vec3::Z,
            orientation,
            pitch: 0.0,
        },
    );
    ticks(&mut app, 120);
    let (body, feet, up, _) = booted_state(&mut app);
    assert_eq!(body, rock, "booted onto the wrong body");
    assert!(
        (feet.length() - 6.0).abs() < 0.05,
        "feet not on the surface: {feet}"
    );
    assert!(
        up.dot(feet.normalize()) > 0.99,
        "up {up} not radial at {feet}"
    );

    // Walk a quarter of the way around; boots must hold on the spinning sphere.
    command(&mut app, |c| c.movement = Vec3::Z);
    ticks(&mut app, 180);
    let (body, feet, up, _) = booted_state(&mut app);
    assert_eq!(body, rock);
    assert!(
        (feet.length() - 6.0).abs() < 0.05,
        "feet left the surface: {feet}"
    );
    assert!(up.dot(feet.normalize()) > 0.99);
}

#[test]
fn bouncing_off_a_light_rock_conserves_momentum() {
    let mut tuning = Tuning::default();
    tuning.bounce_restitution = 0.5;
    let mut app = app_with(tuning.clone());
    let rock = spawn_rock(&mut app, Vec3::new(0.0, 0.0, 80.0), 0.4, Vec3::ZERO);
    let rock_mass = app.world().get::<ComputedMass>(rock).unwrap().value();
    // Boots off so we bounce rather than land.
    {
        let world = app.world_mut();
        world
            .query::<&mut Player>()
            .single_mut(world)
            .unwrap()
            .boots_on = false;
    }
    let v0 = Vec3::Z * 2.0;
    set_mode(
        &mut app,
        Mode::Eva {
            position: Vec3::new(0.0, 0.0, 77.0),
            velocity: v0,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        },
    );
    ticks(&mut app, 120);
    let Mode::Eva { velocity, .. } = player(&mut app).mode else {
        panic!("attached with boots off")
    };
    let rock_v = app.world().get::<LinearVelocity>(rock).unwrap().0;
    let before = v0 * tuning.player_mass;
    let after = velocity * tuning.player_mass + rock_v * rock_mass;
    assert!(rock_v.z > 0.5, "rock should be knocked away, v = {rock_v}");
    assert!(velocity.z < v0.z, "player should slow down, v = {velocity}");
    assert!(
        before.abs_diff_eq(after, 0.02 * before.length()),
        "momentum {before} -> {after}"
    );
}
