//! Keeping weapons: what you carry stays yours through scenario changes,
//! what you leave where you die is yours alone to take back (and back in
//! your stash at the mission's end if you don't), and the armory moves
//! guns between your stash and your hands.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::bots::Robot;
use hullbreakers::sim::events::PlayerEvent;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::items::{Item, Place, Rarity, Weapon};
use hullbreakers::sim::mission::{Director, ScenarioKind};
use hullbreakers::sim::pickups::Pickup;
use hullbreakers::sim::player::{Mode, Player, new_player};
use hullbreakers::sim::progression::{Profile, ProfileOp, ProfileOrders, ProfileWire, RECOVER_UNCLAIMED};
use hullbreakers::sim::ship::HomeShip;
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::vitals::Vitals;
use hullbreakers::sim::weapons::GunKind;

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
    ticks(&mut app, 4);
    app
}

fn ticks(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn me(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, (With<PlayerInput>, With<Vitals>, Without<Robot>)>().iter(world).min().unwrap()
}

fn home(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap()
}

fn player(app: &App, who: Entity) -> Player {
    app.world().get::<Player>(who).unwrap().clone()
}

fn profile(app: &App, who: Entity) -> Profile {
    app.world().get::<Profile>(who).unwrap().clone()
}

fn pickups(app: &mut App) -> Vec<(Entity, Pickup)> {
    let world = app.world_mut();
    world.query::<(Entity, &Pickup)>().iter(world).map(|(e, p)| (e, p.clone())).collect()
}

/// Standing on our deck at `feet`, looking at `at` (ship coordinates).
fn stand_looking_at(app: &mut App, who: Entity, feet: Vec3, at: Vec3) {
    let ship = home(app);
    let eye = Tuning::default().eye_height;
    let look = at - (feet + Vec3::Y * eye);
    let heading = Vec3::new(look.x, 0.0, look.z).normalize();
    let orientation = Transform::IDENTITY.looking_to(heading, Vec3::Y).rotation;
    let pitch = look.y.atan2(Vec3::new(look.x, 0.0, look.z).length());
    app.world_mut().get_mut::<Player>(who).unwrap().mode = Mode::Booted {
        body: ship,
        feet,
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation,
        pitch,
    };
}

fn interact(app: &mut App, who: Entity) {
    app.world_mut().get_mut::<PlayerInput>(who).unwrap().presses.interact.press();
    ticks(app, 2);
}

fn kill(app: &mut App, who: Entity) {
    *app.world_mut().get_mut::<Vitals>(who).unwrap() = Vitals { health: 0.0, since_hurt: 0.0 };
    ticks(app, 2);
}

fn ask(app: &mut App, who: Entity, op: ProfileOp) {
    app.world_mut().resource_mut::<ProfileOrders>().0.push((who, ProfileWire::Op(op)));
    ticks(app, 2);
}

const DECK: Vec3 = Vec3::new(0.0, 0.01, 10.0);

fn rare() -> Weapon {
    Weapon::new(GunKind::Rifle, 404, Rarity::Rare)
}

/// The local player, with a profile and a rare rifle in hand.
fn armed(app: &mut App) -> Entity {
    let me = me(app);
    app.world_mut().entity_mut(me).insert(Profile::default());
    let mut p = app.world_mut().get_mut::<Player>(me).unwrap();
    p.loadout.inventory.take(Item::Weapon(rare()));
    me
}

#[test]
fn what_you_carry_stays_yours_through_a_scenario_change() {
    let mut app = app();
    let me = armed(&mut app);
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
    ticks(&mut app, 3);
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Field);
    ticks(&mut app, 3);
    assert_eq!(player(&app, me).loadout.inventory.weapons[0], Some(rare()));
}

#[test]
fn what_you_leave_where_you_die_is_yours_alone() {
    let mut app = app();
    let me = armed(&mut app);
    let ship = home(&mut app);
    let other = app.world_mut().spawn((new_player(ship, 1), Vitals::full(&Tuning::default()))).id();
    stand_looking_at(&mut app, me, DECK, DECK + Vec3::new(0.0, 0.0, -2.0));
    kill(&mut app, me);
    let (_, kit) = pickups(&mut app).pop().expect("no kit");
    assert_eq!(kit.owner, Some(me));
    assert_eq!(profile(&app, me).out, vec![rare()], "the rifle isn't noted as out");
    // A crewmate can't take it.
    let feet = Vec3::new(0.0, 0.01, 11.5);
    stand_looking_at(&mut app, other, feet, kit.at);
    interact(&mut app, other);
    assert!(player(&app, other).events.iter().any(|(_, e)| e == PlayerEvent::NotYours));
    assert_eq!(player(&app, other).loadout.inventory.weapons[0], None);
    // Its owner can.
    stand_looking_at(&mut app, me, feet, kit.at);
    interact(&mut app, me);
    assert_eq!(player(&app, me).loadout.inventory.weapons[0], Some(rare()));
    assert!(profile(&app, me).out.is_empty());
    assert!(pickups(&mut app).is_empty());
}

