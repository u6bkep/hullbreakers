//! Headless movement tests: the real sim, ticked with scripted commands.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::frame::BodyFrame;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::ship::{HomeShip, ShipCommand};
use hullbreakers::sim::tuning::Tuning;

fn app_with(tuning: Tuning) -> App {
    app_configured(tuning, false)
}

fn app_configured(tuning: Tuning, asteroids: bool) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
    ));
    app.add_plugins(PhysicsPlugins::default());
    app.insert_resource(tuning);
    app.add_plugins(SimPlugin {
        asteroids,
        local_player: true,
    });
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

fn command(app: &mut App, f: impl FnOnce(&mut PlayerInput)) {
    let world = app.world_mut();
    let mut c = world.query::<&mut PlayerInput>().single_mut(world).unwrap();
    f(&mut c);
}

/// The text of the player's most recent event.
fn last_event(app: &mut App) -> String {
    player(app)
        .events
        .last()
        .and_then(|(_, e)| e.caption())
        .unwrap_or_default()
}

fn lurch(app: &mut App) {
    app.world_mut().write_message(ShipCommand::Lurch);
}

/// Contact tests drift into surfaces with no input; flight assist would brake.
fn assist_off(app: &mut App) {
    let world = app.world_mut();
    world
        .query::<&mut Player>()
        .single_mut(world)
        .unwrap()
        .flight_assist = false;
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn ship(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<HomeShip>>()
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
        ), With<HomeShip>>()
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
        Mode::Eva { .. } => panic!("expected booted, got EVA: {}", last_event(app)),
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
    command(&mut app, |c| c.presses.jump.press());
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
    lurch(&mut app);
    ticks(&mut app, 30);
    let p = player(&mut app);
    assert!(matches!(p.mode, Mode::Eva { .. }), "still booted");
    let event = last_event(&mut app);
    assert!(event.contains("ripped"), "event: {event}");
}

#[test]
fn lurch_does_not_rip_player_off_the_deck() {
    let mut app = app();
    ticks(&mut app, 10);
    lurch(&mut app);
    ticks(&mut app, 90);
    booted_state(&mut app);
}

