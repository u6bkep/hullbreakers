//! Profiles over the network: two clients, each with its own
//! `profile.ron`, join a dedicated server; each profile reaches the server
//! as that client's player's, comes back, and is saved in its own file.
//!
//! Lightyear's clock is global (mock_instant), so these tests run one at a
//! time.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::client::save::{ProfileStore, load};
use hullbreakers::net::{LocalPlayer, NetMode, NetPlugin, add_headless_plugins};
use hullbreakers::sim::items::{Item, Rarity, Weapon};
use hullbreakers::sim::player::Player;
use hullbreakers::sim::progression::{Profile, ProfileFile, callsign};
use hullbreakers::sim::weapons::GunKind;
use mock_instant::global::MockClock;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const FRAME: Duration = Duration::from_nanos(16_666_667);

fn free_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn peer(mode: NetMode, store: Option<ProfileStore>) -> App {
    let mut app = App::new();
    add_headless_plugins(&mut app, FRAME);
    if let Some(store) = store {
        app.insert_resource(store);
    }
    app.add_plugins(NetPlugin { mode, lag: None });
    app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    app.finish();
    app.cleanup();
    app
}

/// A profile file in a directory of this test's own.
fn profile_path(test: &str, who: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hullbreakers-{}-{test}-{who}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("profile.ron");
    let _ = std::fs::remove_file(&path);
    path
}

fn store_with(path: &PathBuf, file: &ProfileFile) -> ProfileStore {
    std::fs::write(path, ron::ser::to_string(file).unwrap()).unwrap();
    ProfileStore::open(path.clone())
}

struct Crew {
    server: App,
    clients: Vec<App>,
}

impl Crew {
    fn start(stores: Vec<ProfileStore>) -> Crew {
        let port = free_port();
        let server = peer(NetMode::Server { port }, None);
        let clients = stores
            .into_iter()
            .map(|store| peer(NetMode::Client { server: format!("127.0.0.1:{port}").parse().unwrap() }, Some(store)))
            .collect();
        let mut crew = Crew { server, clients };
        for _ in 0..1500 {
            crew.step(1);
            if (0..crew.clients.len()).all(|i| crew.local(i).is_some()) {
                crew.step(120);
                return crew;
            }
        }
        panic!("the clients never got their players");
    }

    fn step(&mut self, frames: usize) {
        for _ in 0..frames {
            MockClock::advance(FRAME);
            for client in &mut self.clients {
                client.update();
            }
            self.server.update();
        }
    }

    /// Client `i`'s own player and the profile it has of it.
    fn local(&mut self, i: usize) -> Option<(Player, Option<Profile>)> {
        let world = self.clients[i].world_mut();
        world
            .query_filtered::<(&Player, Option<&Profile>), With<LocalPlayer>>()
            .iter(world)
            .next()
            .map(|(p, f)| (p.clone(), f.cloned()))
    }

    /// The server's profiles, by salvage.
    fn server_profiles(&mut self) -> Vec<Profile> {
        let world = self.server.world_mut();
        let mut all: Vec<Profile> = world.query::<&Profile>().iter(world).cloned().collect();
        all.sort_by_key(|p| p.salvage);
        all
    }
}

fn two_profiles(test: &str) -> (PathBuf, PathBuf, ProfileFile, ProfileFile) {
    let (pa, pb) = (profile_path(test, "a"), profile_path(test, "b"));
    let mut a = ProfileFile::fresh();
    a.profile.salvage = 100;
    a.profile.stash.push(Weapon::new(GunKind::Rifle, 31, Rarity::Rare));
    let mut b = ProfileFile::fresh();
    b.profile.salvage = 7;
    b.carried.take(Item::Weapon(Weapon::new(GunKind::Smg, 5, Rarity::Uncommon)));
    (pa, pb, a, b)
}

#[test]
fn each_clients_profile_reaches_the_host_and_comes_back_to_its_file() {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (pa, pb, a, b) = two_profiles("roundtrip");
    let mut crew = Crew::start(vec![store_with(&pa, &a), store_with(&pb, &b)]);
    crew.step(60);
    // The host has both, each on its own player.
    let on_server = crew.server_profiles();
    assert_eq!(on_server.iter().map(|p| p.salvage).collect::<Vec<_>>(), vec![7, 100]);
    assert_eq!(on_server[1].stash, a.profile.stash);
    // And each client its own, carrying what it carried.
    let (player_b, profile_b) = crew.local(1).unwrap();
    assert_eq!(profile_b.map(|p| p.salvage), Some(7));
    assert_eq!(player_b.loadout.inventory.weapons[0], b.carried.weapons[0], "B's gun isn't in its hands");
    // The host changes them (as a mission's pay would); they're saved.
    {
        let world = crew.server.world_mut();
        for mut p in world.query::<&mut Profile>().iter_mut(world) {
            p.salvage += 50;
        }
    }
    crew.step(4 * 60);
    assert_eq!(load(&pa).profile.salvage, 150);
    assert_eq!(load(&pb).profile.salvage, 57);
    assert_eq!(load(&pb).carried.weapons[0], b.carried.weapons[0], "B's file lost its gun");
}

