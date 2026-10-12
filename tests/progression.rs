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
use hullbreakers::sim::progression::{Profile, ProfileFile};
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