#[test]
fn lands_feet_first_on_deck() {
    let mut app = app();
    assist_off(&mut app);
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
    assist_off(&mut app);
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
    assist_off(&mut app);
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
    command(&mut app, |c| c.presses.main.press());
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
            hullbreakers::sim::bodies::dynamic_mass(
                &Collider::sphere(radius),
                hullbreakers::sim::asteroids::ROCK_DENSITY,
            ),
            Collider::sphere(radius),
            RigidBody::Dynamic,
            SleepingDisabled,
            hullbreakers::sim::bodies::PreviousVelocity::default(),
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
    assist_off(&mut app);
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
    assist_off(&mut app);
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

/// Deepest overlap between the player's capsule and any collider.
fn penetration(app: &mut App) -> f32 {
    use bevy::ecs::system::RunSystemOnce;
    app.world_mut()
        .run_system_once(
            |mas: MoveAndSlide,
             tuning: Res<Tuning>,
             players: Query<&Player>,
             poses: Query<(&Position, &Rotation)>| {
                let t = &*tuning;
                let shape =
                    Collider::capsule(t.capsule_radius, t.capsule_height - 2.0 * t.capsule_radius);
                let (center, rotation) = match players.single().unwrap().mode {
                    Mode::Booted { body, feet, up, .. } => {
                        let (pos, rot) = poses.get(body).unwrap();
                        (
                            pos.0 + rot.0 * (feet + up * t.capsule_height / 2.0),
                            rot.0 * Quat::from_rotation_arc(Vec3::Y, up),
                        )
                    }
                    Mode::Eva {
                        position,
                        orientation,
                        ..
                    } => (position, orientation),
                };
                let mut deepest = 0.0f32;
                mas.intersections(
                    &shape,
                    center,
                    rotation,
                    0.0,
                    &SpatialQueryFilter::default(),
                    |_, c, _| {
                        deepest = deepest.max(c.penetration);
                        true
                    },
                );
                deepest
            },
        )
        .unwrap()
}

/// Walks with `movement` for `n` ticks, returning the deepest overlap seen.
fn walk_checked(app: &mut App, movement: Vec3, n: usize) -> f32 {
    command(app, |c| c.movement = movement);
    let mut worst = 0.0f32;
    for _ in 0..n {
        ticks(app, 1);
        worst = worst.max(penetration(app));
    }
    worst
}

#[test]
fn console_edge_toward_bow_does_not_embed() {
    let mut app = app();
    // On top of the bridge console, walking toward the bow wall.
    let mode = booted(&mut app, Vec3::new(0.0, 1.01, -12.8), Vec3::Y, Vec3::NEG_Z);
    set_mode(&mut app, mode);
    let worst = walk_checked(&mut app, Vec3::Z, 240);
    assert!(worst < 0.06, "capsule embedded {worst} m");
    booted_state(&mut app);
}

#[test]
fn window_slot_does_not_let_player_through() {
    let mut app = app();
    // Beside the console, walking into the bow wall below the window slot.
    let mode = booted(&mut app, Vec3::new(2.8, 0.01, -12.0), Vec3::Y, Vec3::NEG_Z);
    set_mode(&mut app, mode);
    command(&mut app, |c| c.movement = Vec3::Z);
    let (mut worst, mut furthest) = (0.0f32, f32::MAX);
    for _ in 0..300 {
        ticks(&mut app, 1);
        worst = worst.max(penetration(&mut app));
        furthest = furthest.min(booted_state(&mut app).1.z);
    }
    assert!(worst < 0.06, "capsule embedded {worst} m");
    // The sill spans z = -15..-14.7; standing on it is fine, beyond is outside.
    assert!(furthest > -15.0, "got outside the bow, z = {furthest}");
}

#[test]
fn wandering_never_embeds() {
    let mut app = app();
    let mut rng = 0x9e3779b9u32;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        rng as f32 / u32::MAX as f32
    };
    let mut worst = 0.0f32;
    for _ in 0..60 {
        let yaw = (next() - 0.5) * 3.0;
        let movement = Vec3::new(next() * 2.0 - 1.0, 0.0, next() * 2.0 - 1.0);
        command(&mut app, |c| {
            c.add_look(Vec2::new(yaw, 0.0));
            c.sprint = true;
        });
        worst = worst.max(walk_checked(&mut app, movement, 30));
    }
    let (_, feet, up, _) = booted_state(&mut app);
    assert!(
        worst < 0.06,
        "capsule embedded {worst} m (ended at {feet}, up {up})"
    );
}

#[test]
fn skids_when_ship_accelerates_beyond_shear() {
    let mut tuning = Tuning::default();
    tuning.ship_max_accel = 30.0;
    tuning.lurch_delta_v = 10.0;
    let mut app = app_with(tuning);
    ticks(&mut app, 5);
    lurch(&mut app);
    ticks(&mut app, 3);
    let (_, _, _, velocity) = booted_state(&mut app);
    // Ship lurches toward -Z; we slide aft, toward +Z, relative to the deck.
    assert!(velocity.z > 0.5, "no skid, velocity {velocity}");
}

fn eva_near_ship(app: &mut App, velocity_rel: Vec3) {
    let frame = ship_frame(app);
    let position = frame.point_to_world(Vec3::new(0.0, 10.0, 0.0));
    let velocity = frame.point_velocity(position) + velocity_rel;
    set_mode(
        app,
        Mode::Eva {
            position,
            velocity,
            orientation: frame.rotation,
            pitch: 0.0,
        },
    );
}

/// Velocity relative to what flight assist holds still against (the ship).
fn relative_velocity(app: &mut App) -> Vec3 {
    use bevy::ecs::system::RunSystemOnce;
    let Mode::Eva {
        position, velocity, ..
    } = player(app).mode
    else {
        panic!("not EVA")
    };
    let ship = ship(app);
    let matched = app
        .world_mut()
        .run_system_once(
            move |bodies: hullbreakers::sim::bodies::Bodies, t: Res<Tuning>| {
                bodies
                    .matched_velocity(ship, position, t.match_surface_near, t.match_surface_far)
                    .unwrap()
            },
        )
        .unwrap();
    velocity - matched
}
#[test]
fn flight_assist_stops_when_keys_released() {
    let mut app = app();
    eva_near_ship(&mut app, Vec3::new(3.0, 0.0, -4.0));
    // 5 m/s at 10 m/s² thrust: stopped within a second.
    ticks(&mut app, 60);
    assert!(relative_velocity(&mut app).length() < 0.05);
}

