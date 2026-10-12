//! Items as they flow through play: picking up, dropping, the inventory
//! screen's changes, the weapons locker, death drops, robot loot, crates,
//! and cargo that fills both hands.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::bots::{Affixes, Robot, new_robot};
use hullbreakers::sim::events::PlayerEvent;
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::items::{AmmoKind, CargoKind, Item, ItemOp, PACK_SLOTS, Place, Rarity, WEAPON_SLOTS, Weapon};
use hullbreakers::sim::mission::{Director, ScenarioKind};
use hullbreakers::sim::pickups::{Pickup, PickupKind};
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::ship::{HomeShip, ShipHull};
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
    world.query_filtered::<Entity, (With<PlayerInput>, Without<Robot>)>().single(world).unwrap()
}

fn player(app: &mut App) -> Player {
    let me = me(app);
    app.world().get::<Player>(me).unwrap().clone()
}

fn set_player(app: &mut App, f: impl FnOnce(&mut Player)) {
    let me = me(app);
    f(&mut app.world_mut().get_mut::<Player>(me).unwrap());
}

fn input(app: &mut App, f: impl FnOnce(&mut PlayerInput)) {
    let me = me(app);
    f(&mut app.world_mut().get_mut::<PlayerInput>(me).unwrap());
}

fn home(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap()
}

fn pickups(app: &mut App) -> Vec<(Entity, Pickup)> {
    let world = app.world_mut();
    world.query::<(Entity, &Pickup)>().iter(world).map(|(e, p)| (e, p.clone())).collect()
}

fn eye_height() -> f32 {
    Tuning::default().eye_height
}

