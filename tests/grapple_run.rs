//! The grapple run's course, flown by two scripted pilots through the sim:
//! one on the jetpack alone, braking for each hairpin as well as it can, and
//! one swinging round each hairpin's pylon on the rope. The rope must be
//! the fast line.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::bots::Robot;
use hullbreakers::sim::design::design;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::mission::{Director, Mission, Phase, ScenarioKind};
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::scenarios::grapple_run::{COURSE, COURSE_AT, gates};
use hullbreakers::sim::tuning::Tuning;

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
    app.world_mut().resource_mut::<Director>().robots = false;
    app.finish();
    app.cleanup();
    for _ in 0..4 {
        app.update();
    }
    app
}

fn me(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, (With<Player>, Without<Robot>)>().single(world).unwrap()
}

/// The pylon each hairpin turns round (after gate k, before gate k + 1), in
/// the course's coordinates: as `assets/ships/course/gates.ron` places them.
const PYLONS: [Vec3; 9] = [
    Vec3::new(4.0, 0.0, 30.0),
    Vec3::new(12.0, 0.0, -10.0),
    Vec3::new(20.0, 0.0, 30.0),
    Vec3::new(24.0, 4.0, -10.0),
    Vec3::new(20.0, 8.0, 30.0),
    Vec3::new(12.0, 8.0, -10.0),
    Vec3::new(4.0, 8.0, 30.0),
    Vec3::new(-4.0, 8.0, -10.0),
    Vec3::new(-8.0, 4.0, 30.0),
];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Leg {
    /// Flying at the next gate.
    Gate,
    /// Past a gate, flying up the lane to abreast of the hairpin's pylon.
    ToPylon,
    /// On the rope, swinging round it.
    Swinging,
}

/// Flies the course from rest 12 m before the first gate; returns the time
/// (s) through the finish, or None if it took over 3 minutes.
fn fly(rope: bool) -> Option<f32> {
    let mut app = app();
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
    for _ in 0..3 {
        app.update();
    }
    let tuning = Tuning::default();
    let course = design(COURSE);
    let gates: Vec<_> = gates(course).into_iter().map(|i| course.devices[i].clone()).collect();
    let e = me(&mut app);
    let start = COURSE_AT + Vec3::new(0.0, 0.0, -12.0);
    {
        let mut p = app.world_mut().get_mut::<Player>(e).unwrap();
        p.mode = Mode::Eva {
            position: start,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
    }
    let (mut next, mut leg) = (0usize, Leg::Gate);
    let mut last = start - COURSE_AT;
    for tick in 0..180 * 60 {
        let p = app.world().get::<Player>(e).unwrap().clone();
        let Mode::Eva { position, velocity, .. } = p.mode else {
            panic!("landed on something: {:?} (gate {next}, {leg:?})", p.mode);
        };
        let here = position - COURSE_AT;
        if gates[next].passed_through(last, here) {
            next += 1;
            if next == gates.len() {
                let won = app.world_mut().query::<&Mission>().single(app.world()).unwrap().phase == Phase::Won;
                assert!(won, "through the finish, but the run isn't won");
                return Some(tick as f32 / 60.0);
            }
            leg = if rope { Leg::ToPylon } else { Leg::Gate };
        }
        last = here;
        let gate = &gates[next];
        let (face, at) = (gate.face, gate.at);
        // Where to fly, how fast, and the hand.
        let mut fire = false;
        let (target, speed) = match leg {
            Leg::Gate => {
                // Line up behind the gate, then through it.
                let entry = at - face * 6.0;
                let behind = (here - at).dot(face) < -2.0;
                let off_axis = (here - at).reject_from(face).length();
                let target = if behind && off_axis < 1.5 { at + face * 3.0 } else { entry };
                // The jetpack has to make each turn: slow to what it can
                // turn at (v² = a·r, r about 6 m) by the time it's there.
                let heading = velocity.normalize_or_zero();
                let turn = 1.0 - heading.dot((target - here).normalize_or_zero());
                let d = here.distance(target);
                let speed = if turn > 0.3 { (9.0f32.powi(2) + 2.0 * 15.0 * d).sqrt().min(20.0) } else { 20.0 };
                (target, speed)
            }
            Leg::ToPylon => {
                let gone = gates[next - 1].clone();
                let pylon = PYLONS[next - 1];
                let abreast = gone.at + gone.face * (pylon - gone.at).dot(gone.face);
                if (abreast - here).dot(gone.face) < 0.5 {
                    fire = true;
                    leg = Leg::Swinging;
                    (pylon, 0.0)
                } else {
                    (abreast + gone.face * 5.0, 20.0)
                }
            }
            Leg::Swinging => {
                // Let go heading the way the next gate faces.
                if velocity.normalize_or_zero().dot(face) > 0.97 {
                    fire = p.grapple.is_some();
                    leg = Leg::Gate;
                }
                (here + velocity, 20.0)
            }
        };
        let look = (target - here).normalize_or(Vec3::Z);
        let up = if look.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
        {
            let mut player = app.world_mut().get_mut::<Player>(e).unwrap();
            if let Mode::Eva { orientation, pitch, .. } = &mut player.mode {
                *orientation = Transform::IDENTITY.looking_to(look, up).rotation;
                *pitch = 0.0;
            }
        }
        {
            let mut input = app.world_mut().get_mut::<PlayerInput>(e).unwrap();
            input.movement = Vec3::Z * (speed / (tuning.assist_max_speed * tuning.boost_multiplier));
            input.sprint = true;
            if fire {
                input.presses.main.press();
            }
        }
        app.update();
        if rope && leg == Leg::Swinging && app.world().get::<Player>(e).unwrap().grapple.is_none() && !fire {
            panic!("the rope went after gate {next}: {:?}", app.world().get::<Player>(e).unwrap().events.iter().collect::<Vec<_>>());
        }
    }
    None
}

/// Over the whole course, swinging round the pylons beats braking for the
/// hairpins on the jetpack by a good margin (measured: 24.4 s against
/// 31.3 s).
#[test]
fn the_rope_is_the_fast_line() {
    let jetpack = fly(false).expect("the jetpack pilot never finished");
    let rope = fly(true).expect("the rope pilot never finished");
    eprintln!("the course: {jetpack:.1} s on the jetpack, {rope:.1} s on the rope");
    assert!(rope < jetpack * 0.85, "the rope ({rope:.1} s) isn't much faster than the jetpack ({jetpack:.1} s)");
}