#[test]
fn flight_assist_cruises_at_commanded_speed() {
    let tuning = Tuning::default();
    let mut app = app();
    eva_near_ship(&mut app, Vec3::ZERO);
    command(&mut app, |c| c.movement = Vec3::Z);
    ticks(&mut app, 120);
    let rel = relative_velocity(&mut app);
    let forward = ship_frame(&mut app).dir_to_world(Vec3::NEG_Z);
    assert!(
        (rel.dot(forward) - tuning.assist_max_speed).abs() < 0.3,
        "rel {rel}"
    );
}

#[test]
fn without_assist_momentum_is_kept() {
    let mut app = app();
    {
        let world = app.world_mut();
        world
            .query::<&mut Player>()
            .single_mut(world)
            .unwrap()
            .flight_assist = false;
    }
    let drift = Vec3::new(1.0, 0.0, 0.5);
    eva_near_ship(&mut app, drift);
    let Mode::Eva {
        velocity: before, ..
    } = player(&mut app).mode
    else {
        unreachable!()
    };
    ticks(&mut app, 30);
    let Mode::Eva {
        velocity: after, ..
    } = player(&mut app).mode
    else {
        unreachable!()
    };
    assert!(before.abs_diff_eq(after, 1e-4), "{before} -> {after}");
}

#[test]
fn riding_a_rock_out_of_the_field_keeps_rock_and_field_around_you() {
    use hullbreakers::sim::asteroids::Asteroid;
    let mut app = app_configured(Tuning::default(), true);
    let (rock, radius) = {
        let w = app.world_mut();
        let mut q = w.query::<(Entity, &Asteroid, &Position)>();
        q.iter(w)
            .filter(|(_, a, _)| a.radius > 4.0 && a.radius < 10.0)
            .min_by(|a, b| a.2.0.length().total_cmp(&b.2.0.length()))
            .map(|(e, a, _)| (e, a.radius))
            .unwrap()
    };
    let rock_pos = app.world().get::<Position>(rock).unwrap().0;
    let rock_vel = app.world().get::<LinearVelocity>(rock).unwrap().0;
    assist_off(&mut app);
    set_mode(
        &mut app,
        Mode::Eva {
            position: rock_pos + Vec3::Y * (radius * 1.6 + 2.0),
            velocity: rock_vel - Vec3::Y * 2.0,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        },
    );
    // Ride well past the downstream end of the ship's field box, then jump.
    ticks(&mut app, 60 * 40);
    assert_eq!(booted_state(&mut app).0, rock);
    command(&mut app, |c| c.presses.jump.press());
    ticks(&mut app, 2);
    let rock_after = app.world().get::<Position>(rock).unwrap().0;
    let Mode::Eva { position, .. } = player(&mut app).mode else {
        panic!("did not jump")
    };
    assert!(
        position.z > 420.0,
        "test should leave the ship's field box, z = {}",
        position.z
    );
    assert!(
        rock_after.distance(position) < 3.0 * radius,
        "rock was teleported to {rock_after}"
    );
    // And the field has followed the player.
    ticks(&mut app, 60 * 30);
    let Mode::Eva { position, .. } = player(&mut app).mode else {
        unreachable!()
    };
    let w = app.world_mut();
    let near = w
        .query_filtered::<&Position, With<Asteroid>>()
        .iter(w)
        .filter(|p| p.0.distance(position) < 250.0)
        .count();
    assert!(near > 20, "only {near} rocks within 250 m of the player");
}