/// Each player goes by their profile's name, everywhere (the replicated
/// `Name`), from joining; a rename in the menu reaches the host, every
/// peer, and the player's own file. A fresh file has a callsign and an id.
#[test]
fn players_go_by_their_profiles_names() {
    use hullbreakers::net::ProfileRequests;
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (pa, pb, mut a, b) = two_profiles("names");
    a.profile.name = "  Vasquez\u{7}  ".into();
    a.profile.id = 41;
    // B's file is from before names: it gets an id and a callsign.
    let mut crew = Crew::start(vec![store_with(&pa, &a), store_with(&pb, &b)]);
    crew.step(60);
    let b_file = load(&pb);
    assert_ne!(b_file.profile.id, 0, "B has no id");
    assert_eq!(b_file.profile.name, callsign(b_file.profile.id));
    let names = |app: &mut App| {
        let world = app.world_mut();
        let mut all: Vec<String> = world
            .query_filtered::<&Name, (With<Player>, Without<hullbreakers::sim::bots::Robot>)>()
            .iter(world)
            .map(|n| n.to_string())
            .collect();
        all.sort();
        all
    };
    let mut expected = vec!["Vasquez".to_string(), callsign(b_file.profile.id)];
    expected.sort();
    assert_eq!(names(&mut crew.server), expected);
    assert_eq!(names(&mut crew.clients[1]), expected, "B doesn't see the crew's names");
    // A renames.
    crew.clients[0].world_mut().resource_mut::<ProfileRequests>().1 = Some("Hicks".into());
    crew.step(4 * 60);
    let mut expected = vec!["Hicks".to_string(), callsign(b_file.profile.id)];
    expected.sort();
    assert_eq!(names(&mut crew.clients[1]), expected);
    assert_eq!(load(&pa).profile.name, "Hicks");
    assert_eq!(load(&pa).profile.id, 41, "the id changed");
}

/// A profile (and its saved snapshot) goes only to its own player's
/// machine; the crew sees only what it needs of it: rank and name.
#[test]
fn a_profile_goes_only_to_its_owner() {
    use hullbreakers::sim::progression::{Rank, SavedProfile, xp_for};
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (pa, pb, mut a, b) = two_profiles("private");
    a.profile.xp = xp_for(4);
    let mut crew = Crew::start(vec![store_with(&pa, &a), store_with(&pb, &b)]);
    crew.step(60);
    for i in 0..2 {
        let world = crew.clients[i].world_mut();
        let seen: Vec<(bool, bool, bool, Option<u8>)> = world
            .query_filtered::<(Has<LocalPlayer>, Has<Profile>, Has<SavedProfile>, Option<&Rank>), (With<Player>, Without<hullbreakers::sim::bots::Robot>)>()
            .iter(world)
            .map(|(local, p, s, r)| (local, p, s, r.map(|r| r.0)))
            .collect();
        assert_eq!(seen.len(), 2, "client {i}: {seen:?}");
        for (local, profile, saved, rank) in seen {
            assert_eq!((profile, saved), (local, local), "client {i}: a profile where it isn't theirs");
            assert!(rank.is_some(), "client {i}: a crewmate's rank didn't arrive");
        }
    }
    // A's rank, as B has it.
    let world = crew.clients[1].world_mut();
    let ranks: Vec<u8> = world.query_filtered::<&Rank, Without<LocalPlayer>>().iter(world).map(|r| r.0).collect();
    assert!(ranks.contains(&4), "{ranks:?}");
}

/// A host's own player takes its profile straight from the file, and
/// saves it there.
#[test]
fn a_hosts_profile_skips_the_network() {
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (pa, _, a, _) = two_profiles("host");
    let mut host = peer(NetMode::Host { port: free_port() }, Some(store_with(&pa, &a)));
    let profile = |host: &mut App| {
        let world = host.world_mut();
        world.query_filtered::<&Profile, With<LocalPlayer>>().iter(world).next().cloned()
    };
    for _ in 0..600 {
        MockClock::advance(FRAME);
        host.update();
        if profile(&mut host).is_some() {
            break;
        }
    }
    assert_eq!(profile(&mut host).map(|p| p.salvage), Some(100));
    {
        let world = host.world_mut();
        for mut p in world.query::<&mut Profile>().iter_mut(world) {
            p.salvage = 1;
        }
    }
    for _ in 0..4 * 60 {
        MockClock::advance(FRAME);
        host.update();
    }
    assert_eq!(load(&pa).profile.salvage, 1);
}

