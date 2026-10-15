//! The grapple run's race course, flown by two scripted pilots through the
//! sim, flight assist off and the jetpack boosted: one on the jetpack alone,
//! braking for each turn as well as it can, and one swinging round each
//! turn's anchor rock on the rope. Both must make it round inside the time
//! limit, and the rope must be the fast line.

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
use hullbreakers::sim::scenarios::grapple_run::{COURSE, COURSE_AT, TIME_LIMIT, anchors, apexes, rings};
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

/// The fastest either pilot flies (m/s), and how fast the jetpack pilot
/// takes a turn.
const TOP_SPEED: f32 = 45.0;
const TURN_SPEED: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Leg {
    /// Flying at the next ring.
    Ring,
    /// Past a ring, on the jetpack: down the leg to the turn's apex,
    /// slowing to take it.
    ToApex,
    /// Past a ring, flying down the leg toward the turn's anchor.
    ToAnchor,
    /// On the rope, swinging round it.
    Swinging,
}

/// Flies the course from rest 20 m behind the start ring; returns the time
/// (s) from the start ring through the finish, or None if the clock ran
/// out first.
fn fly(rope: bool) -> Option<f32> {
    let mut app = app();
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
    for _ in 0..3 {
        app.update();
    }
    let tuning = Tuning::default();
    let course = design(COURSE);
    let rings: Vec<_> = rings(course).into_iter().map(|i| course.devices[i].clone()).collect();
    let anchors = anchors();
    let apexes = apexes(course);
    let e = me(&mut app);
    let start = COURSE_AT - rings[0].face * 20.0;
    {
        let mut p = app.world_mut().get_mut::<Player>(e).unwrap();
        p.flight_assist = false;
        p.mode = Mode::Eva {
            position: start,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
    }
    // The jetpack's whole thrust, boosted, in one tick.
    let max_dv = tuning.thrust_accel * tuning.boost_multiplier / 60.0;
    let brake = tuning.thrust_accel * tuning.boost_multiplier * 0.6;
    let (mut next, mut leg, mut started) = (0usize, Leg::Ring, None);
    let mut last = start - COURSE_AT;
    for tick in 0..(TIME_LIMIT * 60.0) as u32 {
        let p = app.world().get::<Player>(e).unwrap().clone();
        let Mode::Eva { position, velocity, .. } = p.mode else {
            panic!("landed on something: {:?} (ring {next}, {leg:?})", p.mode);
        };
        let here = position - COURSE_AT;
        if rings[next].passed_through(last, here) {
            started.get_or_insert(tick);
            next += 1;
            if next == rings.len() {
                let won = app.world_mut().query::<&Mission>().single(app.world()).unwrap().phase == Phase::Won;
                assert!(won, "through the finish, but the run isn't won");
                return Some((tick - started.unwrap()) as f32 / 60.0);
            }
            // The turn between the ring just flown and the next.
            leg = if rope { Leg::ToAnchor } else { Leg::ToApex };
        }
        last = here;
        let ring = &rings[next];
        let (face, at) = (ring.face, ring.at);
        // Where to fly and how fast (in the course's coordinates), where
        // to look, and the hand.
        let mut fire = false;
        let (want, look) = match leg {
            Leg::Ring => {
                // Line up behind the ring, then through it, slowing (on the
                // jetpack alone) to take the turn after it.
                let entry = at - face * 40.0;
                let behind = (here - at).dot(face) < -5.0;
                let off_axis = (here - at).reject_from(face).length();
                let aim = if !behind || off_axis > 6.0 && (here - entry).dot(face) < 0.0 { entry } else { at + face * 20.0 };
                let dir = (aim - here).normalize_or(face);
                (dir * TOP_SPEED, dir)
            }
            Leg::ToApex => {
                let apex = apexes[next - 1];
                let d = here.distance(apex);
                if d < 8.0 || (apex - here).dot(rings[next - 1].face) < 0.0 {
                    leg = Leg::Ring;
                }
                let dir = (apex - here).normalize_or(face);
                (dir * (TURN_SPEED.powi(2) + 2.0 * brake * d).sqrt().min(TOP_SPEED), dir)
            }
            Leg::ToAnchor => {
                // Down the leg (the line from the last ring through the
                // apex) until the anchor's abeam, then hook it.
                let gone = &rings[next - 1];
                let anchor = anchors[next - 1].0 - COURSE_AT;
                let to = anchor - here;
                if to.dot(velocity) <= 0.0 {
                    fire = true;
                    leg = Leg::Swinging;
                    (velocity, to.normalize())
                } else {
                    (gone.face * TOP_SPEED, to.normalize())
                }
            }
            Leg::Swinging => {
                // Let go heading for the next ring's entry.
                let entry = at - face * 40.0;
                if velocity.normalize_or_zero().dot((entry - here).normalize_or_zero()) > 0.99 {
                    fire = p.grapple.is_some();
                    leg = Leg::Ring;
                }
                (velocity, velocity.normalize_or(face))
            }
        };
        // Thrust toward the velocity wanted, in the view's frame (none on
        // the rope).
        let thrust = if leg == Leg::Swinging { Vec3::ZERO } else { (want - velocity) / max_dv };
        let thrust = thrust.clamp_length_max(1.0);
        let up = if look.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
        let view = Transform::IDENTITY.looking_to(look, up).rotation;
        {
            let mut player = app.world_mut().get_mut::<Player>(e).unwrap();
            if let Mode::Eva { orientation, pitch, .. } = &mut player.mode {
                *orientation = view;
                *pitch = 0.0;
            }
        }
        {
            let local = view.inverse() * thrust;
            let mut input = app.world_mut().get_mut::<PlayerInput>(e).unwrap();
            input.movement = Vec3::new(local.x, local.y, -local.z);
            input.sprint = true;
            if fire {
                input.presses.main.press();
            }
        }
        app.update();
        if rope && leg == Leg::Swinging && app.world().get::<Player>(e).unwrap().grapple.is_none() && !fire {
            panic!("the rope went after ring {next}: {:?}", app.world().get::<Player>(e).unwrap().events.iter().collect::<Vec<_>>());
        }
    }
    None
}

/// Over the whole course, swinging round the anchors beats braking for
/// the turns on the jetpack by a good margin (measured: 64.9 s against
/// 84.1 s, both at most 45 m/s).
#[test]
fn the_rope_is_the_fast_line() {
    let jetpack = fly(false).expect("the jetpack pilot ran out of time");
    let rope = fly(true).expect("the rope pilot ran out of time");
    eprintln!("the course: {jetpack:.1} s on the jetpack, {rope:.1} s on the rope");
    assert!(rope < jetpack * 0.85, "the rope ({rope:.1} s) isn't much faster than the jetpack ({jetpack:.1} s)");
}