#[test]
fn reference_body_is_chosen_with_crosshair_and_persists_while_booted() {
    let mut app = app();
    // A rock straight ahead of a player standing on the deck, visible through the open stern.
    let rock = spawn_rock(&mut app, Vec3::new(0.0, 1.6, 60.0), 3.0, Vec3::ZERO);
    let mode = booted(&mut app, Vec3::new(0.0, 0.01, 10.0), Vec3::Y, Vec3::Z);
    set_mode(&mut app, mode);
    command(&mut app, |c| c.presses.set_reference.press());
    ticks(&mut app, 1);
    assert_eq!(
        player(&mut app).reference_body,
        Some(rock),
        "{}",
        last_event(&mut app)
    );
    // Still booted on the ship, the explicit choice sticks.
    ticks(&mut app, 60);
    assert_eq!(booted_state(&mut app).0, ship(&mut app));
    assert_eq!(player(&mut app).reference_body, Some(rock));
}

#[test]
fn assisted_reel_with_swing_is_smooth_and_arrives_gently() {
    let tuning = Tuning::default();
    let mut app = app();
    let frame = ship_frame(&mut app);
    let position = frame.point_to_world(Vec3::new(2.6, 1.5, 30.0));
    // Swinging sideways at 5 m/s when we start reeling.
    let velocity = frame.point_velocity(position) + frame.dir_to_world(Vec3::X * 5.0);
    set_mode(
        &mut app,
        Mode::Eva {
            position,
            velocity,
            orientation: frame.rotation,
            pitch: 0.0,
        },
    );
    command(&mut app, |c| c.presses.main.press());
    ticks(&mut app, 1);
    let anchor_local = player(&mut app).grapple.expect("hit the stern").anchor;
    command(&mut app, |c| c.reel_in = true);
    let (mut flips, mut last_taut, mut worst_arrival) = (0, false, 0.0f32);
    for _ in 0..240 {
        ticks(&mut app, 1);
        let p = player(&mut app);
        let g = p.grapple.expect("rope held");
        let taut = g.tension > tuning.player_mass * 2.0;
        flips += (taut != last_taut) as u32;
        last_taut = taut;
        if let Mode::Eva {
            position, velocity, ..
        } = p.mode
        {
            let frame = ship_frame(&mut app);
            if frame.point_to_world(anchor_local).distance(position) < 2.5 {
                worst_arrival =
                    worst_arrival.max((velocity - frame.point_velocity(position)).length());
            }
        }
    }
    assert!(flips <= 6, "rope went taut/slack {flips} times in 4 s");
    assert!(
        worst_arrival < tuning.attach_max_speed,
        "arrived at {worst_arrival} m/s"
    );
}

fn set_reference(app: &mut App, body: Entity) {
    let world = app.world_mut();
    world
        .query::<&mut Player>()
        .single_mut(world)
        .unwrap()
        .reference_body = Some(body);
}

#[test]
fn assist_holds_still_beside_a_tumbling_rock_instead_of_orbiting() {
    let mut app = app();
    let rock = spawn_rock(
        &mut app,
        Vec3::new(0.0, 0.0, 80.0),
        6.0,
        Vec3::new(0.2, 0.4, 0.1),
    );
    // 30 m from the surface, at rest relative to the rock's center.
    let start = Vec3::new(0.0, 36.0, 80.0);
    set_mode(
        &mut app,
        Mode::Eva {
            position: start,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        },
    );
    set_reference(&mut app, rock);
    ticks(&mut app, 180);
    let Mode::Eva { position, .. } = player(&mut app).mode else {
        panic!()
    };
    let rock_pos = app.world().get::<Position>(rock).unwrap().0;
    let drift = (position - rock_pos).distance(start - Vec3::new(0.0, 0.0, 80.0));
    // Co-rotating would have carried us ~0.46 rad/s * 36 m * 3 s ≈ 50 m around.
    assert!(
        drift < 0.5,
        "drifted {drift} m relative to the rock's center"
    );
}

