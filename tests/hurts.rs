//! A player's HurtLog: where each hurt came from (a shot's muzzle, a beam's
//! turret, a blast's centre), the killing blow included. The HUD's damage
//! arcs read it.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::bots::{Affixes, Detonation, Robot, new_robot};
use hullbreakers::sim::combat::{Beam, Beams};
use hullbreakers::sim::events::{EventLog, PlayerEvent};
use hullbreakers::sim::input::{InputCursor, PlayerInput};
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::ship::HomeShip;
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::vitals::{Damage, HurtLog, Vitals, beam_burns};
use hullbreakers::sim::weapons::Loadout;
use hullbreakers::sim::{SimPlugin, SimTick};

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

fn ship(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap()
}

fn crew(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, (With<PlayerInput>, Without<Robot>)>()
        .single(world)
        .unwrap()
}

fn hurts(app: &App, e: Entity) -> HurtLog {
    app.world().get::<HurtLog>(e).expect("players keep a hurt log").clone()
}

/// A point on the ship's deck (ship coordinates) in the world.
fn on_ship(app: &mut App, local: Vec3) -> Vec3 {
    let ship = ship(app);
    app.world().get::<Transform>(ship).unwrap().transform_point(local)
}

#[test]
fn a_shot_is_put_down_to_where_it_was_fired_from() {
    let mut app = app();
    // A Gunner in the aft hold with the crew member (who stands at z = 5).
    let ship = ship(&mut app);
    let feet = Vec3::new(0.0, 0.01, 12.0);
    let mode = Mode::Booted {
        body: ship,
        feet,
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    app.world_mut().spawn(new_robot(Robot::Gunner, Affixes::NONE, mode, ship, feet, &Tuning::default(), 0));
    let me = crew(&mut app);
    for _ in 0..60 * 6 {
        ticks(&mut app, 1);
        if hurts(&app, me).count > 0 {
            break;
        }
    }
    let log = hurts(&app, me);
    let hurt = log.recent.back().expect("the Gunner hit");
    let gunner = on_ship(&mut app, feet + Vec3::Y * 1.0);
    let from = hurt.from.expect("a shot says where it came from");
    assert!(from.distance(gunner) < 2.5, "from {from}, the Gunner at {gunner}");
    assert!(hurt.amount > 0.0 && !hurt.kill);
}

#[test]
fn a_blast_is_put_down_to_its_centre() {
    let mut app = app();
    let me = crew(&mut app);
    let at = on_ship(&mut app, Vec3::new(1.5, 1.0, 5.0));
    app.world_mut().write_message(Detonation {
        at,
        radius: 6.0,
        damage: 40.0,
        impulse: 0.0,
        robot: Entity::PLACEHOLDER,
        credit: None,
    });
    ticks(&mut app, 3);
    let log = hurts(&app, me);
    let hurt = log.recent.back().expect("the blast hurt");
    assert_eq!(hurt.from, Some(at));
}

#[test]
fn the_killing_blow_is_recorded_and_outlives_the_respawn() {
    let mut app = app();
    let me = crew(&mut app);
    let from = Vec3::new(10.0, 0.0, 0.0);
    app.world_mut().write_message(Damage {
        target: me,
        amount: 10_000.0,
        source: None,
        weak: false,
        from: Some(from),
    });
    ticks(&mut app, 2);
    let log = hurts(&app, me);
    let hurt = log.recent.back().expect("recorded");
    assert!(hurt.kill);
    assert_eq!(hurt.from, Some(from));
    // Respawned, at full health, and the log still says what killed them.
    let vitals = app.world().get::<Vitals>(me).unwrap();
    assert_eq!(vitals.health, Tuning::default().player_health);
}

#[test]
fn a_beam_burn_is_put_down_to_its_turret() {
    // beam_burns alone, on a world with one beam through a player in EVA
    // who was caught in it this tick.
    let mut world = World::new();
    world.init_resource::<Messages<Damage>>();
    world.insert_resource(Tuning::default());
    world.insert_resource(SimTick(100));
    let turret = Vec3::new(0.0, 0.0, -300.0);
    world.insert_resource(Beams(vec![Beam {
        shooter: Entity::PLACEHOLDER,
        target: Entity::PLACEHOLDER,
        from: turret,
        to: Vec3::new(0.0, 0.0, 300.0),
        shielded: false,
    }]));
    let mut events = EventLog::default();
    events.begin_tick(100);
    events.push(PlayerEvent::BeamHit);
    let me = world
        .spawn(Player {
            mode: Mode::Eva {
                position: Vec3::new(0.5, 0.0, 0.0),
                velocity: Vec3::ZERO,
                orientation: Quat::IDENTITY,
                pitch: 0.0,
            },
            boots_on: false,
            flight_assist: true,
            grapple: None,
            reference_body: None,
            detach_timer: 0.0,
            loadout: Loadout::base(),
            input: InputCursor::default(),
            events,
        })
        .id();
    world.run_system_once(beam_burns).unwrap();
    let messages = world.resource::<Messages<Damage>>();
    let mut cursor = messages.get_cursor();
    let burns: Vec<Damage> = cursor.read(messages).copied().collect();
    assert_eq!(burns.len(), 1);
    assert_eq!(burns[0].target, me);
    assert_eq!(burns[0].from, Some(turret));
}
