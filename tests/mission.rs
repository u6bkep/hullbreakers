//! Scenarios: the mission wall, the boarding action, and how it ends.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::bots::Robot;
use hullbreakers::sim::combat::{Beams, Combatant};
use hullbreakers::sim::events::{Detach, PlayerEvent};
use hullbreakers::sim::flight::Flight;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::mission::{Debris, Director, Mission, Phase, ScenarioKind};
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::design::design;
use hullbreakers::sim::devices::Devices;
use hullbreakers::sim::ship::{ControlAction, HOME, HomeShip, ShipHull};
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
    // Robots are tested on their own (tests/robots.rs).
    app.world_mut().resource_mut::<Director>().robots = false;
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
    world.query_filtered::<&Player, Without<Robot>>().single(world).unwrap().clone()
}

fn set_player(app: &mut App, f: impl FnOnce(&mut Player)) {
    let world = app.world_mut();
    f(&mut world.query_filtered::<&mut Player, Without<Robot>>().single_mut(world).unwrap());
}

fn interact(app: &mut App) {
    let world = app.world_mut();
    let mut input = world.query_filtered::<&mut PlayerInput, Without<Robot>>().single_mut(world).unwrap();
    input.presses.interact.press();
}

fn mission(app: &mut App) -> Mission {
    let world = app.world_mut();
    world.query::<&Mission>().single(world).unwrap().clone()
}

