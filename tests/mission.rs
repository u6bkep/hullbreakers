//! Scenarios: the mission wall, the boarding action, and how it ends.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::combat::{Beams, Combatant};
use hullbreakers::sim::events::{Detach, PlayerEvent};
use hullbreakers::sim::flight::Flight;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::mission::{Debris, Mission, Phase, ScenarioKind};
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::ship::{HomeShip, HullKind, ShipHull};
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
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / 60.0,
    )));
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

fn player(app: &mut App) -> Player {
    let world = app.world_mut();
    world.query::<&Player>().single(world).unwrap().clone()
}

fn set_player(app: &mut App, f: impl FnOnce(&mut Player)) {
    let world = app.world_mut();
    f(&mut world.query::<&mut Player>().single_mut(world).unwrap());
}

fn interact(app: &mut App) {
    let world = app.world_mut();
    let mut input = world.query::<&mut PlayerInput>().single_mut(world).unwrap();
    input.presses.interact.press();
}

fn mission(app: &mut App) -> Mission {
    let world = app.world_mut();
    world.query::<&Mission>().single(world).unwrap().clone()
}

fn ship_of(app: &mut App, kind: HullKind) -> Option<Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &ShipHull)>()
        .iter(world)
        .find(|(_, h)| h.0 == kind)
        .map(|(e, _)| e)
}

fn home(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<HomeShip>>()
        .single(world)
        .unwrap()
}

/// Standing on `ship`'s deck at `feet`, looking along `look` (ship
/// coordinates, any length).
fn stand(app: &mut App, ship: Entity, feet: Vec3, look: Vec3) {
    let heading = Vec3::new(look.x, 0.0, look.z).normalize();
    let orientation = Transform::IDENTITY.looking_to(heading, Vec3::Y).rotation;
    let pitch = look.y.atan2(Vec3::new(look.x, 0.0, look.z).length());
    set_player(app, |p| {
        p.mode = Mode::Booted {
            body: ship,
            feet,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
            orientation,
            pitch,
        };
    });
}

fn eye_height() -> f32 {
    Tuning::default().eye_height
}

/// Presses the mission wall's "Boarding action" button.
fn start_boarding(app: &mut App) {
    let ship = home(app);
    // The button is on the port wall of the bridge.
    let feet = Vec3::new(-2.0, 0.01, -7.3);
    let button = Vec3::new(-3.5, 1.75, -7.3);
    stand(app, ship, feet, button - (feet + Vec3::Y * eye_height()));
    interact(app);
    ticks(app, 2);
}

#[test]
fn the_game_starts_in_the_field_with_the_crew_aboard() {
    let mut app = app();
    assert_eq!(mission(&mut app).kind, ScenarioKind::Field);
    let home = home(&mut app);
    assert_eq!(player(&mut app).mode.body(), Some(home));
}

#[test]
fn the_mission_wall_starts_a_boarding_action() {
    let mut app = app();
    let old_home = home(&mut app);
    start_boarding(&mut app);
    assert_eq!(mission(&mut app).kind, ScenarioKind::Boarding);
    let new_home = home(&mut app);
    assert_ne!(new_home, old_home, "the world wasn't rebuilt");
    assert!(ship_of(&mut app, HullKind::Raider).is_some());
    let p = player(&mut app);
    assert_eq!(p.mode.body(), Some(new_home), "not put back aboard");
    assert!(p.events.iter().any(|(_, e)| e == PlayerEvent::Respawned));
    // Both ships fly their duel, among rocks.
    let world = app.world_mut();
    assert_eq!(world.query::<&Flight>().iter(world).count(), 2);
    let rocks = world.query::<&hullbreakers::sim::asteroids::Asteroid>().iter(world).count();
    assert!(rocks > 250, "only {rocks} rocks");
}

#[test]
fn standing_on_a_manoeuvring_ship_holds() {
    let mut app = app();
    start_boarding(&mut app);
    let home = home(&mut app);
    // A minute of the duel: close passes, and hits.
    for tick in 0..60 * 60 {
        ticks(&mut app, 1);
        let p = player(&mut app);
        assert_eq!(p.mode.body(), Some(home), "thrown off at {tick}: {:?}", p.events);
    }
}

#[test]
fn the_raider_hits_us_slowly() {
    let mut app = app();
    start_boarding(&mut app);
    ticks(&mut app, 20 * 60);
    let home = home(&mut app);
    let ours = *app.world().get::<Combatant>(home).unwrap();
    let raider = ship_of(&mut app, HullKind::Raider).unwrap();
    let theirs = *app.world().get::<Combatant>(raider).unwrap();
    let survival = Tuning::default().home_survival_seconds;
    // About 20 s of a survival time's worth of fire.
    let expected = 1.0 - 20.0 / survival;
    assert!((ours.integrity - expected).abs() < 0.05, "integrity {}", ours.integrity);
    assert!(theirs.integrity > 0.95, "raider at {}", theirs.integrity);
}

