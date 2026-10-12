//! Wave 4's guns, on the authority: what a shotgun's pellets, a plasma
//! bolt's burst, an arc's jumps, a laser's beam and a sniper's round do to
//! what they meet (see `docs/plan-weapons.md`).

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::items::{Family, Weapon};
use hullbreakers::sim::player::{Mode, Player, new_player};
use hullbreakers::sim::ship::HomeShip;
use hullbreakers::sim::shots::Team;
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::vitals::Vitals;
use hullbreakers::sim::weapons::{GunKind, Slot};

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

fn eva_at(position: Vec3) -> Mode {
    Mode::Eva {
        position,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    }
}

/// The height of the line of fire over a floating player's position.
fn eye() -> f32 {
    Tuning::default().eye_height - Tuning::default().capsule_height / 2.0
}

/// The local player floating far from the ship facing −Z, `gun` in the
/// main hand, the off hand free (braced), every pool full.
fn armed_with(app: &mut App, gun: Weapon) -> Entity {
    let me = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<PlayerInput>>().single(world).unwrap()
    };
    let mut player = app.world_mut().get_mut::<Player>(me).unwrap();
    player.mode = eva_at(Vec3::new(0.0, 0.0, 200.0));
    player.flight_assist = false;
    player.loadout.restock();
    player.loadout.inventory.weapons[0] = Some(gun);
    player.loadout.stances[0].main = Slot::Gun(0);
    player.loadout.stances[0].off = Slot::Empty;
    player.loadout.current = 0;
    player.loadout.switching = 0.0;
    me
}

/// Someone of the robots' side floating still at `at`, flight assist off.
fn target(app: &mut App, at: Vec3) -> Entity {
    let ship = {
        let world = app.world_mut();
        world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap()
    };
    let tuning = Tuning::default();
    let target = app.world_mut().spawn((new_player(ship, 1), Vitals::full(&tuning), Team::Robots)).id();
    let mut player = app.world_mut().get_mut::<Player>(target).unwrap();
    player.mode = eva_at(at);
    // Drifting as it's pushed (assist would hold it still).
    player.flight_assist = false;
    ticks(app, 2);
    target
}

/// A slab of hull: a static box, `size` across, centred at `at`.
fn wall(app: &mut App, at: Vec3, size: Vec3) {
    app.world_mut().spawn((RigidBody::Static, Collider::cuboid(size.x, size.y, size.z), Transform::from_translation(at)));
    ticks(app, 2);
}

/// Holds the trigger for `held` ticks, then lets go for `after`.
fn fire(app: &mut App, me: Entity, held: usize, after: usize) {
    app.world_mut().get_mut::<PlayerInput>(me).unwrap().main.held = true;
    ticks(app, held);
    app.world_mut().get_mut::<PlayerInput>(me).unwrap().main.held = false;
    ticks(app, after);
}

fn health(app: &App, e: Entity) -> f32 {
    app.world().get::<Vitals>(e).unwrap().health
}

fn velocity(app: &App, e: Entity) -> Vec3 {
    match app.world().get::<Player>(e).unwrap().mode {
        Mode::Eva { velocity, .. } => velocity,
        _ => panic!("not floating"),
    }
}

const FULL: f32 = 100.0;

#[test]
fn a_shotgun_hurts_and_shoves_hard_where_a_rifle_barely_nudges() {
    let shove = |gun: GunKind| {
        let mut app = app();
        let me = armed_with(&mut app, Weapon::issue(gun));
        let them = target(&mut app, Vec3::new(0.0, eye(), 194.0));
        fire(&mut app, me, 2, 20);
        assert!(health(&app, them) < FULL, "{gun:?} missed");
        -velocity(&app, them).z
    };
    let (shotgun, rifle) = (shove(GunKind::Shotgun), shove(GunKind::Rifle));
    assert!(shotgun > 0.4, "shotgun shove {shotgun} m/s");
    assert!(shotgun > 5.0 * rifle, "shotgun {shotgun}, rifle {rifle}");
}