#[test]
fn gear_left_behind_is_back_in_the_stash_after_the_mission() {
    let mut app = app();
    let me = armed(&mut app);
    stand_looking_at(&mut app, me, DECK, DECK + Vec3::new(0.0, 0.0, -2.0));
    kill(&mut app, me);
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Field);
    ticks(&mut app, 3);
    let p = profile(&app, me);
    assert!(p.out.is_empty());
    assert_eq!(p.stash.contains(&rare()), RECOVER_UNCLAIMED);
}

#[test]
fn a_gun_put_down_for_a_crewmate_becomes_theirs() {
    let mut app = app();
    let me = armed(&mut app);
    let ship = home(&mut app);
    let other = app.world_mut().spawn((new_player(ship, 1), Vitals::full(&Tuning::default()))).id();
    app.world_mut().get_mut::<Player>(me).unwrap().loadout.current = 0;
    stand_looking_at(&mut app, me, DECK, DECK + Vec3::new(0.0, 0.0, -2.0));
    app.world_mut().get_mut::<PlayerInput>(me).unwrap().presses.drop.press();
    ticks(&mut app, 2);
    assert_eq!(profile(&app, me).out, vec![rare()]);
    let (_, dropped) = pickups(&mut app).pop().expect("nothing dropped");
    stand_looking_at(&mut app, other, DECK + Vec3::Z * 0.5, dropped.at);
    interact(&mut app, other);
    assert_eq!(player(&app, other).loadout.inventory.weapons[0], Some(rare()));
    assert!(profile(&app, me).out.is_empty(), "still noted as mine");
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Field);
    ticks(&mut app, 3);
    assert!(!profile(&app, me).stash.contains(&rare()), "came back to me too");
}

#[test]
fn the_armory_moves_guns_between_the_stash_and_your_hands() {
    let mut app = app();
    let me = armed(&mut app);
    let smg = Weapon::new(GunKind::Smg, 8, Rarity::Uncommon);
    app.world_mut().get_mut::<Profile>(me).unwrap().stash.push(smg);
    stand_looking_at(&mut app, me, DECK, DECK + Vec3::new(0.0, 0.0, -2.0));
    // The rifle to the stash, the SMG from it.
    ask(&mut app, me, ProfileOp::Store(Place::Weapon(0)));
    ask(&mut app, me, ProfileOp::Take(0));
    let (p, pr) = (player(&app, me), profile(&app, me));
    assert_eq!(p.loadout.inventory.weapons[0], Some(smg));
    assert_eq!(pr.stash, vec![rare()]);
    // Standard issue is free, and goes back on the rack.
    ask(&mut app, me, ProfileOp::TakeIssue(GunKind::Rifle));
    assert_eq!(player(&app, me).loadout.inventory.weapons[1], Some(Weapon::issue(GunKind::Rifle)));
    ask(&mut app, me, ProfileOp::Store(Place::Weapon(1)));
    assert_eq!(player(&app, me).loadout.inventory.weapons[1], None);
    assert_eq!(profile(&app, me).stash, vec![rare()], "issue went in the stash");
    // Not from off the ship.
    app.world_mut().get_mut::<Player>(me).unwrap().mode = Mode::Eva {
        position: Vec3::new(0.0, 0.0, 300.0),
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    ask(&mut app, me, ProfileOp::Take(0));
    assert_eq!(profile(&app, me).stash, vec![rare()], "the armory worked from space");
}

#[test]
fn a_mission_pays_its_crew_when_it_ends() {
    use hullbreakers::sim::mission::{Mission, Phase};
    let mut app = app();
    let me = armed(&mut app);
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
    ticks(&mut app, 3);
    // Abandoning it pays nothing.
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Field);
    ticks(&mut app, 3);
    assert_eq!(profile(&app, me).salvage, 0);
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
    ticks(&mut app, 3);
    {
        let world = app.world_mut();
        let mut mission = world.query::<&mut Mission>().single_mut(world).unwrap();
        let tick = mission.started + 60;
        mission.decide(Phase::Won, tick, "COURSE CLEAR");
    }
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Field);
    ticks(&mut app, 3);
    let p = profile(&app, me);
    assert!(p.salvage >= 150 && p.xp >= 200, "{p:?}");
    assert_eq!(p.missions, 1);
    let world = app.world_mut();
    let mission = world.query::<&Mission>().single(world).unwrap();
    let rewards = &mission.debrief.as_ref().expect("no debrief").rewards;
    assert_eq!(rewards.len(), 1);
    assert_eq!(rewards[0].salvage, p.salvage);
}
