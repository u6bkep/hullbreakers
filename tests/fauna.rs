//! The asteroid field's life: limpets that catch the crew, grazers that
//! swim in pulses, carry riders and stampede.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::asteroids::{self, Asteroid};
use hullbreakers::sim::bots::{Affixes, Robot, new_robot};
use hullbreakers::sim::fauna::{self, Grazer, Mood};
use hullbreakers::sim::input::PlayerInput;
use hullbreakers::sim::player::{Grapple, Mode, Player, new_player};
use hullbreakers::sim::ship::HomeShip;
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::shots::{Shot, Shots, Team};
use hullbreakers::sim::vitals::{Damage, Vitals};
use hullbreakers::sim::weapons::ShotKind;

fn app_with(asteroids: bool) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, AssetPlugin::default(), bevy::mesh::MeshPlugin));
    app.add_plugins(PhysicsPlugins::default());
    app.insert_resource(Tuning::default());
    app.add_plugins(SimPlugin { asteroids, local_player: true });
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)));
    app.finish();
    app.cleanup();
    ticks(&mut app, 4);
    app
}

fn app() -> App {
    app_with(false)
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
    world.query_filtered::<Entity, (With<PlayerInput>, Without<Robot>)>().iter(world).next().unwrap()
}

fn health(app: &App, e: Entity) -> f32 {
    app.world().get::<Vitals>(e).unwrap().health
}

fn player(app: &App, e: Entity) -> Player {
    app.world().get::<Player>(e).unwrap().clone()
}

fn set(app: &mut App, e: Entity, f: impl FnOnce(&mut Player)) {
    f(&mut app.world_mut().get_mut::<Player>(e).unwrap());
}

/// A rock at rest at `at`, slowly turning.
fn rock(app: &mut App, at: Vec3, radius: f32) -> Entity {
    let world = app.world_mut();
    asteroids::spawn_rocks(&mut world.commands(), 11, &[(at, radius)]);
    world.flush();
    world
        .query_filtered::<(Entity, &Position), With<Asteroid>>()
        .iter(world)
        .find(|(_, p)| p.0.distance(at) < 0.01)
        .unwrap()
        .0
}

/// Standing on `rock` somewhere picked by `seed`.
fn on_rock(app: &App, rock: Entity, seed: u64) -> Mode {
    let collider = app.world().get::<Collider>(rock).unwrap();
    let radius = app.world().get::<Asteroid>(rock).unwrap().radius;
    fauna::cling_spot(seed, rock, collider, radius).unwrap()
}

fn limpet_on(app: &mut App, rock: Entity, seed: u64) -> Entity {
    let mode = on_rock(app, rock, seed);
    let Mode::Booted { feet, .. } = mode else { unreachable!() };
    let robot = app.world_mut().spawn(new_robot(Robot::Limpet, Affixes::NONE, mode, rock, feet, &Tuning::default(), 0)).id();
    ticks(app, 1);
    robot
}

/// Where a player's middle is in the world.
fn center(app: &App, e: Entity) -> Vec3 {
    let p = player(app, e);
    let t = app.world().get::<hullbreakers::sim::bots::BodyTuning>(e).map_or(Tuning::default(), |t| t.0.clone());
    match p.mode {
        Mode::Eva { position, .. } => position,
        Mode::Booted { body, feet, up, .. } => {
            let pos = app.world().get::<Position>(body).unwrap().0;
            let rot = app.world().get::<Rotation>(body).unwrap().0;
            pos + rot * (feet + up * t.capsule_height / 2.0)
        }
    }
}

fn eva(position: Vec3) -> Mode {
    Mode::Eva {
        position,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    }
}

/// The crew member, floating free at `at`.
fn adrift(app: &mut App, me: Entity, at: Vec3) {
    set(app, me, |p| {
        p.mode = eva(at);
        p.reference_body = None;
    });
}

// --- Limpets ----------------------------------------------------------------

#[test]
fn a_limpet_feels_someone_on_its_rock_flies_at_them_and_bites() {
    let mut app = app();
    let rock = rock(&mut app, Vec3::new(0.0, 0.0, -90.0), 5.0);
    let limpet = limpet_on(&mut app, rock, 1);
    let me = crew(&mut app);
    // Standing somewhere else on its rock, out of its sight round the
    // curve maybe: it feels them through the rock.
    let spot = on_rock(&app, rock, 5);
    set(&mut app, me, |p| {
        p.mode = spot;
        p.reference_body = Some(rock);
    });
    let full = health(&app, me);
    let mut closest = f32::MAX;
    for _ in 0..60 * 15 {
        ticks(&mut app, 1);
        closest = closest.min(center(&app, limpet).distance(center(&app, me)));
    }
    assert!(closest < 1.3, "never got onto them (closest {closest} m)");
    assert!(health(&app, me) < full - 10.0, "never bit them: health {}", health(&app, me));
}