#[test]
fn assist_matches_the_surface_up_close() {
    let tuning = Tuning::default();
    let mut app = app();
    let spin = Vec3::new(0.0, 0.5, 0.0);
    let rock = spawn_rock(&mut app, Vec3::new(0.0, 0.0, 80.0), 6.0, spin);
    // 1 m off the equator, moving with the surface beneath.
    let offset = Vec3::new(7.0, 0.0, 0.0);
    let surface_velocity = spin.cross(Vec3::X * 6.0);
    set_mode(
        &mut app,
        Mode::Eva {
            position: Vec3::new(0.0, 0.0, 80.0) + offset,
            velocity: surface_velocity,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        },
    );
    set_reference(&mut app, rock);
    assert!(tuning.match_surface_near > 1.0);
    ticks(&mut app, 30);
    let Mode::Eva { position, .. } = player(&mut app).mode else {
        panic!("landed?")
    };
    let rock_pos = app.world().get::<Position>(rock).unwrap().0;
    let height = (position - rock_pos).length() - 6.0;
    // Still hovering over the moving surface, not left behind by it or pressed into it.
    assert!((height - 1.0).abs() < 0.3, "height above surface {height}");
}

/// Walks over a faceted rock and returns the largest per-tick turn of `up`
/// (ignoring deliberate relocations) and the total turn.
fn walk_faceted_rock(tuning: Tuning) -> (f32, f32) {
    let mut app = app_with(tuning);
    assist_off(&mut app);
    let rock = app
        .world_mut()
        .spawn((
            hullbreakers::sim::bodies::dynamic_mass(
                &hullbreakers::sim::asteroids::asteroid_collider(7, 6.0),
                hullbreakers::sim::asteroids::ROCK_DENSITY,
            ),
            hullbreakers::sim::asteroids::asteroid_collider(7, 6.0),
            RigidBody::Dynamic,
            SleepingDisabled,
            hullbreakers::sim::bodies::PreviousVelocity::default(),
            Transform::from_translation(Vec3::new(0.0, 0.0, 80.0)),
        ))
        .id();
    ticks(&mut app, 2);
    set_mode(
        &mut app,
        Mode::Eva {
            position: Vec3::new(0.0, 0.0, 66.0),
            velocity: Vec3::Z * 1.5,
            orientation: Quat::from_rotation_arc(Vec3::Y, Vec3::NEG_Z),
            pitch: 0.0,
        },
    );
    for _ in 0..900 {
        ticks(&mut app, 1);
        if matches!(player(&mut app).mode, Mode::Booted { .. }) {
            break;
        }
    }
    assert_eq!(booted_state(&mut app).0, rock, "never landed on the rock");

    command(&mut app, |c| c.movement = Vec3::Z);
    let (mut last_up, mut worst, mut total) = (booted_state(&mut app).2, 0.0f32, 0.0f32);
    for _ in 0..240 {
        ticks(&mut app, 1);
        let (body, _, up, _) = booted_state(&mut app);
        assert_eq!(body, rock);
        let step = last_up.angle_between(up).to_degrees();
        // Wall climbs and edge wraps relocate the capsule on purpose.
        if step < 45.0 {
            worst = worst.max(step);
        }
        total += step;
        last_up = up;
    }
    (worst, total)
}

/// Walking over a faceted rock turns the body smoothly: `up` follows the
/// surface at a limited rate instead of snapping at every face.
#[test]
fn up_turns_smoothly_walking_over_a_faceted_rock() {
    let (worst, total) = walk_faceted_rock(Tuning::default());
    assert!(total > 10.0, "walked without the surface turning ({total}°)");
    assert!(worst < 6.0, "up turned {worst}° in one tick");
    // Following the surface instantly snaps at the creases.
    let mut snappy = Tuning::default();
    snappy.surface_follow_rate = 1e4;
    let (snap_worst, _) = walk_faceted_rock(snappy);
    assert!(
        snap_worst > 2.0 * worst,
        "smoothing made no difference ({snap_worst}° vs {worst}°)"
    );
}

