//! Scenes, headless: every size builds from its seed and runs; a salvage
//! carried into our hold is the main objective done, the jump window
//! opens, and the wall's field button jumps the crew out, won.

use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::sim::SimPlugin;
use hullbreakers::sim::asteroids::{Asteroid, RockField};
use hullbreakers::sim::bots::Robot;
use hullbreakers::sim::items::CargoKind;
use hullbreakers::sim::mission::{Director, Mission, Phase, ScenarioKind};
use hullbreakers::sim::pickups::Pickup;
use hullbreakers::sim::player::Player;
use hullbreakers::sim::scene::{Encounter, SceneSeed, SceneSize, plan};
use hullbreakers::sim::ship::{HomeShip, Site};
use hullbreakers::sim::tuning::Tuning;

fn app(robots: bool) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, AssetPlugin::default(), bevy::mesh::MeshPlugin));
    app.add_plugins(PhysicsPlugins::default());
    app.insert_resource(Tuning::default());
    app.add_plugins(SimPlugin {
        asteroids: false,
        local_player: true,
    });
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)));
    app.world_mut().resource_mut::<Director>().robots = robots;
    app.finish();
    app.cleanup();
    for _ in 0..4 {
        app.update();
    }
    app
}

fn mission(app: &mut App) -> Mission {
    app.world_mut().query::<&Mission>().single(app.world()).unwrap().clone()
}

fn me(app: &mut App) -> Entity {
    let world = app.world_mut();
    world.query_filtered::<Entity, (With<Player>, Without<Robot>)>().single(world).unwrap()
}

fn count<F: bevy::ecs::query::QueryFilter>(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query_filtered::<Entity, F>().iter(world).count()
}

/// Every size builds its planned sites and grows its field; a crewed site
/// has robots aboard; the board names the scene.
#[test]
fn every_size_builds_from_its_seed() {
    for size in SceneSize::ALL {
        for seed in 0..3u64 {
            let scene = SceneSeed { seed, size };
            let planned = plan(scene);
            let mut app = app(true);
            app.world_mut().resource_mut::<Director>().start_scene(scene);
            for _ in 0..40 {
                app.update();
            }
            let m = mission(&mut app);
            assert_eq!(m.kind, ScenarioKind::Scene, "{scene:?}");
            assert_eq!(m.phase, Phase::Underway);
            assert!(m.status.first().is_some_and(|l| l.contains(&seed.to_string())), "{scene:?}: board {:?}", m.status);
            assert_eq!(count::<With<Site>>(&mut app), planned.sites.len(), "{scene:?}: sites");
            assert_eq!(count::<With<HomeShip>>(&mut app), 1);
            let field = {
                let world = app.world_mut();
                world.query::<&RockField>().single(world).unwrap().count as usize
            };
            assert_eq!(field, size.rocks());
            // Grown a batch a frame; after 40 frames there are plenty.
            let rocks = count::<With<Asteroid>>(&mut app);
            assert!(rocks > 200, "{scene:?}: {rocks} rocks grown");
            let crewed = planned.sites.iter().any(|s| s.crewed);
            let robots = count::<With<Robot>>(&mut app);
            assert_eq!(robots > 0, crewed, "{scene:?}: {robots} robots, crewed {crewed}");
            // The loot: crates aboard the sites and adrift.
            let crates = count::<With<Pickup>>(&mut app);
            assert!(crates >= planned.drifting.len(), "{scene:?}: {crates} pickups");
            assert!(!m.objective.is_empty());
            assert!(!m.markers.is_empty(), "{scene:?}: nothing marked");
        }
    }
}

/// A seed whose main objective is a salvage set out from the start.
fn salvage_seed() -> (SceneSeed, CargoKind) {
    for seed in 0..200u64 {
        let scene = SceneSeed { seed, size: SceneSize::Tight };
        let p = plan(scene);
        if let Encounter::Salvage { cargo, released_by: None } = &p.sites[p.main].encounter {
            return (scene, *cargo);
        }
    }
    panic!("no seed plans a plain salvage as its main")
}