/// What a client's armory asks reaches the host, and the result comes back.
#[test]
fn a_clients_armory_works_through_the_host() {
    use hullbreakers::net::ProfileRequests;
    use hullbreakers::sim::items::Place;
    use hullbreakers::sim::progression::ProfileOp;
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (pa, pb, a, b) = two_profiles("armory");
    let mut crew = Crew::start(vec![store_with(&pa, &a), store_with(&pb, &b)]);
    let gun = b.carried.weapons[0].unwrap();
    crew.clients[1]
        .world_mut()
        .resource_mut::<ProfileRequests>()
        .0
        .push(ProfileOp::Store(hullbreakers::sim::progression::Holding::Carried(Place::Weapon(0), gun)));
    crew.clients[0].world_mut().resource_mut::<ProfileRequests>().0.push(ProfileOp::Take(a.profile.stash[0]));
    crew.step(60);
    let (player_b, profile_b) = crew.local(1).unwrap();
    assert_eq!(profile_b.unwrap().stash, vec![gun], "B's gun isn't in B's stash");
    assert_eq!(player_b.loadout.inventory.weapons[0], None);
    let (player_a, profile_a) = crew.local(0).unwrap();
    assert!(profile_a.unwrap().stash.is_empty());
    assert_eq!(player_a.loadout.inventory.weapons[0], Some(a.profile.stash[0]), "A didn't get its rifle");
    crew.step(4 * 60);
    assert_eq!(load(&pb).profile.stash, vec![gun]);
    assert_eq!(load(&pa).carried.weapons[0], Some(a.profile.stash[0]));
}

/// A mission's end pays both crew members, and each file has its pay.
#[test]
fn a_missions_pay_is_saved_in_each_crew_members_file() {
    use hullbreakers::sim::mission::{Director, Mission, Phase, ScenarioKind};
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (pa, pb, a, b) = two_profiles("pay");
    let mut crew = Crew::start(vec![store_with(&pa, &a), store_with(&pb, &b)]);
    crew.server.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
    crew.step(60);
    {
        let world = crew.server.world_mut();
        let mut mission = world.query::<&mut Mission>().single_mut(world).unwrap();
        let tick = mission.started + 600;
        mission.decide(Phase::Won, tick, "COURSE CLEAR");
    }
    crew.server.world_mut().resource_mut::<Director>().start(ScenarioKind::Field);
    // (Behind the race's rock field, paced out to the clients: a few
    // seconds of the rig's clock.)
    for _ in 0..40 {
        crew.step(30);
        if (load(&pa).profile.missions, load(&pb).profile.missions) == (1, 1) {
            break;
        }
    }
    let (fa, fb) = (load(&pa), load(&pb));
    assert_eq!((fa.profile.missions, fb.profile.missions), (1, 1), "not saved at the mission's end");
    let pay = fa.profile.salvage - a.profile.salvage;
    assert!(pay > 0);
    assert_eq!(fb.profile.salvage - b.profile.salvage, pay, "paid differently");
    assert_eq!(fa.profile.stash, a.profile.stash, "A's stash changed");
    assert_eq!(fb.carried.weapons[0], b.carried.weapons[0], "B lost its gun");
}

/// A high-ranked client's own player moves by its rank (a stronger
/// jetpack) and has its slots, and predicts itself so: no more rollbacks
/// than the low-ranked one.
#[test]
fn a_ranked_players_unlocks_are_predicted() {
    use hullbreakers::sim::bots::BodyTuning;
    use hullbreakers::sim::progression::xp_for;
    use lightyear::prelude::PredictionMetrics;
    let _guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let (pa, pb, mut a, b) = two_profiles("rank");
    a.profile.xp = xp_for(9);
    let mut crew = Crew::start(vec![store_with(&pa, &a), store_with(&pb, &b)]);
    crew.step(120);
    let (player_a, _) = crew.local(0).unwrap();
    assert_eq!(player_a.loadout.inventory.weapon_slots(), 4);
    assert_eq!(player_a.loadout.inventory.pack_slots(), 10);
    let tuned = {
        let world = crew.clients[0].world_mut();
        world.query_filtered::<&BodyTuning, With<LocalPlayer>>().iter(world).next().map(|t| t.0.player_health)
    };
    assert_eq!(tuned, Some(150.0), "client A doesn't know its suit is plated");
    // Both thrust up off the deck and drift: A by its stronger jetpack.
    let rollbacks = |app: &App| app.world().resource::<PredictionMetrics>().rollbacks;
    let before = [rollbacks(&crew.clients[0]), rollbacks(&crew.clients[1])];
    for client in &mut crew.clients {
        let mut input = client.world_mut().resource_mut::<hullbreakers::net::LocalInput>();
        input.0.presses.toggle_boots.press();
        input.0.movement = Vec3::Y;
    }
    crew.step(120);
    let after = [rollbacks(&crew.clients[0]), rollbacks(&crew.clients[1])];
    let (ra, rb) = (after[0] - before[0], after[1] - before[1]);
    let off = |crew: &mut Crew, i| matches!(crew.local(i).unwrap().0.mode, hullbreakers::sim::player::Mode::Eva { .. });
    assert!(off(&mut crew, 0) && off(&mut crew, 1), "nobody took off");
    eprintln!("rollbacks in 2 s thrusting: rank 9 {ra}, rank 1 {rb}");
    assert!(ra <= rb + 2, "the ranked player mispredicts: {ra} vs {rb}");
}