/// A plasma bolt into a wall at z = 180 with one target on our side of it
/// and one behind it, both in reach of the burst; their health after.
#[test]
fn a_plasma_burst_reaches_round_corners_but_not_through_a_hull() {
    let mut app = app();
    let me = armed_with(&mut app, Weapon::issue(GunKind::Rifle).with_family(Family::Plasma));
    wall(&mut app, Vec3::new(0.0, eye(), 180.0), Vec3::new(8.0, 8.0, 0.2));
    let near = target(&mut app, Vec3::new(1.3, eye(), 181.5));
    let behind = target(&mut app, Vec3::new(1.3, eye(), 178.5));
    // A loose crate on our side of the wall, in reach of the burst.
    let crate_ = app
        .world_mut()
        .spawn((
            RigidBody::Dynamic,
            Collider::cuboid(0.5, 0.5, 0.5),
            hullbreakers::sim::bodies::dynamic_mass(&Collider::cuboid(0.5, 0.5, 0.5), 200.0),
            SleepingDisabled,
            hullbreakers::sim::bodies::PreviousVelocity::default(),
            Position(Vec3::new(-1.4, eye(), 181.2)),
            Rotation::default(),
        ))
        .id();
    ticks(&mut app, 2);
    // One bolt; it's slow (about 100 m/s): time to land.
    fire(&mut app, me, 1, 40);
    assert!(health(&app, near) < FULL, "the burst missed what it could see");
    assert!(velocity(&app, near).length() > 0.05, "the burst didn't shove");
    assert_eq!(health(&app, behind), FULL, "the burst went through the hull");
    let pushed = app.world().get::<LinearVelocity>(crate_).unwrap().0;
    assert!(pushed.z > 0.01 && pushed.x < 0.0, "the crate wasn't pushed off the burst: {pushed}");
}

/// An arc into one target, with another beside it and a third on its other
/// side, behind a wall or not.
fn arc(walled: bool) -> (f32, f32, f32) {
    let mut app = app();
    let me = armed_with(&mut app, Weapon::issue(GunKind::Rifle).with_family(Family::Arc));
    let first = target(&mut app, Vec3::new(0.0, eye(), 190.0));
    let beside = target(&mut app, Vec3::new(3.0, eye(), 190.0));
    let walled_off = target(&mut app, Vec3::new(-3.5, eye(), 190.0));
    if walled {
        wall(&mut app, Vec3::new(-1.8, eye(), 190.0), Vec3::new(0.2, 8.0, 8.0));
    }
    fire(&mut app, me, 1, 5);
    // It pushes nothing.
    assert_eq!(velocity(&app, first), Vec3::ZERO);
    (FULL - health(&app, first), FULL - health(&app, beside), FULL - health(&app, walled_off))
}

#[test]
fn an_arc_jumps_to_targets_in_sight_and_stops_at_hulls() {
    let (first, beside, other) = arc(false);
    assert!(first > 0.0 && beside > 0.0 && other > 0.0, "{first} {beside} {other}");
    // Less with each jump.
    assert!(beside < first && other < beside, "{first} {beside} {other}");
    let (first, beside, other) = arc(true);
    assert!(first > 0.0 && beside > 0.0, "{first} {beside}");
    assert_eq!(other, 0.0, "jumped through a wall");
}

#[test]
fn a_laser_burns_while_held_heats_and_pushes_nothing() {
    let mut app = app();
    let me = armed_with(&mut app, Weapon::issue(GunKind::Smg).with_family(Family::Laser));
    let them = target(&mut app, Vec3::new(0.0, eye(), 180.0));
    fire(&mut app, me, 30, 1);
    let hurt = FULL - health(&app, them);
    // Half a second of beam: a tick's damage every tick.
    let per_tick = Weapon::issue(GunKind::Smg).with_family(Family::Laser).stats().damage;
    assert!(hurt > per_tick * 25.0, "only {hurt} from 30 ticks of {per_tick}");
    assert_eq!(velocity(&app, them), Vec3::ZERO);
    let hand = app.world().get::<Player>(me).unwrap().loadout.hands[0];
    assert!(hand.heat > 0.05 && !hand.overheated, "{hand:?}");
    // Five more seconds held: it overheats and stops a while to cool.
    let before = app.world().get::<Player>(me).unwrap().loadout.hands[0].fired;
    fire(&mut app, me, 300, 0);
    let fired = app.world().get::<Player>(me).unwrap().loadout.hands[0].fired - before;
    assert!((200..260).contains(&fired), "{fired} ticks of beam in 300");
}

#[test]
fn a_sniper_round_goes_through_one_body_into_the_next() {
    let mut app = app();
    let me = armed_with(&mut app, Weapon::issue(GunKind::Sniper));
    let targets = [190.0, 186.0, 182.0].map(|z| target(&mut app, Vec3::new(0.0, eye(), z)));
    fire(&mut app, me, 1, 10);
    let hurt = targets.map(|t| health(&app, t) < FULL);
    assert_eq!(hurt, [true, true, false]);
}