#[test]
fn a_limpet_hauls_in_someone_drifting_past_and_rides_them() {
    let mut app = app();
    let rock = rock(&mut app, Vec3::new(0.0, 0.0, -90.0), 4.0);
    let limpet = limpet_on(&mut app, rock, 2);
    let me = crew(&mut app);
    // Drifting 9 m off the rock's surface, beside the limpet.
    let at = center(&app, limpet);
    let out = (at - Vec3::new(0.0, 0.0, -90.0)).normalize();
    adrift(&mut app, me, at + out * 9.0);
    let start = center(&app, me);
    let full = health(&app, me);
    let mut pulled = 0.0f32;
    let mut riding = false;
    for _ in 0..60 * 12 {
        ticks(&mut app, 1);
        pulled = pulled.max(start.distance(center(&app, me)));
        riding |= matches!(player(&app, limpet).mode, Mode::Eva { .. }) && center(&app, limpet).distance(center(&app, me)) < 1.0;
    }
    assert!(pulled > 4.0, "never hauled them in (moved {pulled} m)");
    assert!(riding, "never rode them");
    assert!(health(&app, me) < full - 10.0, "never bit them: health {}", health(&app, me));
}

#[test]
fn a_crewmate_reeling_a_limpet_tears_it_off() {
    let mut app = app();
    let ship = ship(&mut app);
    let me = crew(&mut app);
    let mate = app.world_mut().spawn((new_player(ship, 1), Vitals::full(&Tuning::default()))).id();
    ticks(&mut app, 2);
    let rock = rock(&mut app, Vec3::new(0.0, 0.0, -90.0), 4.0);
    let limpet = limpet_on(&mut app, rock, 2);
    let at = center(&app, limpet);
    let out = (at - Vec3::new(0.0, 0.0, -90.0)).normalize();
    adrift(&mut app, me, at + out * 8.0);
    adrift(&mut app, mate, at + out * 8.0 + Vec3::X * 12.0);
    // Until it's on them.
    let mut on = false;
    for _ in 0..60 * 12 {
        ticks(&mut app, 1);
        if matches!(player(&app, limpet).mode, Mode::Eva { .. }) && center(&app, limpet).distance(center(&app, me)) < 1.0 {
            on = true;
            break;
        }
    }
    assert!(on, "it never got onto them");
    // The crewmate hooks it and reels.
    let length = center(&app, mate).distance(center(&app, limpet));
    set(&mut app, mate, |p| {
        p.grapple = Some(Grapple {
            target: limpet,
            anchor: Vec3::ZERO,
            length,
            tension: 0.0,
        })
    });
    let mut torn = false;
    for _ in 0..60 * 4 {
        app.world_mut().get_mut::<PlayerInput>(mate).unwrap().reel_in = true;
        ticks(&mut app, 1);
        if player(&app, limpet).grapple.is_none_or(|g| g.target != me) {
            torn = true;
            break;
        }
    }
    assert!(torn, "it held on");
    // Torn off, it's stunned a moment: it lets them be. (They were dragged
    // along with it, so they're still beside it; what comes next is up to
    // the crew.)
    ticks(&mut app, 2);
    let before = health(&app, me);
    ticks(&mut app, 60);
    assert!(player(&app, limpet).grapple.is_none(), "it's back on them already");
    assert!(health(&app, me) >= before, "still biting after it was torn off");
}

#[test]
fn a_limpet_adrift_clings_to_a_rock_again() {
    let mut app = app();
    let rock = rock(&mut app, Vec3::new(0.0, 0.0, -90.0), 3.0);
    let ship = ship(&mut app);
    let at = Vec3::new(0.0, 0.0, -90.0 + 10.0);
    let limpet = app.world_mut().spawn(new_robot(Robot::Limpet, Affixes::NONE, eva(at), ship, at, &Tuning::default(), 0)).id();
    let mut clung = false;
    for _ in 0..60 * 12 {
        ticks(&mut app, 1);
        if player(&app, limpet).mode.body() == Some(rock) {
            clung = true;
            break;
        }
    }
    assert!(clung, "never got back onto a rock: {:?}", player(&app, limpet).mode);
}