#[test]
fn dropping_the_shield_wins_and_blows_the_crew_clear() {
    let mut app = app();
    start_boarding(&mut app);
    let raider = ship_of(&mut app, HullKind::Raider).unwrap();
    // On the raider's bridge, looking down at the shield console.
    let feet = Vec3::new(0.0, 0.01, -11.0);
    let button = Vec3::new(0.0, 1.1, -13.0);
    stand(&mut app, raider, feet, button - (feet + Vec3::Y * eye_height()));
    interact(&mut app);
    ticks(&mut app, 2);
    assert!(!app.world().get::<Combatant>(raider).unwrap().shield);
    assert_eq!(mission(&mut app).shield_breaker.as_deref(), Some("Player 1"));
    // Our beams cut through in seconds, with the boarder still aboard.
    let exposed = Tuning::default().raider_exposed_seconds;
    let mut waited = 0;
    while app.world().get_entity(raider).is_ok() {
        assert!(waited < ((exposed + 2.0) * 60.0) as usize, "raider survived");
        ticks(&mut app, 1);
        waited += 1;
    }
    assert!(waited as f32 > exposed * 0.5 * 60.0, "raider blew up after {waited} ticks");
    assert_eq!(mission(&mut app).phase, Phase::Won);
    let p = player(&mut app);
    assert!(matches!(p.mode, Mode::Eva { .. }));
    assert!(
        p.events
            .iter()
            .any(|(_, e)| e == PlayerEvent::Detached(Detach::Blasted))
    );
    let world = app.world_mut();
    assert!(world.query::<&Debris>().iter(world).count() > 10);
}

#[test]
fn losing_the_ship_returns_to_the_field() {
    let mut app = app();
    start_boarding(&mut app);
    let home = home(&mut app);
    app.world_mut().get_mut::<Combatant>(home).unwrap().integrity = 0.001;
    ticks(&mut app, 60);
    assert_eq!(mission(&mut app).phase, Phase::Lost);
    assert!(matches!(player(&mut app).mode, Mode::Eva { .. }));
    let wait = Tuning::default().wreck_return_seconds;
    ticks(&mut app, (wait * 60.0) as usize + 10);
    let m = mission(&mut app);
    assert_eq!(m.kind, ScenarioKind::Field);
    let debrief = m.debrief.expect("no debrief");
    assert_eq!(debrief.phase, Phase::Lost);
    assert_eq!(debrief.adrift, vec!["Player 1".to_string()]);
    // Back aboard, and no wreckage left over.
    let home = self::home(&mut app);
    assert_eq!(player(&mut app).mode.body(), Some(home));
    let world = app.world_mut();
    assert_eq!(world.query::<&Debris>().iter(world).count(), 0);
}