/// Standing on the home ship's deck aft at `feet`, looking at `at` (ship
/// coordinates).
fn stand_looking_at(app: &mut App, feet: Vec3, at: Vec3) {
    let ship = home(app);
    let look = at - (feet + Vec3::Y * eye_height());
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

/// A pickup on the home ship's deck at `at` (its middle).
fn lay_pickup(app: &mut App, at: Vec3, items: Vec<Item>) -> Entity {
    let ship = home(app);
    app.world_mut().spawn(Pickup::dropped((Some(ship), at, Vec3::ZERO), 0, items)).id()
}

fn interact(app: &mut App) {
    input(app, |i| i.presses.interact.press());
    ticks(app, 2);
}

fn ask(app: &mut App, op: ItemOp) {
    input(app, |i| {
        i.ask(op);
    });
    ticks(app, 2);
}

fn events(app: &mut App) -> Vec<PlayerEvent> {
    player(app).events.iter().map(|(_, e)| e).collect()
}

fn fired(app: &mut App) -> usize {
    events(app).iter().filter(|e| matches!(e, PlayerEvent::Fired { .. })).count()
}

const DECK: Vec3 = Vec3::new(0.0, 0.01, 10.0);

#[test]
fn dropping_the_gun_in_hand_leaves_it_on_the_deck_to_take_back() {
    let mut app = app();
    set_player(&mut app, |p| {
        p.loadout.restock();
        p.loadout.current = 0;
    });
    stand_looking_at(&mut app, DECK, DECK + Vec3::new(0.0, 0.0, -2.0));
    let smg = player(&mut app).loadout.inventory.weapons[0].unwrap();
    input(&mut app, |i| i.presses.drop.press());
    ticks(&mut app, 2);
    assert_eq!(player(&mut app).loadout.inventory.weapons[0], None, "still in hand");
    let dropped = pickups(&mut app);
    assert_eq!(dropped.len(), 1);
    let (_, pickup) = &dropped[0];
    assert_eq!(pickup.items, vec![Item::Weapon(smg)]);
    assert_eq!(pickup.body, Some(home(&mut app)));
    assert!(pickup.at.z < DECK.z - 0.3, "not in front: {}", pickup.at);
    // Look down at it, and take it back: into the first free weapon slot.
    stand_looking_at(&mut app, DECK + Vec3::Z * 0.5, pickup.at);
    interact(&mut app);
    assert_eq!(player(&mut app).loadout.inventory.weapons[0], Some(smg));
    assert!(pickups(&mut app).is_empty(), "the gun is still lying there");
}

#[test]
fn a_full_inventory_says_so_and_takes_only_what_fits() {
    let mut app = app();
    // Every slot but one full.
    set_player(&mut app, |p| {
        for s in 0..(WEAPON_SLOTS + PACK_SLOTS - 1) as u32 {
            p.loadout.inventory.take(Item::Weapon(Weapon::new(GunKind::Smg, s, Rarity::Common)));
        }
    });
    let at = DECK + Vec3::new(0.0, 0.2, -1.5);
    let (a, b) = (Weapon::new(GunKind::Rifle, 900, Rarity::Rare), Weapon::new(GunKind::Rifle, 901, Rarity::Exotic));
    let ammo = Item::Ammo { kind: AmmoKind::Heavy, rounds: 30 };
    let case = lay_pickup(&mut app, at, vec![Item::Weapon(a), ammo, Item::Weapon(b)]);
    stand_looking_at(&mut app, DECK, at);
    interact(&mut app);
    let p = player(&mut app);
    assert_eq!(p.loadout.inventory.pack[PACK_SLOTS - 1], Some(Item::Weapon(a)));
    assert_eq!(p.loadout.inventory.ammo(AmmoKind::Heavy), 30);
    let left = app.world().get::<Pickup>(case).expect("the case went").items.clone();
    assert_eq!(left, vec![Item::Weapon(b)], "what didn't fit stays");
    // Again: nothing fits now.
    interact(&mut app);
    assert!(events(&mut app).contains(&PlayerEvent::InventoryFull));
    assert!(app.world().get::<Pickup>(case).is_some());
}

#[test]
fn the_inventory_screen_moves_and_drops_through_the_input() {
    let mut app = app();
    set_player(&mut app, |p| {
        p.loadout.restock();
        p.loadout.current = 0;
    });
    stand_looking_at(&mut app, DECK, DECK + Vec3::new(0.0, 0.0, -2.0));
    let smg = player(&mut app).loadout.inventory.weapons[0].unwrap();
    // The SMG to the pack: the hand it was in comes up empty.
    ask(&mut app, ItemOp::Move { from: Place::Weapon(0), to: Place::Pack(2) });
    let p = player(&mut app);
    assert_eq!(p.loadout.inventory.pack[2], Some(Item::Weapon(smg)));
    assert_eq!(p.loadout.inventory.weapons[0], None);
    assert!(p.loadout.switching > 0.0, "the hand didn't notice");
    // The same press again does nothing more (input is absolute).
    ticks(&mut app, 3);
    assert_eq!(player(&mut app).loadout.inventory.pack[2], Some(Item::Weapon(smg)));
    // Swap it into the rifle's slot; the rifle goes to the pack.
    let rifle = player(&mut app).loadout.inventory.weapons[1].unwrap();
    ask(&mut app, ItemOp::Move { from: Place::Pack(2), to: Place::Weapon(1) });
    let p = player(&mut app);
    assert_eq!(p.loadout.inventory.weapons[1], Some(smg));
    assert_eq!(p.loadout.inventory.pack[2], Some(Item::Weapon(rifle)));
    // Drop the rifle, and a box of light rounds.
    ask(&mut app, ItemOp::Drop(Place::Pack(2)));
    ask(&mut app, ItemOp::DropAmmo(AmmoKind::Light));
    let p = player(&mut app);
    assert_eq!(p.loadout.inventory.pack[2], None);
    assert_eq!(p.loadout.inventory.ammo(AmmoKind::Light), AmmoKind::Light.cap() - AmmoKind::Light.box_rounds());
    let mut left: Vec<Item> = pickups(&mut app).into_iter().flat_map(|(_, p)| p.items).collect();
    left.sort_by_key(|i| matches!(i, Item::Ammo { .. }));
    assert_eq!(
        left,
        vec![Item::Weapon(rifle), Item::Ammo { kind: AmmoKind::Light, rounds: AmmoKind::Light.box_rounds() }]
    );
}

#[test]
fn the_dead_leave_everything_they_carried_in_one_case() {
    let mut app = app();
    let looted = Weapon::new(GunKind::Rifle, 4, Rarity::Uncommon);
    set_player(&mut app, |p| {
        p.loadout.restock();
        p.loadout.inventory.take(Item::Weapon(looted));
        p.loadout.inventory.take(Item::Cargo(CargoKind::DataCore));
        p.loadout.inventory.take(Item::Cargo(CargoKind::PowerCell));
    });
    stand_looking_at(&mut app, DECK, DECK + Vec3::new(0.0, 0.0, -2.0));
    let carried = player(&mut app).loadout.inventory;
    {
        let me = me(&mut app);
        *app.world_mut().get_mut::<Vitals>(me).unwrap() = Vitals { health: 0.0, since_hurt: 0.0 };
    }
    ticks(&mut app, 2);
    assert_eq!(player(&mut app).loadout.inventory, Default::default(), "respawned carrying");
    let cases = pickups(&mut app);
    assert_eq!(cases.len(), 1);
    let items = &cases[0].1.items;
    let count = |f: fn(&Item) -> bool| items.iter().filter(|i| f(i)).count();
    assert_eq!(count(|i| matches!(i, Item::Weapon(_))), 3);
    assert!(items.contains(&Item::Weapon(looted)));
    assert!(items.contains(&Item::Cargo(CargoKind::DataCore)) && items.contains(&Item::Cargo(CargoKind::PowerCell)));
    for kind in AmmoKind::ALL {
        assert!(items.contains(&Item::Ammo { kind, rounds: carried.ammo(kind) }));
    }
}

#[test]
fn bulky_cargo_takes_both_hands_until_put_down() {
    let mut app = app();
    set_player(&mut app, |p| {
        p.loadout.restock();
        p.loadout.current = 0;
    });
    stand_looking_at(&mut app, DECK, DECK + Vec3::new(0.0, 0.2, -1.5));
    let at = DECK + Vec3::new(0.0, 0.2, -1.5);
    lay_pickup(&mut app, at, vec![Item::Cargo(CargoKind::PowerCell)]);
    interact(&mut app);
    assert_eq!(player(&mut app).loadout.inventory.carrying, Some(CargoKind::PowerCell));
    // Trigger held, grapple pressed: nothing.
    input(&mut app, |i| {
        i.main.held = true;
        i.presses.off.press();
    });
    ticks(&mut app, 60);
    let p = player(&mut app);
    assert_eq!(fired(&mut app), 0, "shot while carrying");
    assert!(p.grapple.is_none() && !events(&mut app).contains(&PlayerEvent::GrappleMissed), "grappled while carrying");
    // Put it down: the SMG comes back up and fires.
    input(&mut app, |i| i.presses.drop.press());
    ticks(&mut app, 60);
    assert_eq!(player(&mut app).loadout.inventory.carrying, None);
    assert!(fired(&mut app) > 0, "never fired after putting it down");
    assert!(pickups(&mut app).iter().any(|(_, p)| p.items == vec![Item::Cargo(CargoKind::PowerCell)]));
}

#[test]
fn robots_leave_something_behind() {
    let mut app = app();
    let ship = home(&mut app);
    let tuning = Tuning::default();
    let feet = Vec3::new(0.0, 0.01, -4.0);
    let mode = Mode::Booted {
        body: ship,
        feet,
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    let robot = app.world_mut().spawn(new_robot(Robot::Gunner, Affixes::NONE, mode, ship, feet, &tuning, 0)).id();
    ticks(&mut app, 2);
    assert!(pickups(&mut app).is_empty());
    app.world_mut().get_mut::<Vitals>(robot).unwrap().health = 0.0;
    ticks(&mut app, 2);
    assert!(app.world().get_entity(robot).is_err(), "not destroyed");
    let left = pickups(&mut app);
    assert_eq!(left.len(), 1);
    let (_, loot) = &left[0];
    assert_eq!(loot.body, Some(ship));
    assert!(loot.at.distance(feet) < 0.5, "not where it fell: {}", loot.at);
    assert!(loot.items.iter().any(|i| matches!(i, Item::Ammo { .. })));
    // Never its own gun.
    assert!(!loot.items.iter().any(|i| matches!(i, Item::Weapon(w) if !GunKind::CREW.contains(&w.frame))));
}

#[test]
fn the_carrier_has_loot_crates_aboard() {
    let mut app = app();
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Boarding);
    ticks(&mut app, 3);
    let carrier = {
        let world = app.world_mut();
        world.query::<(Entity, &ShipHull)>().iter(world).find(|(_, h)| h.0 == "carrier").map(|(e, _)| e).unwrap()
    };
    let crates: Vec<Pickup> = pickups(&mut app).into_iter().map(|(_, p)| p).filter(|p| p.kind == PickupKind::Crate).collect();
    assert_eq!(crates.len(), 5);
    for c in &crates {
        assert_eq!(c.body, Some(carrier));
        assert!(c.items.iter().any(|i| matches!(i, Item::Weapon(_))), "a crate without a gun");
    }
    // A fresh fight, fresh crates.
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Boarding);
    ticks(&mut app, 3);
    assert_eq!(pickups(&mut app).into_iter().filter(|(_, p)| p.kind == PickupKind::Crate).count(), 5);
}