fn ship_of(app: &mut App, kind: &str) -> Option<Entity> {
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

/// Presses the mission wall's button for `kind`.
fn start(app: &mut App, kind: ScenarioKind) {
    let ship = home(app);
    // The buttons are on the port wall of the bridge.
    let (_, button, _) = *design(HOME)
        .controls
        .iter()
        .find(|(a, ..)| *a == ControlAction::Scenario(kind))
        .expect("no button");
    let feet = Vec3::new(-2.0, 0.01, button.z);
    stand(app, ship, feet, button - (feet + Vec3::Y * eye_height()));
    interact(app);
    ticks(app, 2);
    assert_eq!(mission(app).kind, kind, "the button didn't start it");
}

fn start_boarding(app: &mut App) {
    start(app, ScenarioKind::Boarding);
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
    assert!(ship_of(&mut app, "carrier").is_some());
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
fn the_carrier_hits_us_slowly() {
    let mut app = app();
    start_boarding(&mut app);
    ticks(&mut app, 20 * 60);
    let home = home(&mut app);
    let ours = *app.world().get::<Combatant>(home).unwrap();
    let carrier = ship_of(&mut app, "carrier").unwrap();
    let theirs = *app.world().get::<Combatant>(carrier).unwrap();
    let survival = Tuning::default().home_survival_seconds;
    // About 20 s of a survival time's worth of fire.
    let expected = 1.0 - 20.0 / survival;
    assert!((ours.integrity - expected).abs() < 0.05, "integrity {}", ours.integrity);
    assert!(theirs.integrity > 0.95, "carrier at {}", theirs.integrity);
}

#[test]
fn dropping_the_shield_wins_and_blows_the_crew_clear() {
    let mut app = app();
    start_boarding(&mut app);
    let carrier = ship_of(&mut app, "carrier").unwrap();
    // On the carrier's bridge, looking down at the shield console.
    let console = hullbreakers::sim::design::design("carrier").marker("objective");
    let feet = Vec3::new(0.0, console.y - 0.5 + 0.01, console.z + 2.0);
    let button = console + Vec3::Y * 0.6;
    stand(&mut app, carrier, feet, button - (feet + Vec3::Y * eye_height()));
    interact(&mut app);
    ticks(&mut app, 2);
    assert!(!app.world().get::<Combatant>(carrier).unwrap().shield);
    let report = mission(&mut app).report;
    assert!(report.iter().any(|l| l.starts_with("Shield dropped by Player 1")), "{report:?}");
    // Our beams cut through in seconds, with the boarder still aboard.
    let exposed = Tuning::default().enemy_exposed_seconds;
    let mut waited = 0;
    while app.world().get_entity(carrier).is_ok() {
        assert!(waited < ((exposed + 2.0) * 60.0) as usize, "carrier survived");
        ticks(&mut app, 1);
        waited += 1;
    }
    assert!(waited as f32 > exposed * 0.5 * 60.0, "carrier blew up after {waited} ticks");
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
    assert_eq!(debrief.outcome, "SHIP LOST");
    assert!(!debrief.crew[0].aboard);
    assert!(debrief.lines.iter().any(|l| l.starts_with("Left drifting: Player 1")), "{:?}", debrief.lines);
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
    *world.query_filtered::<&Vitals, Without<Robot>>().single(world).unwrap()
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
        let mut v = world.query_filtered::<&mut Vitals, Without<Robot>>().single_mut(world).unwrap();
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
/// pass, grapple the carrier, and reel in to its hull.
#[test]
fn a_player_can_cross_to_the_carrier() {
    let mut app = app();
    start_boarding(&mut app);
    // Coming up on the first close pass (half a pass in).
    let pass = Tuning::default().carrier_pass_seconds;
    ticks(&mut app, (pass * 0.45 * 60.0) as usize);
    let carrier = ship_of(&mut app, "carrier").unwrap();
    let home = home(&mut app);
    let ours = frame(&mut app, home);
    let theirs = frame(&mut app, carrier);
    // Just off our stern, moving with the ship, looking at the carrier.
    let start = ours.point_to_world(Vec3::new(0.0, 1.5, 18.0));
    let target = theirs.point_to_world(Vec3::new(0.0, 3.0, 0.0));
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
        let mut input = world.query_filtered::<&mut PlayerInput, Without<Robot>>().single_mut(world).unwrap();
        input.presses.main.press();
    }
    ticks(&mut app, 1);
    let grapple = player(&mut app).grapple.expect("missed the carrier");
    assert_eq!(grapple.target, carrier);
    {
        let world = app.world_mut();
        let mut input = world.query_filtered::<&mut PlayerInput, Without<Robot>>().single_mut(world).unwrap();
        input.reel_in = true;
    }
    // Reeled in to hang at the hull, nearly at rest relative to it (from
    // there a player turns feet-first and touches down).
    for second in 0..10 {
        ticks(&mut app, 60);
        let p = player(&mut app);
        let theirs = frame(&mut app, carrier);
        let Mode::Eva { position, velocity, .. } = p.mode else {
            panic!("expected EVA: {:?}", p.mode);
        };
        let local = theirs.point_to_local(position);
        let speed = theirs.velocity_to_local(position, velocity).length();
        // The carrier's sides are at |x| = 6.3, its roof at y = 6.6.
        if local.x.abs() < 8.0 && local.y > -2.5 && local.y < 9.0 && local.z.abs() < 32.0 && speed < 1.0 {
            eprintln!("at the carrier's hull after {second} s of reeling, {speed:.1} m/s");
            return;
        }
    }
    panic!("never reached the carrier: {:?}", player(&mut app).mode);
}

/// From the carrier's hangar floor, up the ramp along its port wall,
/// through the door at its top, into the upper deck's port room.
#[test]
fn the_carriers_ramp_leads_to_the_upper_deck() {
    let mut app = app();
    start_boarding(&mut app);
    let carrier = ship_of(&mut app, "carrier").unwrap();
    stand(&mut app, carrier, Vec3::new(-4.8, 0.01, 28.5), Vec3::NEG_Z);
    {
        let world = app.world_mut();
        let mut input = world.query_filtered::<&mut PlayerInput, Without<Robot>>().single_mut(world).unwrap();
        input.movement = Vec3::Z;
    }
    let upper = hullbreakers::sim::design::design("carrier").marker("upper_deck").y;
    for second in 0..12 {
        ticks(&mut app, 60);
        let Mode::Booted { body, feet, up, .. } = player(&mut app).mode else {
            panic!("fell off at {second} s: {:?}", player(&mut app).mode);
        };
        assert_eq!(body, carrier);
        if feet.z < 12.0 {
            assert!((feet.y - upper).abs() < 0.1 && up.y > 0.99, "in the room but at {feet}, up {up}");
            return;
        }
    }
    panic!("never reached the upper deck: {:?}", player(&mut app).mode);
}

/// The carrier's robots stand at their posts on its decks.
#[test]
fn the_carriers_gunners_stand_at_their_posts() {
    let mut app = app();
    app.world_mut().resource_mut::<Director>().robots = true;
    start_boarding(&mut app);
    ticks(&mut app, 120);
    let world = app.world_mut();
    let gunners: Vec<Mode> = world
        .query::<(&Robot, &Player, &hullbreakers::sim::bots::Brain)>()
        .iter(world)
        .filter(|(r, ..)| r.gait() == hullbreakers::sim::bots::Gait::Walker)
        .map(|(_, p, b)| {
            if !matches!(p.mode, Mode::Booted { .. }) {
                eprintln!("off its feet: {:?} post {}", p.events.iter().collect::<Vec<_>>(), b.post);
            }
            p.mode
        })
        .collect();
    let crew: Vec<_> = world.query_filtered::<&Player, Without<Robot>>().single(world).unwrap().events.iter().collect();
    eprintln!("crew events: {crew:?}");
    assert_eq!(gunners.len(), 7);
    for mode in gunners {
        assert!(matches!(mode, Mode::Booted { up, .. } if up.y > 0.99), "{mode:?}");
    }
}

#[test]
fn the_weapons_locker_arms_you() {
    let mut app = app();
    let ship = home(&mut app);
    assert!(player(&mut app).loadout.inventory.weapons.iter().all(Option::is_none), "started armed");
    // In the aft hold, facing the locker on the starboard wall.
    let feet = Vec3::new(2.4, 0.01, 9.0);
    let locker = Vec3::new(3.54, 1.5, 9.0);
    stand(&mut app, ship, feet, locker - (feet + Vec3::Y * eye_height()));
    interact(&mut app);
    ticks(&mut app, 2);
    assert_eq!(player(&mut app).loadout.inventory.weapons.iter().flatten().count(), 2);
}

#[test]
fn the_dead_leave_their_guns_where_they_fell() {
    use hullbreakers::sim::pickups::Pickup;
    let mut app = app();
    let ship = home(&mut app);
    // Armed, on the deck aft.
    set_player(&mut app, |p| p.loadout.restock());
    let feet = Vec3::new(0.0, 0.01, 10.0);
    stand(&mut app, ship, feet, Vec3::NEG_Z);
    {
        let world = app.world_mut();
        let mut v = world.query_filtered::<&mut Vitals, Without<Robot>>().single_mut(world).unwrap();
        *v = Vitals { health: 0.0, since_hurt: 0.0 };
    }
    ticks(&mut app, 2);
    assert!(player(&mut app).loadout.inventory.weapons.iter().all(Option::is_none), "respawned armed");
    let pickups = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Pickup>().iter(world).cloned().collect::<Vec<_>>()
    };
    let dropped = pickups(&mut app);
    assert_eq!(dropped.len(), 1);
    let guns = dropped[0].items.iter().filter(|i| matches!(i, hullbreakers::sim::items::Item::Weapon(_)));
    assert_eq!(guns.count(), 2);
    assert_eq!(dropped[0].body, Some(ship));
    // Back to it, looking at it: take them.
    let stand_at = Vec3::new(0.0, 0.01, 8.5);
    stand(&mut app, ship, stand_at, dropped[0].at - (stand_at + Vec3::Y * eye_height()));
    interact(&mut app);
    ticks(&mut app, 2);
    assert_eq!(player(&mut app).loadout.inventory.weapons.iter().flatten().count(), 2, "didn't pick them up");
    assert!(pickups(&mut app).is_empty(), "the case is still there");
}

// ---------------------------------------------------------------------------
// Devices, and the scenarios built from them

fn live(app: &mut App, ship: Entity, device: &str) -> bool {
    let hull = app.world().get::<ShipHull>(ship).unwrap().design();
    app.world().get::<Devices>(ship).unwrap().live(hull, device)
}

/// Presses `device` on `ship`, standing a metre out from its face, looking
/// at it, through the usual interact path.
fn press_device(app: &mut App, ship: Entity, device: &str) {
    let hull = app.world().get::<ShipHull>(ship).unwrap().design();
    let d = &hull.devices[hull.device(device)];
    let out = Vec3::new(d.face.x, 0.0, d.face.z).normalize();
    let feet = Vec3::new(d.at.x, 0.01, d.at.z) + out;
    stand(app, ship, feet, d.at - (feet + Vec3::Y * eye_height()));
    interact(app);
    ticks(app, 2);
}

/// Floating at `position`, still.
fn float(app: &mut App, position: Vec3) {
    set_player(app, |p| {
        p.mode = Mode::Eva {
            position,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
    });
}

fn walk_forward(app: &mut App, on: bool) {
    let world = app.world_mut();
    let mut input = world.query_filtered::<&mut PlayerInput, Without<Robot>>().single_mut(world).unwrap();
    input.movement = if on { Vec3::Z } else { Vec3::ZERO };
}

#[test]
fn the_mission_wall_starts_the_grapple_run_and_the_dead_ship() {
    use hullbreakers::sim::scenarios::grapple_run::{COURSE, COURSE_AT, gates, rock_places};
    let mut app = app();
    start(&mut app, ScenarioKind::GrappleRun);
    let course = ship_of(&mut app, COURSE).expect("no course");
    assert_eq!(app.world().get::<Position>(course).unwrap().0, COURSE_AT);
    let world = app.world_mut();
    let rocks = world.query::<&hullbreakers::sim::asteroids::Asteroid>().iter(world).count();
    assert!(rocks > 200, "only {rocks} rocks");
    // Every gate is clear of rocks.
    let course_design = design(COURSE);
    for i in gates(course_design) {
        let gate = COURSE_AT + course_design.devices[i].at;
        for (at, r) in rock_places() {
            assert!(gate.distance(at) > r * 2.0 + 8.0, "a rock at {at} (r {r}) crowds gate {i} at {gate}");
        }
    }
    assert_eq!(mission(&mut app).markers.len(), 1, "a marker on the next gate");
    start(&mut app, ScenarioKind::DeadShip);
    assert!(ship_of(&mut app, COURSE).is_none(), "the course outlived its scenario");
    assert!(ship_of(&mut app, "derelict").is_some());
}

/// A door is a wall until it's opened: walking at the port door stops at
/// it, until junction A is turned to it.
#[test]
fn a_door_blocks_the_way_until_its_power_opens_it() {
    let mut app = app();
    start(&mut app, ScenarioKind::DeadShip);
    let derelict = ship_of(&mut app, "derelict").unwrap();
    let toward_door = |app: &mut App| {
        // In the mid hall, facing the port door (at x = -1, z = 0.3 to 1.7).
        stand(app, derelict, Vec3::new(1.5, 0.01, 1.0), Vec3::NEG_X);
        walk_forward(app, true);
        ticks(app, 120);
        walk_forward(app, false);
        let Mode::Booted { feet, .. } = player(app).mode else { panic!("off the deck") };
        feet
    };
    // (Walking into it climbs it, as with any wall: but never through.)
    let feet = toward_door(&mut app);
    assert!(feet.x > -0.9, "walked through a shut door to {feet}");
    press_device(&mut app, derelict, "reactor");
    press_device(&mut app, derelict, "j1");
    assert!(live(&mut app, derelict, "port_door"));
    let feet = toward_door(&mut app);
    assert!(feet.x < -2.0, "the open door still stopped us at {feet}");
}

/// The whole dead ship, solo: restart the reactor, route the power to the
/// vault, turn the keys, release the core, and get home.
#[test]
fn the_dead_ship_can_be_won() {
    use hullbreakers::sim::scenarios::BANNER_SECONDS;
    let mut app = app();
    start(&mut app, ScenarioKind::DeadShip);
    let derelict = ship_of(&mut app, "derelict").unwrap();
    assert!(mission(&mut app).objective.contains("restart its reactor"));
    press_device(&mut app, derelict, "reactor");
    assert!(live(&mut app, derelict, "reactor") && live(&mut app, derelict, "board"));
    assert!(mission(&mut app).objective.contains("Route power"));
    // Junction A to the port door (it latches open), then on to junction B.
    press_device(&mut app, derelict, "j1");
    press_device(&mut app, derelict, "j1");
    assert!(live(&mut app, derelict, "port_door") && live(&mut app, derelict, "j2"));
    // Junction B to the bulkhead, then on to junction C.
    press_device(&mut app, derelict, "j2");
    press_device(&mut app, derelict, "j2");
    assert!(live(&mut app, derelict, "bulkhead") && live(&mut app, derelict, "j3"));
    // Junction C to the vault bus.
    press_device(&mut app, derelict, "j3");
    assert!(live(&mut app, derelict, "vault_bus"));
    assert!(mission(&mut app).objective.contains("key B"), "{}", mission(&mut app).objective);
    // Alone: key B holds while we get to key A.
    press_device(&mut app, derelict, "key_b");
    ticks(&mut app, 120);
    assert!(!live(&mut app, derelict, "vault"));
    press_device(&mut app, derelict, "key_a");
    assert!(live(&mut app, derelict, "vault"), "the keys didn't open the vault");
    press_device(&mut app, derelict, "core_clamp");
    let m = mission(&mut app);
    assert!(m.objective.contains("take the data core"), "{}", m.objective);
    assert!(m.status[0].starts_with("Overload in"), "{:?}", m.status);
    // Home without the core isn't a win.
    let ship = home(&mut app);
    stand(&mut app, ship, Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z);
    ticks(&mut app, 2);
    assert_eq!(mission(&mut app).phase, Phase::Underway, "won without the core");
    // Take the core off its cradle, then home.
    let core = mission(&mut app).markers.iter().find(|m| m.label == "Data core").expect("no core marker").at;
    let feet = Vec3::new(core.x, 0.01, core.z + 1.4);
    stand(&mut app, derelict, feet, core - (feet + Vec3::Y * eye_height()));
    interact(&mut app);
    ticks(&mut app, 2);
    let core = hullbreakers::sim::items::Item::Cargo(hullbreakers::sim::items::CargoKind::DataCore);
    assert!(player(&mut app).loadout.inventory.pack.contains(&Some(core)), "didn't take the core");
    assert!(mission(&mut app).objective.contains("everyone back"), "{}", mission(&mut app).objective);
    stand(&mut app, ship, Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z);
    ticks(&mut app, 2);
    let m = mission(&mut app);
    assert_eq!((m.phase, m.outcome.as_str()), (Phase::Won, "SALVAGE COMPLETE"));
    assert!(ship_of(&mut app, "derelict").is_none(), "the derelict should go up as we leave");
    // Back to the field, with the score card.
    ticks(&mut app, (BANNER_SECONDS * 60.0) as usize + 5);
    let m = mission(&mut app);
    assert_eq!(m.kind, ScenarioKind::Field);
    let debrief = m.debrief.expect("no debrief");
    assert_eq!(debrief.scenario, ScenarioKind::DeadShip);
    let card = &debrief.crew[0];
    assert!(card.aboard);
    for credit in ["restarted the reactor", "turned key A", "turned key B", "released the data core", "carried the data core home", "turned junctions 5 times"] {
        assert!(card.credits.iter().any(|c| c == credit), "no {credit:?} in {:?}", card.credits);
    }
    assert!(card.presses >= 9, "{} presses", card.presses);
    assert!(debrief.lines.iter().any(|l| l.starts_with("Reactor restarted by Player 1")), "{:?}", debrief.lines);
}

/// A pull pulled before its power comes counts when the power does: the
/// core's clamp, pulled at a shut vault (by reaching through, here),
/// releases the core as the vault opens.
#[test]
fn the_core_released_early_counts_when_the_vault_opens() {
    let mut app = app();
    start(&mut app, ScenarioKind::DeadShip);
    let derelict = ship_of(&mut app, "derelict").unwrap();
    {
        let hull = design("derelict");
        let mut devices = app.world_mut().get_mut::<Devices>(derelict).unwrap();
        devices.0[hull.device("core_clamp")].set = 1;
    }
    for device in ["reactor", "j1", "j1", "j2", "j2", "j3", "key_b", "key_a"] {
        press_device(&mut app, derelict, device);
    }
    ticks(&mut app, 1);
    let m = mission(&mut app);
    assert!(m.report.iter().any(|l| l.starts_with("Data core released by ?")), "{:?}", m.report);
    assert!(m.status[0].starts_with("Overload in 1:1"), "{:?}", m.status);
}

#[test]
fn the_dead_ship_is_lost_if_the_crew_is_still_out_when_it_goes() {
    use hullbreakers::sim::scenarios::dead_ship::OVERLOAD_SECONDS;
    let mut app = app();
    start(&mut app, ScenarioKind::DeadShip);
    let derelict = ship_of(&mut app, "derelict").unwrap();
    for device in ["reactor", "j1", "j1", "j2", "j2", "j3", "key_b", "key_a", "core_clamp"] {
        press_device(&mut app, derelict, device);
    }
    assert!(live(&mut app, derelict, "alarm"), "the alarms should be going");
    assert!(!live(&mut app, derelict, "eng_lights"), "the lights should be out");
    // Standing in the vault as it goes.
    stand(&mut app, derelict, Vec3::new(-3.0, 0.01, -14.0), Vec3::NEG_Z);
    ticks(&mut app, (OVERLOAD_SECONDS * 60.0) as usize);
    let m = mission(&mut app);
    assert_eq!((m.phase, m.outcome.as_str()), (Phase::Lost, "DERELICT LOST"));
    assert!(ship_of(&mut app, "derelict").is_none());
    assert!(matches!(player(&mut app).mode, Mode::Eva { .. }), "not thrown off the wreck");
}

/// Through every gate in order, by hopping from just before each to just
/// after it.
fn fly_the_gates(app: &mut App, order: impl IntoIterator<Item = usize>) {
    use hullbreakers::sim::scenarios::grapple_run::{COURSE, COURSE_AT, gates};
    let course = design(COURSE);
    let gates = gates(course);
    for k in order {
        let gate = &course.devices[gates[k]];
        let at = COURSE_AT + gate.at;
        float(app, at - gate.face * 1.0);
        ticks(app, 1);
        float(app, at + gate.face * 1.0);
        ticks(app, 1);
    }
}

#[test]
fn the_grapple_run_is_won_through_every_gate_in_order() {
    use hullbreakers::sim::scenarios::BANNER_SECONDS;
    let mut app = app();
    start(&mut app, ScenarioKind::GrappleRun);
    ticks(&mut app, 60);
    // Out of order doesn't count: the next gate is still the first.
    fly_the_gates(&mut app, [1, 2]);
    let m = mission(&mut app);
    assert_eq!(m.markers[0].label, "Gate 1", "{:?}", m.markers);
    assert!(m.status[0].contains("to gate 1"), "{:?}", m.status);
    fly_the_gates(&mut app, 0..4);
    let m = mission(&mut app);
    assert_eq!(m.markers[0].label, "Gate 5");
    assert!(m.status[0].contains("gate 4/10"), "{:?}", m.status);
    fly_the_gates(&mut app, 4..10);
    let m = mission(&mut app);
    assert_eq!((m.phase, m.outcome.as_str()), (Phase::Won, "COURSE CLEAR"));
    assert!(m.report[0].starts_with("Player 1: 0:"), "{:?}", m.report);
    assert!(m.markers.is_empty());
    ticks(&mut app, (BANNER_SECONDS * 60.0) as usize + 5);
    let m = mission(&mut app);
    assert_eq!(m.kind, ScenarioKind::Field);
    let debrief = m.debrief.expect("no debrief");
    assert!(debrief.crew[0].vacuum_seconds > 0.0);
    assert!(debrief.crew[0].credits[0].starts_with("ran the course in"));
    // Finishing out past the bow isn't being left drifting.
    assert!(!debrief.lines.iter().any(|l| l.contains("drifting")), "{:?}", debrief.lines);
    // A second run: the board remembers the first.
    start(&mut app, ScenarioKind::GrappleRun);
    ticks(&mut app, 30);
    fly_the_gates(&mut app, 0..10);
    let report = mission(&mut app).report;
    assert!(report[0].contains("personal best"), "{report:?}");
}

#[test]
fn the_grapple_run_is_lost_when_the_clock_runs_out() {
    use hullbreakers::sim::scenarios::grapple_run::TIME_LIMIT;
    let mut app = app();
    start(&mut app, ScenarioKind::GrappleRun);
    fly_the_gates(&mut app, 0..3);
    {
        let world = app.world_mut();
        let mut m = world.query::<&mut Mission>().single_mut(world).unwrap();
        m.started = m.started.wrapping_sub((TIME_LIMIT * 60.0) as u32);
    }
    ticks(&mut app, 2);
    let m = mission(&mut app);
    assert_eq!((m.phase, m.outcome.as_str()), (Phase::Lost, "OUT OF TIME"));
    assert!(m.report.iter().any(|l| l.contains("out of time at gate 4 of 10")), "{:?}", m.report);
}