// --- Grazers ----------------------------------------------------------------

fn grazer_at(app: &mut App, seed: u32, mood: Mood, at: Vec3, turn: Quat) -> Entity {
    let world = app.world_mut();
    let tick = world.resource::<hullbreakers::sim::SimTick>().0;
    let e = world.spawn(fauna::grazer(seed, mood, tick, at, turn, Vec3::ZERO, false)).id();
    ticks(app, 1);
    e
}

fn grazer(app: &App, e: Entity) -> Grazer {
    *app.world().get::<Grazer>(e).unwrap()
}

fn pos(app: &App, e: Entity) -> Vec3 {
    app.world().get::<Position>(e).unwrap().0
}

fn vel(app: &App, e: Entity) -> Vec3 {
    app.world().get::<LinearVelocity>(e).unwrap().0
}

#[test]
fn a_grazer_swims_to_a_rock_and_grazes_it_changing_course_only_in_pulses() {
    let mut app = app();
    let food = Vec3::new(30.0, 0.0, -90.0);
    rock(&mut app, food, 1.5);
    let g = grazer_at(&mut app, 3, Mood::Cruising, Vec3::new(0.0, 0.0, -90.0), Quat::IDENTITY);
    let mut grazed = false;
    let mut last = vel(&app, g);
    for _ in 0..60 * 50 {
        ticks(&mut app, 1);
        let state = grazer(&app, g);
        let tick = app.world().resource::<hullbreakers::sim::SimTick>().0;
        let v = vel(&app, g);
        // Free flight between pulses: nothing else changes its motion
        // (it touches nothing out here).
        if state.last_pulse(tick) != tick {
            assert!((v - last).length() < 1e-3, "its velocity changed between pulses: {last} -> {v}");
        }
        last = v;
        if state.mood == Mood::Grazing {
            grazed = true;
            break;
        }
    }
    assert!(grazed, "never got to graze; at {} ({:?})", pos(&app, g), grazer(&app, g).mood);
    assert!(pos(&app, g).distance(food) < 1.5 + 4.5, "grazing from {} m off", pos(&app, g).distance(food));
}

#[test]
fn hurting_one_grazer_stampedes_its_herd() {
    let mut app = app();
    let food = Vec3::new(0.0, 0.0, -100.0);
    rock(&mut app, food, 2.0);
    // A string of them, each within earshot of the next, the last well
    // beyond the first's.
    let herd: Vec<Entity> = (0..4)
        .map(|i| grazer_at(&mut app, 10 + i, Mood::Grazing, food + Vec3::new(4.0 + 25.0 * i as f32, 0.0, 0.0), Quat::IDENTITY))
        .collect();
    let me = crew(&mut app);
    app.world_mut().write_message(Damage {
        target: herd[0],
        amount: 5.0,
        source: Some(me),
        weak: false,
        from: Some(food + Vec3::new(-20.0, 0.0, 0.0)),
        steady: false,
    });
    ticks(&mut app, 60 * 3);
    for (i, g) in herd.iter().enumerate() {
        assert_eq!(grazer(&app, *g).mood, Mood::Startled, "grazer {i} didn't bolt");
        assert!(vel(&app, *g).length() > 5.0, "grazer {i} only at {} m/s", vel(&app, *g).length());
    }
}

#[test]
fn a_crew_member_can_ride_a_grazer() {
    let mut app = app();
    // A rock to swim to, 40 m off to its side: it turns and swims, the
    // rider standing on its bell.
    rock(&mut app, Vec3::new(40.0, 0.0, -100.0), 1.5);
    let g = grazer_at(&mut app, 5, Mood::Cruising, Vec3::new(0.0, 0.0, -100.0), Quat::IDENTITY);
    let me = crew(&mut app);
    set(&mut app, me, |p| {
        p.mode = Mode::Booted {
            body: g,
            feet: Vec3::new(0.0, 0.71, 0.0),
            up: Vec3::Y,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            pitch: 0.0,
        };
        p.reference_body = Some(g);
    });
    let start = center(&app, me);
    ticks(&mut app, 60 * 15);
    assert_eq!(player(&app, me).mode.body(), Some(g), "thrown off: {:?}", player(&app, me).mode);
    assert!(center(&app, me).distance(start) > 10.0, "carried only {} m", center(&app, me).distance(start));
}