#[test]
fn a_salvage_home_opens_the_window_and_the_wall_jumps_out_won() {
    let (scene, cargo) = salvage_seed();
    let mut app = app(false);
    app.world_mut().resource_mut::<Director>().start_scene(scene);
    for _ in 0..10 {
        app.update();
    }
    let m = mission(&mut app);
    assert!(m.objective.starts_with("Main:"), "{:?}", m.objective);
    assert!(m.status.iter().any(|l| l.starts_with("MAIN:")), "{:?}", m.status);
    assert!(!m.status.iter().any(|l| l.starts_with("Jump in")));
    // The prize lies at its site.
    let prize = {
        let world = app.world_mut();
        world.query::<&Pickup>().iter(world).filter(|p| p.items.iter().any(|i| matches!(i, hullbreakers::sim::items::Item::Cargo(c) if *c == cargo))).count()
    };
    assert_eq!(prize, 1);
    // Someone on our deck holding it: it's home.
    let e = me(&mut app);
    {
        let mut p = app.world_mut().get_mut::<Player>(e).unwrap();
        if cargo.bulky() {
            p.loadout.inventory.carrying = Some(cargo);
        } else {
            p.loadout.inventory.pack[0] = Some(hullbreakers::sim::items::Item::Cargo(cargo));
        }
    }
    for _ in 0..3 {
        app.update();
    }
    let m = mission(&mut app);
    assert_eq!(m.phase, Phase::Underway);
    assert!(m.report.iter().any(|l| l.starts_with("Main objective done")), "{:?}", m.report);
    assert!(m.status.iter().any(|l| l.starts_with("Jump in")), "{:?}", m.status);
    assert!(m.objective.starts_with("Jump window open"), "{:?}", m.objective);
    // The wall: jump now.
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Field);
    app.update();
    app.update();
    let m = mission(&mut app);
    assert_eq!(m.kind, ScenarioKind::Field);
    let debrief = m.debrief.expect("a debrief");
    assert_eq!(debrief.scenario, ScenarioKind::Scene);
    assert_eq!(debrief.phase, Phase::Won);
    assert_eq!(debrief.outcome, "JUMP COMPLETE");
    assert!(debrief.results.iter().any(|(n, v)| n == "main_done" && *v == 1.0), "{:?}", debrief.results);
    // What it pays (the test's player has no profile to pay into).
    let pay = hullbreakers::sim::progression::reward(ScenarioKind::Scene, &debrief.results, &[], true, "me".into()).expect("a scene pays");
    assert!(pay.salvage > 0 && pay.lines.iter().any(|l| l.contains("Main objective")), "{pay:?}");
}

#[test]
fn jumping_without_the_main_is_the_scene_abandoned() {
    let (scene, _) = salvage_seed();
    let mut app = app(false);
    app.world_mut().resource_mut::<Director>().start_scene(scene);
    for _ in 0..10 {
        app.update();
    }
    app.world_mut().resource_mut::<Director>().start(ScenarioKind::Field);
    app.update();
    app.update();
    let m = mission(&mut app);
    let debrief = m.debrief.expect("a debrief");
    assert_eq!(debrief.phase, Phase::Lost);
    assert_eq!(debrief.outcome, "SCENE ABANDONED");
}

#[test]
fn the_window_closing_jumps_the_crew_out() {
    let (scene, cargo) = salvage_seed();
    let mut app = app(false);
    app.world_mut().resource_mut::<Director>().start_scene(scene);
    for _ in 0..10 {
        app.update();
    }
    let e = me(&mut app);
    {
        let mut p = app.world_mut().get_mut::<Player>(e).unwrap();
        if cargo.bulky() {
            p.loadout.inventory.carrying = Some(cargo);
        } else {
            p.loadout.inventory.pack[0] = Some(hullbreakers::sim::items::Item::Cargo(cargo));
        }
    }
    let window = plan(scene).window;
    for _ in 0..((window + 2.0) * 60.0) as usize {
        app.update();
        if mission(&mut app).phase != Phase::Underway {
            break;
        }
    }
    let m = mission(&mut app);
    assert_eq!(m.phase, Phase::Won, "{:?}", m.status);
    // The ship's a home: standing on it counts as aboard.
    assert!(m.results.iter().any(|(n, v)| n == "aboard" && *v == 1.0), "{:?}", m.results);
}