/// Two runs from startup with the same inputs give bit-identical results.
/// Necessary for rollback, not sufficient: it doesn't catch state that is
/// identical across fresh runs but not restored by a rollback (e.g. Avian's
/// solver warm-start data). That needs a snapshot/restore test once there is
/// a snapshot to restore.
#[test]
fn simulation_is_deterministic() {
    fn run() -> (Player, Vec<(Vec3, Quat, Vec3)>) {
        let mut app = app_configured(Tuning::default(), true);
        // Walk and look around, jump off, fly, grapple, reel in.
        let script: &[(usize, fn(&mut PlayerInput))] = &[
            (40, |c| c.movement = Vec3::new(0.3, 0.0, 1.0)),
            (30, |c| c.add_look(Vec2::new(0.4, -0.2))),
            (20, |c| c.presses.jump.press()),
            (60, |c| c.movement = Vec3::new(0.0, 0.5, 1.0)),
            (5, |c| c.presses.main.press()),
            (90, |c| c.reel_in = true),
        ];
        for (n, f) in script {
            command(&mut app, f);
            ticks(&mut app, *n);
        }
        let bodies = app
            .world_mut()
            .query::<(&Position, &Rotation, &LinearVelocity)>()
            .iter(app.world())
            .map(|(p, r, v)| (p.0, r.0, v.0))
            .collect();
        (player(&mut app), bodies)
    }
    let (a, b) = (run(), run());
    assert_eq!(a.0, b.0, "player state diverged");
    assert!(a.1.len() > 300);
    assert!(a.1 == b.1, "body state diverged");
}

/// A held input is read every tick; a press in it acts once.
#[test]
fn a_press_acts_once_however_often_the_input_is_read() {
    let mut app = app();
    ticks(&mut app, 5);
    command(&mut app, |c| c.presses.toggle_boots.press());
    ticks(&mut app, 30);
    let toggles = player(&mut app)
        .events
        .iter()
        .filter(|(_, e)| matches!(e, hullbreakers::sim::events::PlayerEvent::BootsToggled(_)))
        .count();
    assert_eq!(toggles, 1);
    assert!(!player(&mut app).boots_on);
}

/// A free-tumbling rock (irregular shape, real Avian physics) stays on the
/// motion sampled from it: nothing needs re-sending until something touches
/// it. This is what makes extrapolated replication cheap.
#[test]
fn free_rock_follows_its_sampled_motion() {
    use hullbreakers::sim::SimTick;
    use hullbreakers::sim::bodies::BodyInertia;
    use hullbreakers::sim::frame::BodyFrame;
    use hullbreakers::sim::motion::Motion;
    let mut app = app();
    let collider = hullbreakers::sim::asteroids::asteroid_collider(11, 8.0);
    let rock = app
        .world_mut()
        .spawn((
            hullbreakers::sim::bodies::dynamic_mass(
                &collider,
                hullbreakers::sim::asteroids::ROCK_DENSITY,
            ),
            collider,
            RigidBody::Dynamic,
            SleepingDisabled,
            LinearVelocity(Vec3::new(0.3, -0.2, 15.0)),
            AngularVelocity(Vec3::new(0.25, -0.1, 0.18)),
            Position(Vec3::new(40.0, 0.0, 200.0)),
        ))
        .id();
    ticks(&mut app, 2);
    let state = |app: &mut App| {
        let w = app.world();
        let e = w.entity(rock);
        let inertia = *e.get::<BodyInertia>().unwrap();
        let frame = BodyFrame {
            position: e.get::<Position>().unwrap().0,
            rotation: e.get::<Rotation>().unwrap().0,
            center_of_mass: e.get::<Position>().unwrap().0
                + e.get::<Rotation>().unwrap().0 * inertia.center_of_mass,
            linear_velocity: e.get::<LinearVelocity>().unwrap().0,
            angular_velocity: e.get::<AngularVelocity>().unwrap().0,
        };
        (w.resource::<SimTick>().0, frame, inertia)
    };
    let (tick0, frame0, inertia) = state(&mut app);
    let motion = Motion::sample(tick0, &frame0);
    ticks(&mut app, 600);
    let (tick, actual, _) = state(&mut app);
    let predicted = motion.frame_at(tick, inertia.center_of_mass);
    let drift = (predicted.center_of_mass - actual.center_of_mass).length();
    let turn = (predicted.rotation * actual.rotation.inverse())
        .to_scaled_axis()
        .length();
    // At the rock's ~8 m extent, after 10 s.
    assert!(drift + turn * 8.0 < 0.01, "drift {drift} m, turn {turn} rad");
}