#[test]
fn hooking_a_grazer_makes_it_bolt_towing_whoever_hooked_it() {
    let mut app = app();
    let at = Vec3::new(0.0, 0.0, -100.0);
    let g = grazer_at(&mut app, 6, Mood::Grazing, at, Quat::IDENTITY);
    let me = crew(&mut app);
    let from = at + Vec3::new(-12.0, 0.0, 0.0);
    adrift(&mut app, me, from);
    set(&mut app, me, |p| {
        p.grapple = Some(Grapple {
            target: g,
            anchor: Vec3::new(-1.5, 0.0, 0.0),
            length: 10.5,
            tension: 0.0,
        })
    });
    ticks(&mut app, 60 * 5);
    assert_eq!(grazer(&app, g).mood, Mood::Startled);
    let Mode::Eva { velocity, .. } = player(&app, me).mode else { panic!("not adrift") };
    assert!(velocity.length() > 4.0, "towed at only {} m/s", velocity.length());
    assert!(center(&app, me).distance(from) > 10.0, "towed only {} m", center(&app, me).distance(from));
}

#[test]
fn a_bolting_grazer_tramples_whoever_is_in_its_way() {
    let mut app = app();
    let at = Vec3::new(0.0, 0.0, -100.0);
    // Startled from behind (−X), it bolts along +X, where the crew member
    // floats.
    let g = grazer_at(&mut app, 8, Mood::Grazing, at, Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2));
    let me = crew(&mut app);
    adrift(&mut app, me, at + Vec3::new(9.0, 0.0, 0.0));
    let full = health(&app, me);
    app.world_mut().write_message(Damage {
        target: g,
        amount: 1.0,
        source: None,
        weak: false,
        from: Some(at - Vec3::X * 20.0),
        steady: false,
    });
    ticks(&mut app, 60 * 4);
    assert!(health(&app, me) < full, "never trampled");
    let Mode::Eva { velocity, .. } = player(&app, me).mode else { panic!("not adrift") };
    assert!(velocity.x > 2.0, "not knocked aside: {velocity}");
}

// --- The field --------------------------------------------------------------

#[test]
fn the_field_streams_in_limpets_and_herds_but_not_on_the_crew() {
    let mut app = app_with(true);
    ticks(&mut app, 10);
    let world = app.world_mut();
    let limpets: Vec<Vec3> = world
        .query::<(&Robot, &Player)>()
        .iter(world)
        .filter(|(k, _)| **k == Robot::Limpet)
        .filter_map(|(_, p)| match p.mode {
            Mode::Booted { body, .. } => Some(body),
            _ => None,
        })
        .filter_map(|rock| world.get::<Position>(rock).map(|p| p.0))
        .collect();
    let grazers: Vec<Vec3> = world.query_filtered::<&Position, With<Grazer>>().iter(world).map(|p| p.0).collect();
    assert!(limpets.len() >= 2, "{} limpets", limpets.len());
    assert!(grazers.len() >= 3, "{} grazers", grazers.len());
    for p in limpets.iter().chain(&grazers) {
        assert!(p.length() > 60.0, "fauna {} m from the ship", p.length());
    }
}

// --- Shooting them --------------------------------------------------------------

/// A shot of `kind` fired by the crew member from `from` along `dir`, flown
/// (or settled) this tick.
fn shoot(app: &mut App, kind: ShotKind, from: Vec3, dir: Vec3, damage: f32) {
    let me = crew(app);
    let speed = match kind {
        ShotKind::Plasma { .. } => 100.0,
        _ => 800.0,
    };
    app.world_mut().resource_mut::<Shots>().0.push(Shot {
        shooter: me,
        team: Team::Crew,
        damage,
        mass: 0.012,
        position: from,
        velocity: dir.normalize() * speed,
        age: 0.0,
        origin: from,
        kind,
        through: None,
    });
}

/// What one shot of `kind` from under a grazer, straight up at its heart,
/// takes off it.
fn heart_shot(kind: ShotKind) -> f32 {
    let mut app = app();
    let at = Vec3::new(0.0, 0.0, -100.0);
    let g = grazer_at(&mut app, 4, Mood::Grazing, at, Quat::IDENTITY);
    let full = health(&app, g);
    shoot(&mut app, kind, at - Vec3::Y * 4.0, Vec3::Y, 10.0);
    ticks(&mut app, 8);
    full - health(&app, g)
}

