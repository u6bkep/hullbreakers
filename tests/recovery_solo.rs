//! Recovery for a lone player, flown by a scripted pilot through the sim:
//! out to a pod where it drifts, a rope on it, home round our ship and into
//! the hold's mouth, within its air.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::bots::Robot;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::mission::{Director, Mission, Phase, ScenarioKind};
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::scenarios::recovery::{Pod, PodKind};
use hullbreakers::sim::ship::HomeShip;
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::TICK_HZ;

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

/// Steers the player: looking at `target` (world), thrusting toward it at
/// up to `speed` (m/s), boosted.
fn steer(app: &mut App, e: Entity, here: Vec3, target: Vec3, speed: f32, fire: bool) {
    let tuning = Tuning::default();
    let look = (target - here).normalize_or(Vec3::Z);
    let up = if look.y.abs() > 0.9 { Vec3::X } else { Vec3::Y };
    if let Mode::Eva { orientation, pitch, .. } = &mut app.world_mut().get_mut::<Player>(e).unwrap().mode {
        *orientation = Transform::IDENTITY.looking_to(look, up).rotation;
        *pitch = 0.0;
    }
    let mut input = app.world_mut().get_mut::<PlayerInput>(e).unwrap();
    input.movement = Vec3::Z * (speed / (tuning.assist_max_speed * tuning.boost_multiplier)).min(1.0);
    input.sprint = true;
    if fire {
        input.presses.main.press();
    }
}

/// Fetches the pod of `kind` (the first of it) alone, from our hold's door;
/// returns the seconds it took to get it home, or None if its air ran out.
fn fetch(kind: PodKind) -> Option<f32> {
    let mut app = app();
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Recovery);
    for _ in 0..3 {
        app.update();
    }
    let ship = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap()
    };
    let (pod, name) = {
        let world = app.world_mut();
        let (e, p) = world.query::<(Entity, &Pod)>().iter(world).find(|(_, p)| p.kind == kind).unwrap();
        (e, p.name())
    };
    let e = me(&mut app);
    let ours = |app: &App| {
        let (p, r) = (app.world().get::<Position>(ship).unwrap().0, app.world().get::<Rotation>(ship).unwrap().0);
        move |local: Vec3| p + r * local
    };
    let door = ours(&app)(Vec3::new(0.0, 1.6, 18.0));
    {
        let mut p = app.world_mut().get_mut::<Player>(e).unwrap();
        p.mode = Mode::Eva {
            position: door,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
        p.reference_body = Some(ship);
        // Boots off: in the open, nothing to land on.
        p.boots_on = false;
    }
    let start = app.world().resource::<hullbreakers::sim::SimTick>().0;
    // Out: aft of our stern and round to the side it's on, if it's
    // forward of us, then at it.
    let pod_local = {
        let (p, r) = (app.world().get::<Position>(ship).unwrap().0, app.world().get::<Rotation>(ship).unwrap().0);
        r.inverse() * (app.world().get::<Position>(pod).unwrap().0 - p)
    };
    let mut out: Vec<Vec3> = Vec::new();
    if pod_local.z < 20.0 {
        let side = if pod_local.x < 0.0 { -30.0 } else { 30.0 };
        out = vec![Vec3::new(0.0, 1.6, 30.0), Vec3::new(side, 1.6, 30.0), Vec3::new(side, 1.6, pod_local.z.max(-30.0))];
    }
    let mut out: Vec<Vec3> = out.into_iter().map(|l| ours(&app)(l)).collect();
    let mut leg = 0;
    let mut route: Vec<Vec3> = Vec::new();
    for tick in 0..300 * 60 {
        let p = app.world().get::<Player>(e).unwrap().clone();
        let Mode::Eva { position: here, .. } = p.mode else {
            panic!("landed on something: {:?} (leg {leg})", p.mode);
        };
        let at = app.world().get::<Position>(pod).map(|p| p.0);
        let mission = app.world_mut().query::<&Mission>().single(app.world()).unwrap().clone();
        if mission.report.iter().any(|l| l.starts_with(&format!("{name} home"))) {
            return Some(tick as f32 / 60.0);
        }
        if mission.report.iter().any(|l| l.starts_with(&format!("{name} ran out"))) || mission.phase != Phase::Underway {
            eprintln!("{name}: lost at {:.0} s (leg {leg}): {:?}", tick as f32 / 60.0, mission.report);
            return None;
        }
        let local = |w: Vec3| app.world().get::<Rotation>(ship).unwrap().0.inverse() * (w - app.world().get::<Position>(ship).unwrap().0);
        match leg {
            // Out to it, and a rope on it from 8 m.
            0 if !out.is_empty() => {
                if here.distance(out[0]) < 3.0 {
                    out.remove(0);
                }
                if let Some(&w) = out.first() {
                    steer(&mut app, e, here, w, 14.0, false);
                }
            }
            0 => {
                let at = at.unwrap();
                if here.distance(at) < 8.0 {
                    steer(&mut app, e, here, at, 0.0, true);
                    leg = 1;
                } else {
                    steer(&mut app, e, here, at, 20.0, false);
                }
            }
            // Home: out to the side it's on, aft past our stern, in behind
            // the door, slowing as it comes.
            1 => {
                assert!(p.grapple.is_some_and(|g| g.target == pod), "the rope missed {name}");
                let side = if local(here).x < 0.0 { -30.0 } else { 30.0 };
                let to = ours(&app);
                route = vec![
                    to(Vec3::new(side, 1.6, local(here).z.min(10.0))),
                    to(Vec3::new(side, 1.6, 45.0)),
                    to(Vec3::new(0.0, 1.6, 45.0)),
                    to(Vec3::new(0.0, 1.6, 30.0)),
                    to(Vec3::new(0.0, 1.6, 8.0)),
                ];
                eprintln!("{name}: hooked at {:.0} s, {:.0} m out", tick as f32 / 60.0, here.distance(door));
                leg = 2;
                steer(&mut app, e, here, route[0], 0.0, false);
            }
            n => {
                if p.grapple.is_none() {
                    panic!("the rope's gone (leg {n}, me {:.1}): {:?}", local(here), p.events.iter().collect::<Vec<_>>());
                }
                if tick % 300 == 0 {
                    eprintln!(
                        "{:.0} s leg {n}: me {:.1}, it {:.1}, rope {:?}",
                        tick as f32 / 60.0,
                        local(here),
                        at.map(local).unwrap_or(Vec3::NAN),
                        p.grapple.map(|g| g.length)
                    );
                }
                let target = route[n - 2];
                let last = n - 2 == route.len() - 1;
                let d = here.distance(target);
                if d < 2.0 && !last {
                    leg += 1;
                }
                // Slow toward each turn, and to a stop at the last.
                let speed = (2.0 * 4.0 * d).sqrt().min(if n - 2 >= 2 { 6.0 } else { 14.0 });
                steer(&mut app, e, here, target, if last && d < 1.0 { 0.0 } else { speed }, false);
            }
        }
        app.update();
        let _ = start;
    }
    None
}

/// Alone, the lifeboat (four aboard: enough to win) comes home well inside
/// its air: out to where it drifts, a rope on it, and towed round our ship
/// into the hold's mouth.
#[test]
fn alone_the_lifeboat_comes_home_in_time() {
    let took = fetch(PodKind::Lifeboat).expect("the lifeboat never came home");
    let air = 300.0;
    eprintln!("the lifeboat home in {took:.0} s (its air: {air:.0} s)");
    assert!(took < air * 0.75, "the lifeboat took {took:.0} s of its {air:.0} s of air");
    let _ = TICK_HZ;
}