#[test]
fn a_nearly_full_pool_takes_part_of_a_box() {
    let mut app = app();
    let light = AmmoKind::Light;
    set_player(&mut app, |p| *p.loadout.inventory.ammo_mut(light) = light.cap() - 10);
    let at = DECK + Vec3::new(0.0, 0.2, -1.5);
    let case = lay_pickup(&mut app, at, vec![Item::Ammo { kind: light, rounds: 60 }]);
    stand_looking_at(&mut app, DECK, at);
    interact(&mut app);
    assert_eq!(player(&mut app).loadout.inventory.ammo(light), light.cap());
    let left = app.world().get::<Pickup>(case).expect("the box went").items.clone();
    assert_eq!(left, vec![Item::Ammo { kind: light, rounds: 50 }], "rounds lost or made");
}

#[test]
fn what_lies_on_a_ship_that_goes_is_left_adrift() {
    let mut app = app();
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Boarding);
    ticks(&mut app, 3);
    let carrier = {
        let world = app.world_mut();
        world.query::<(Entity, &ShipHull)>().iter(world).find(|(_, h)| h.0 == "carrier").map(|(e, _)| e).unwrap()
    };
    let at = app.world().get::<Position>(carrier).unwrap().0;
    app.world_mut().entity_mut(carrier).despawn();
    ticks(&mut app, 2);
    let crates: Vec<Pickup> = pickups(&mut app).into_iter().map(|(_, p)| p).filter(|p| p.kind == PickupKind::Crate).collect();
    assert_eq!(crates.len(), 5);
    for c in &crates {
        assert_eq!(c.body, None, "still on a ship that's gone");
        assert!(c.at.distance(at) < 40.0, "{} far from the carrier at {at}", c.at);
    }
}