#[test]
fn bullets_and_beams_find_a_grazers_heart_arcs_and_bursts_dont() {
    let bullet = heart_shot(ShotKind::Bullet { pierce: 0 });
    let sniper = heart_shot(ShotKind::Bullet { pierce: 1 });
    let laser = heart_shot(ShotKind::Laser { range: 400.0 });
    let arc = heart_shot(ShotKind::Arc { range: 35.0, chain: 2 });
    let plasma = heart_shot(ShotKind::Plasma { splash: 2.5 });
    assert!((bullet - 25.0).abs() < 0.1, "a bullet in its heart took {bullet}");
    assert!((sniper - 25.0).abs() < 0.1, "a sniper round in its heart took {sniper}");
    assert!((laser - 25.0).abs() < 0.1, "a beam on its heart took {laser}");
    assert!((arc - 10.0).abs() < 0.1, "an arc took {arc}");
    assert!((plasma - 10.0).abs() < 0.1, "a plasma bolt took {plasma}");
}

#[test]
fn a_plasma_burst_beside_a_grazer_hurts_nudges_and_startles_it() {
    let mut app = app();
    let at = Vec3::new(0.0, 0.0, -100.0);
    let r = rock(&mut app, at + Vec3::X * 3.0, 0.8);
    let g = grazer_at(&mut app, 4, Mood::Grazing, at, Quat::IDENTITY);
    let full = health(&app, g);
    let before = vel(&app, g);
    // At the rock, from beyond it: the burst is 1–2 m from the grazer's bell.
    let rock_at = pos(&app, r);
    shoot(&mut app, ShotKind::Plasma { splash: 2.5 }, rock_at + Vec3::new(0.0, 0.0, 8.0), Vec3::NEG_Z, 16.0);
    ticks(&mut app, 12);
    assert!(health(&app, g) < full, "the burst didn't hurt it");
    assert!((vel(&app, g) - before).length() > 1e-4, "the burst didn't move it");
    assert_eq!(grazer(&app, g).mood, Mood::Startled, "the burst didn't startle it");
}

#[test]
fn a_bullet_finds_a_limpets_sac() {
    let mut app = app();
    let rock = rock(&mut app, Vec3::new(0.0, 0.0, -90.0), 4.0);
    let limpet = limpet_on(&mut app, rock, 2);
    let Mode::Booted { body, feet, up, orientation, .. } = player(&app, limpet).mode else { unreachable!() };
    let rot = app.world().get::<Rotation>(body).unwrap().0;
    let pos = app.world().get::<Position>(body).unwrap().0;
    // Straight down onto the sac from above its back.
    let sac = pos + rot * (feet + orientation * Vec3::new(0.0, 0.26, 0.2));
    let up = rot * up;
    let full = health(&app, limpet);
    shoot(&mut app, ShotKind::Bullet { pierce: 0 }, sac + up * 3.0, -up, 10.0);
    ticks(&mut app, 4);
    let took = full - health(&app, limpet);
    assert!((took - 25.0).abs() < 0.1, "a bullet in its sac took {took}");
}

#[test]
fn a_grazer_kept_bolting_is_not_rewritten_every_tick() {
    // (Its replicated state goes out whenever it's marked changed.)
    let mut app = app();
    let at = Vec3::new(0.0, 0.0, -100.0);
    let g = grazer_at(&mut app, 6, Mood::Grazing, at, Quat::IDENTITY);
    let me = crew(&mut app);
    adrift(&mut app, me, at + Vec3::new(-12.0, 0.0, 0.0));
    set(&mut app, me, |p| {
        p.grapple = Some(Grapple {
            target: g,
            anchor: Vec3::new(-1.5, 0.0, 0.0),
            length: 10.5,
            tension: 0.0,
        })
    });
    ticks(&mut app, 30);
    assert_eq!(grazer(&app, g).mood, Mood::Startled);
    let changed = |app: &mut App| {
        let world = app.world_mut();
        world.query::<Ref<Grazer>>().get(world, g).unwrap().last_changed()
    };
    let before = changed(&mut app);
    ticks(&mut app, 60);
    assert_eq!(grazer(&app, g).mood, Mood::Startled, "it stopped bolting while hooked");
    assert_eq!(changed(&mut app), before, "rewritten while nothing about it changed");
}
