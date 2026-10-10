//! Grappling another player: a two-ended rope that both of them feel.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use space_game::sim::SimPlugin;
use space_game::sim::events::{Detach, PlayerEvent};
use space_game::sim::input::PlayerInput;
use space_game::sim::player::{Mode, Player, new_player};
use space_game::sim::ship::Ship;
use space_game::sim::tuning::Tuning;

/// A ship, the default player (A), and a second player (B).
fn two_players() -> (App, Entity, Entity) {
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
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
    app.finish();
    app.cleanup();
    for _ in 0..3 {
        app.update();
    }
    let world = app.world_mut();
    let ship = world
        .query_filtered::<Entity, With<Ship>>()
        .single(world)
        .unwrap();
    let a = world
        .query_filtered::<Entity, With<Player>>()
        .single(world)
        .unwrap();
    let b = world.spawn(new_player(ship, 1)).id();
    for _ in 0..3 {
        app.update();
    }
    (app, a, b)
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn player(app: &App, e: Entity) -> Player {
    app.world().get::<Player>(e).unwrap().clone()
}

fn set(app: &mut App, e: Entity, f: impl FnOnce(&mut Player)) {
    f(&mut app.world_mut().get_mut::<Player>(e).unwrap());
}

fn input(app: &mut App, e: Entity, f: impl FnOnce(&mut PlayerInput)) {
    f(&mut app.world_mut().get_mut::<PlayerInput>(e).unwrap());
}

fn eva(position: Vec3, facing: Vec3) -> Mode {
    Mode::Eva {
        position,
        velocity: Vec3::ZERO,
        orientation: Transform::IDENTITY.looking_to(facing, Vec3::Y).rotation,
        pitch: 0.0,
    }
}

fn position(app: &App, e: Entity) -> Vec3 {
    match player(app, e).mode {
        Mode::Eva { position, .. } => position,
        Mode::Booted { .. } => panic!("expected EVA"),
    }
}

/// Floating apart in open space, assist off, facing each other.
fn floating_pair() -> (App, Entity, Entity) {
    let (mut app, a, b) = two_players();
    // Well above the ship.
    let (pa, pb) = (Vec3::new(0.0, 30.0, 0.0), Vec3::new(10.0, 30.0, 0.0));
    set(&mut app, a, |p| {
        p.mode = eva(pa, Vec3::X);
        p.flight_assist = false;
    });
    set(&mut app, b, |p| {
        p.mode = eva(pb, -Vec3::X);
        p.flight_assist = false;
    });
    ticks(&mut app, 2);
    (app, a, b)
}

#[test]
fn grapple_hooks_another_player() {
    let (mut app, a, b) = floating_pair();
    input(&mut app, a, |i| i.presses.grapple.press());
    ticks(&mut app, 1);
    let grapple = player(&app, a).grapple.expect("missed the other player");
    assert_eq!(grapple.target, b);
    // Center to center.
    assert!((grapple.length - 10.0).abs() < 0.05, "length {}", grapple.length);
}

/// Unassisted, the winch's pull is shared by mass: equal players move
/// equally toward each other.
#[test]
fn reeling_in_a_free_player_pulls_both_equally() {
    let (mut app, a, b) = floating_pair();
    input(&mut app, a, |i| i.presses.grapple.press());
    ticks(&mut app, 1);
    input(&mut app, a, |i| i.reel_in = true);
    let (a0, b0) = (position(&app, a), position(&app, b));
    let reel_speed = Tuning::default().reel_speed;
    let mut fastest = 0.0f32;
    for _ in 0..30 {
        let before = position(&app, b).x - position(&app, a).x;
        ticks(&mut app, 1);
        let after = position(&app, b).x - position(&app, a).x;
        fastest = fastest.max((before - after) * 60.0);
    }
    // The winch never closes faster than it reels.
    assert!(fastest <= reel_speed * 1.05, "closed at {fastest} m/s");
    let (a1, b1) = (position(&app, a), position(&app, b));
    let (moved_a, moved_b) = (a1.x - a0.x, b0.x - b1.x);
    assert!(moved_a > 1.0, "A moved {moved_a}");
    assert!((moved_a - moved_b).abs() < 0.1, "A moved {moved_a}, B moved {moved_b}");
}

/// In play, with flight assist on: the reeler flies in with the winch and
/// stops short of the other player, who holds station on their jetpack.
#[test]
fn assisted_reel_to_a_player_arrives_without_passing_through() {
    let (mut app, a, b) = floating_pair();
    for e in [a, b] {
        set(&mut app, e, |p| p.flight_assist = true);
    }
    input(&mut app, a, |i| i.presses.grapple.press());
    ticks(&mut app, 1);
    input(&mut app, a, |i| i.reel_in = true);
    let mut closest = f32::MAX;
    for _ in 0..180 {
        ticks(&mut app, 1);
        closest = closest.min(position(&app, b).x - position(&app, a).x);
    }
    let gap = position(&app, b).x - position(&app, a).x;
    assert!(closest > 0.4, "came within {closest} m (or passed through)");
    assert!(gap < 2.0, "stopped {gap} m away");
}

#[test]
fn reeling_yanks_a_booted_player_off_the_hull() {
    let (mut app, a, b) = two_players();
    // B stands on the dorsal spine (top of the ship); A floats above, looking down.
    set(&mut app, b, |p| {
        p.mode = Mode::Booted {
            body: p.mode.body().unwrap(),
            feet: Vec3::new(0.0, 4.11, 2.0),
            up: Vec3::Y,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
    });
    ticks(&mut app, 10);
    let b_center = {
        let world = app.world();
        world.get::<Position>(b).unwrap().0
    };
    // Looking straight down, with the eye (0.72 m up the body's axis, which
    // now points along +Z) right above B.
    let down = Transform::IDENTITY.looking_to(Vec3::NEG_Y, Vec3::Z).rotation;
    let eye_offset = down * Vec3::Y * 0.72;
    set(&mut app, a, |p| {
        p.mode = Mode::Eva {
            position: b_center + Vec3::new(0.0, 12.0, 0.0) - eye_offset,
            velocity: Vec3::ZERO,
            orientation: down,
            pitch: 0.0,
        };
        p.flight_assist = false;
    });
    ticks(&mut app, 2);
    input(&mut app, a, |i| i.presses.grapple.press());
    ticks(&mut app, 1);
    assert_eq!(player(&app, a).grapple.map(|g| g.target), Some(b));
    input(&mut app, a, |i| i.reel_in = true);
    ticks(&mut app, 30);
    let b_now = player(&app, b);
    let Mode::Eva { velocity, .. } = b_now.mode else {
        panic!("B's boots held against the winch");
    };
    // Yanked free, B is pulled by the winch as a free 120 kg body (sharing
    // the winch's speed with A), not flung with an anchor-sized impulse.
    assert!(velocity.length() < 8.0, "B flung at {} m/s", velocity.length());
    assert!(
        b_now
            .events
            .iter()
            .any(|(_, e)| matches!(e, PlayerEvent::Detached(Detach::Yanked(_)))),
        "B wasn't yanked: {:?}",
        b_now.events
    );
}

/// Two players with ropes on each other, drifting apart: the ropes catch
/// them and they come to rest together, rather than bouncing forever.
#[test]
fn mutual_ropes_catch_and_settle() {
    let (mut app, a, b) = floating_pair();
    input(&mut app, a, |i| i.presses.grapple.press());
    input(&mut app, b, |i| i.presses.grapple.press());
    ticks(&mut app, 1);
    assert_eq!(player(&app, a).grapple.map(|g| g.target), Some(b));
    assert_eq!(player(&app, b).grapple.map(|g| g.target), Some(a));
    // Drift apart at 2 m/s.
    for (e, v) in [(a, -1.0), (b, 1.0)] {
        set(&mut app, e, |p| {
            if let Mode::Eva { velocity, .. } = &mut p.mode {
                *velocity = Vec3::X * v;
            }
        });
    }
    ticks(&mut app, 180);
    let speed = |e| match player(&app, e).mode {
        Mode::Eva { velocity, .. } => velocity.x,
        _ => panic!("expected EVA"),
    };
    // Caught, not bounced back: the rope's catch is slightly springy (as
    // with any rope), far from the 2 m/s a full rebound would give.
    let separating = speed(b) - speed(a);
    assert!(separating.abs() < 0.8, "relative speed {separating} m/s after the catch");
    let gap = position(&app, b).x - position(&app, a).x;
    assert!(gap < 10.5, "ropes stretched to {gap} m");
}