#[test]
fn two_takers_in_one_tick_dont_both_get_the_same_gun() {
    use hullbreakers::sim::player::new_player;
    let mut app = app();
    let ship = home(&mut app);
    let at = DECK + Vec3::new(0.0, 0.2, -1.5);
    let gun = Weapon::new(GunKind::Smg, 77, Rarity::Rare);
    lay_pickup(&mut app, at, vec![Item::Weapon(gun)]);
    stand_looking_at(&mut app, DECK, at);
    let me = me(&mut app);
    let other = app.world_mut().spawn((new_player(ship, 1), Vitals::full(&Tuning::default()))).id();
    let mode = app.world().get::<Player>(me).unwrap().mode;
    app.world_mut().get_mut::<Player>(other).unwrap().mode = mode;
    for p in [me, other] {
        app.world_mut().get_mut::<PlayerInput>(p).unwrap().presses.interact.press();
    }
    ticks(&mut app, 2);
    let mine = app.world().get::<Player>(me).unwrap().loadout.inventory.weapons[0];
    let theirs = app.world().get::<Player>(other).unwrap().loadout.inventory.weapons[0];
    assert!(mine.is_some() != theirs.is_some(), "mine {mine:?}, theirs {theirs:?}");
    assert!(pickups(&mut app).is_empty());
}