#[test]
fn a_beam_shoves_a_player_out_of_its_way() {
    let mut app = app();
    start_boarding(&mut app);
    // Wait for a beam, then float in the middle of it.
    let beam = loop {
        ticks(&mut app, 1);
        if let Some(b) = app.world().resource::<Beams>().0.first() {
            break *b;
        }
    };
    let middle = (beam.from + beam.to) / 2.0;
    set_player(&mut app, |p| {
        p.flight_assist = false;
        p.mode = Mode::Eva {
            position: middle,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
    });
    ticks(&mut app, 30);
    let p = player(&mut app);
    let Mode::Eva { velocity, .. } = p.mode else {
        panic!("expected EVA");
    };
    assert!(velocity.length() > 3.0, "only pushed to {velocity}");
    let hits = p
        .events
        .iter()
        .filter(|(_, e)| *e == PlayerEvent::BeamHit)
        .count();
    assert_eq!(hits, 1, "one crossing, one hit");
}

fn vitals(app: &mut App) -> Vitals {
    let world = app.world_mut();
    *world.query::<&Vitals>().single(world).unwrap()
}

#[test]
fn beams_kill_and_the_dead_come_back_aboard() {
    let mut app = app();
    start_boarding(&mut app);
    let home = home(&mut app);
    // Keep putting the player back in the middle of whatever beam is firing.
    let mut died = false;
    for _ in 0..60 * 60 {
        let beam = app.world().resource::<Beams>().0.first().copied();
        if let Some(beam) = beam {
            set_player(&mut app, |p| {
                p.mode = Mode::Eva {
                    position: (beam.from + beam.to) / 2.0,
                    velocity: Vec3::ZERO,
                    orientation: Quat::IDENTITY,
                    pitch: 0.0,
                };
            });
        }
        ticks(&mut app, 1);
        if player(&mut app).events.iter().any(|(_, e)| e == PlayerEvent::Died) {
            died = true;
            break;
        }
    }
    assert!(died, "never died; health {}", vitals(&mut app).health);
    assert!(matches!(player(&mut app).mode, Mode::Booted { body, .. } if body == home));
    assert_eq!(vitals(&mut app).health, Tuning::default().player_health);
}

#[test]
fn health_comes_back_after_a_while_unhurt() {
    let mut app = app();
    let full = Tuning::default().player_health;
    {
        let world = app.world_mut();
        let mut v = world.query::<&mut Vitals>().single_mut(world).unwrap();
        v.health = full / 2.0;
        v.since_hurt = 0.0;
    }
    ticks(&mut app, (Tuning::default().health_regen_delay * 60.0) as usize - 10);
    assert_eq!(vitals(&mut app).health, full / 2.0, "healed too soon");
    ticks(&mut app, 60 * 10);
    assert_eq!(vitals(&mut app).health, full);
}

fn frame(app: &mut App, ship: Entity) -> hullbreakers::sim::frame::BodyFrame {
    let world = app.world();
    let (p, r, v, w) = (
        world.get::<Position>(ship).unwrap().0,
        world.get::<Rotation>(ship).unwrap().0,
        world.get::<LinearVelocity>(ship).unwrap().0,
        world.get::<AngularVelocity>(ship).unwrap().0,
    );
    hullbreakers::sim::frame::BodyFrame {
        position: p,
        rotation: r,
        center_of_mass: p,
        linear_velocity: v,
        angular_velocity: w,
    }
}

/// The crossing, as a player would make it: out of the stern on a close
/// pass, grapple the raider, and reel in to its hull.
#[test]
fn a_player_can_cross_to_the_raider() {
    let mut app = app();
    start_boarding(&mut app);
    // Coming up on the first close pass (half a pass in).
    let pass = Tuning::default().duel_pass_seconds;
    ticks(&mut app, (pass * 0.45 * 60.0) as usize);
    let raider = ship_of(&mut app, HullKind::Raider).unwrap();
    let home = home(&mut app);
    let ours = frame(&mut app, home);
    let theirs = frame(&mut app, raider);
    // Just off our stern, moving with the ship, looking at the raider.
    let start = ours.point_to_world(Vec3::new(0.0, 1.5, 18.0));
    let target = theirs.point_to_world(Vec3::new(0.0, 1.5, 0.0));
    let eye = Tuning::default().eye_height - Tuning::default().capsule_height / 2.0;
    let orientation = Transform::IDENTITY
        .looking_to(target - (start + Vec3::Y * eye), Vec3::Y)
        .rotation;
    set_player(&mut app, |p| {
        p.mode = Mode::Eva {
            position: start,
            velocity: ours.point_velocity(start),
            orientation,
            pitch: 0.0,
        };
        p.reference_body = Some(home);
    });
    {
        let world = app.world_mut();
        let mut input = world.query::<&mut PlayerInput>().single_mut(world).unwrap();
        input.presses.grapple.press();
    }
    ticks(&mut app, 1);
    let grapple = player(&mut app).grapple.expect("missed the raider");
    assert_eq!(grapple.target, raider);
    {
        let world = app.world_mut();
        let mut input = world.query::<&mut PlayerInput>().single_mut(world).unwrap();
        input.reel_in = true;
    }
    // Reeled in to hang at the hull, nearly at rest relative to it (from
    // there a player turns feet-first and touches down).
    for second in 0..10 {
        ticks(&mut app, 60);
        let p = player(&mut app);
        let theirs = frame(&mut app, raider);
        let Mode::Eva { position, velocity, .. } = p.mode else {
            panic!("expected EVA: {:?}", p.mode);
        };
        let local = theirs.point_to_local(position);
        let speed = theirs.velocity_to_local(position, velocity).length();
        // The raider's port or starboard wall is at |x| = 4.
        if local.x.abs() < 5.5 && local.y.abs() < 4.0 && local.z.abs() < 16.0 && speed < 1.0 {
            eprintln!("at the raider's hull after {second} s of reeling, {speed:.1} m/s");
            return;
        }
    }
    panic!("never reached the raider: {:?}", player(&mut app).mode);
}
