//! A dedicated server and a client in one process, over loopback UDP with a
//! simulated bad link, stepped in lockstep on a mock clock. Measures how
//! often the client's prediction of its own player is wrong.
//!
//! Lightyear's clock is global (mock_instant), so these tests run one at a
//! time.

use std::sync::Mutex;
use std::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use lightyear::prelude::*;
use mock_instant::global::MockClock;
use hullbreakers::net::{
    LocalInput, LocalPlayer, NetMode, NetPlugin, SimulatedLag, add_headless_plugins,
};
use hullbreakers::sim::asteroids::Asteroid;
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::ship::HomeShip;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const FRAME: Duration = Duration::from_nanos(16_666_667);

struct Session {
    server: App,
    client: App,
}

fn free_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn peer(mode: NetMode, lag: Option<SimulatedLag>) -> App {
    let mut app = App::new();
    add_headless_plugins(&mut app, FRAME);
    app.add_plugins(NetPlugin { mode, lag });
    app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    app.finish();
    app.cleanup();
    app
}

impl Session {
    /// Connects a client to a fresh server and waits until it is simulating
    /// its own player.
    fn start(lag: Option<SimulatedLag>) -> Self {
        let port = free_port();
        let server = peer(NetMode::Server { port }, None);
        let client = peer(
            NetMode::Client {
                server: format!("127.0.0.1:{port}").parse().unwrap(),
            },
            lag,
        );
        let mut session = Self { server, client };
        for _ in 0..1200 {
            session.step(1);
            if session.local_player().is_some() {
                // Let prediction settle after joining.
                session.step(120);
                return session;
            }
        }
        panic!("client never started predicting its player");
    }

    fn step(&mut self, frames: usize) {
        for _ in 0..frames {
            MockClock::advance(FRAME);
            self.client.update();
            self.server.update();
        }
    }

    fn local_player(&mut self) -> Option<Player> {
        let world = self.client.world_mut();
        world
            .query_filtered::<&Player, With<LocalPlayer>>()
            .iter(world)
            .next()
            .cloned()
    }

    fn input(&mut self) -> Mut<'_, LocalInput> {
        self.client.world_mut().resource_mut::<LocalInput>()
    }

    fn rollbacks(&self) -> u32 {
        self.client.world().resource::<PredictionMetrics>().rollbacks
    }

    /// Rollbacks per second over `frames` frames.
    fn rollback_rate(&mut self, frames: usize) -> f32 {
        let before = self.rollbacks();
        self.step(frames);
        (self.rollbacks() - before) as f32 / (frames as f32 * FRAME.as_secs_f32())
    }

    /// The server's (only) crew member (the field's limpets are players
    /// too).
    fn server_player(&mut self) -> Mut<'_, Player> {
        let world = self.server.world_mut();
        let entity = world
            .query_filtered::<Entity, (With<Player>, Without<hullbreakers::sim::bots::Robot>)>()
            .single(world)
            .unwrap();
        world.get_mut::<Player>(entity).unwrap().into()
    }

    fn server_ship(&mut self) -> (Entity, Vec3, Quat) {
        let world = self.server.world_mut();
        let (e, p, r) = world
            .query_filtered::<(Entity, &Position, &Rotation), With<HomeShip>>()
            .single(world)
            .unwrap();
        (e, p.0, r.0)
    }

    /// Puts the player in EVA at `position` with `velocity`, assist off.
    fn teleport(&mut self, position: Vec3, velocity: Vec3, facing: Vec3) {
        let mut player = self.server_player();
        player.mode = Mode::Eva {
            position,
            velocity,
            orientation: Transform::IDENTITY.looking_to(facing, Vec3::Y).rotation,
            pitch: 0.0,
        };
        player.flight_assist = false;
        player.reference_body = None;
    }

    /// A rock spawned by the authority at rest relative to the ship.
    fn spawn_rock(&mut self, radius: f32, position: Vec3) -> Entity {
        let (_, _, _) = self.server_ship();
        let world = self.server.world_mut();
        world
            .spawn((
                Name::new("Test rock"),
                Asteroid { radius, seed: 7 },
                RigidBody::Dynamic,
                SleepingDisabled,
                hullbreakers::sim::bodies::PreviousVelocity::default(),
                Position(position),
                Rotation::default(),
            ))
            .id()
    }
}

fn session(lag_ms: u64) -> (std::sync::MutexGuard<'static, ()>, Session) {
    let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let lag = (lag_ms > 0).then_some(SimulatedLag {
        latency_ms: lag_ms,
        loss_percent: 0.0,
    });
    (guard, Session::start(lag))
}

#[test]
fn walking_and_turning_is_predicted() {
    let (_guard, mut s) = session(100);
    s.input().0.movement = Vec3::Z;
    let mut rate = 0.0f32;
    for turn in 0..6 {
        s.input().0.add_look(Vec2::new(0.4 * if turn % 2 == 0 { 1.0 } else { -1.0 }, 0.0));
        rate = rate.max(s.rollback_rate(60));
    }
    s.input().0.movement = Vec3::ZERO;
    let still = s.rollback_rate(120);
    eprintln!("walking: worst {rate:.2} rollbacks/s; standing {still:.2}");
    assert!(matches!(s.local_player().unwrap().mode, Mode::Booted { .. }));
    // Health comes from the authority.
    let world = s.client.world_mut();
    let vitals = world
        .query_filtered::<&hullbreakers::sim::vitals::Vitals, With<LocalPlayer>>()
        .iter(world)
        .next()
        .copied();
    assert_eq!(vitals.map(|v| v.health), Some(100.0), "client has no vitals");
    assert!(rate <= 2.0, "walking mispredicted {rate:.1}/s");
    assert!(still == 0.0, "standing mispredicted {still:.1}/s");
}

/// Drifting into a rock and bouncing off. The client predicts its own bounce
/// using the rock's mass but not the rock's recoil (that arrives from the
/// server), so a light rock is the hard case.
fn bounce_off_rock(radius: f32) -> f32 {
    let (_guard, mut s) = session(100);
    let (_, ship, rotation) = s.server_ship();
    // Off the ship's beam, clear of the hull.
    let rock_at = ship + rotation * Vec3::new(25.0, 0.0, 0.0);
    s.spawn_rock(radius, rock_at);
    s.step(60);
    let start = rock_at + rotation * Vec3::new(-(radius + 4.0), 0.0, 0.0);
    // Drift toward the rock (at rest) at 2 m/s.
    s.teleport(start, rotation * Vec3::X * 2.0, rotation * Vec3::X);
    s.step(30);
    let rate = s.rollback_rate(180);
    let player = s.local_player().unwrap();
    eprintln!(
        "radius {radius} m rock: {rate:.2} rollbacks/s, ended {:?}",
        player.mode
    );
    rate
}

#[test]
fn bouncing_off_a_heavy_rock_is_predicted() {
    let rate = bounce_off_rock(5.0);
    assert!(rate <= 2.0, "heavy rock bounce mispredicted {rate:.1}/s");
}

#[test]
fn bouncing_off_a_light_rock() {
    // Measured, not yet bounded: the rock's recoil isn't predicted.
    bounce_off_rock(0.4);
}

#[test]
fn grappling_and_reeling_a_light_rock() {
    let (_guard, mut s) = session(100);
    let (_, ship, rotation) = s.server_ship();
    let rock_at = ship + rotation * Vec3::new(25.0, 0.0, 0.0);
    s.spawn_rock(0.6, rock_at);
    s.step(60);
    // Hold still 15 m from the rock, eye level with it, facing it.
    let eye_above_center = 0.72;
    let start = rock_at + rotation * Vec3::new(-15.0, -eye_above_center, 0.0);
    s.teleport(start, Vec3::ZERO, rotation * Vec3::X);
    s.step(60);
    s.input().0.presses.main.press();
    s.step(30);
    let grappled = s.local_player().unwrap().grapple.is_some();
    s.input().0.reel_in = true;
    let rate = s.rollback_rate(180);
    eprintln!("grapple light rock: hooked {grappled}, reeling {rate:.2} rollbacks/s");
    assert!(grappled, "grapple missed the rock");
}

/// Re-simulating on every server update must not change the outcome: every
/// system a client replays has to be a pure function of its tick. Walks,
/// jumps and flies with rollback forced on every update, then checks the
/// client's settled player against the server's.
#[test]
fn forced_rollbacks_change_nothing() {
    let (_guard, mut s) = session(100);
    s.client
        .world_mut()
        .resource_mut::<PredictionManager>()
        .rollback_policy
        .state = RollbackMode::Always;
    let before = s.rollbacks();
    s.input().0.movement = Vec3::Z;
    s.input().0.add_look(Vec2::new(0.6, 0.0));
    s.step(90);
    s.input().0.presses.jump.press();
    s.step(30);
    s.input().0.movement = Vec3::new(0.3, 0.2, 1.0);
    s.step(90);
    s.input().0.movement = Vec3::ZERO;
    // Settle: flight assist brings the player to rest relative to the ship.
    s.step(240);
    let replays = s.rollbacks() - before;
    let client = s.local_player().unwrap().mode;
    let server = s.server_player().mode;
    eprintln!("forced {replays} rollbacks; client {client:?}\n server {server:?}");
    assert!(replays > 100, "rollback wasn't forced ({replays})");
    let (Mode::Eva { position: c, .. }, Mode::Eva { position: v, .. }) = (client, server) else {
        panic!("expected both in EVA: client {client:?}, server {server:?}");
    };
    assert!(c.distance(v) < 0.05, "client settled {} m from the server", c.distance(v));
}

/// Standing on our ship through a boarding action's duel: the ship flies a
/// scripted path both sides compute, so a client standing on it predicts
/// itself without corrections, and sees the ship exactly where the server
/// has it.
#[test]
fn standing_on_a_duelling_ship_is_predicted() {
    use hullbreakers::sim::flight::Flight;
    use hullbreakers::sim::mission::{Director, ScenarioKind};
    let (_guard, mut s) = session(100);
    s.server
        .world_mut()
        .resource_mut::<Director>()
        .start(ScenarioKind::Boarding);
    s.step(180);
    let player = s.local_player().unwrap();
    let (home, server_pos, _) = s.server_ship();
    assert!(s.server.world().get::<Flight>(home).is_some(), "no duel");
    let rate = s.rollback_rate(600);
    let player = s.local_player().map(|p| p.mode.body()).unwrap_or(player.mode.body());
    // The client's copy of the ship, at the same tick.
    let world = s.client.world_mut();
    let client_pos = world
        .query_filtered::<&Position, With<HomeShip>>()
        .single(world)
        .unwrap()
        .0;
    let (_, server_now, _) = s.server_ship();
    eprintln!("duel: {rate:.2} rollbacks/s standing; ship server {server_now} client {client_pos} (was {server_pos})");
    assert!(player.is_some(), "not standing on anything");
    assert!(rate <= 0.5, "standing on the duelling ship mispredicted {rate:.1}/s");
}

/// Firing, reloading and switching stance on the deck: all predicted player
/// state, so none of it should need correcting.
#[test]
fn shooting_is_predicted() {
    use hullbreakers::sim::events::PlayerEvent;
    let (_guard, mut s) = session(100);
    s.step(60);
    // Armed as at the weapons locker. (The locker arms a player in their
    // own simulation, which on a client is its prediction; so both copies.)
    for world in [s.server.world_mut(), s.client.world_mut()] {
        for mut p in world.query::<&mut Player>().iter_mut(world) {
            p.loadout.restock();
        }
    }
    s.step(60);
    s.input().0.stance = 0;
    s.input().0.presses.stance.press();
    s.input().0.main.held = true;
    let firing = s.rollback_rate(120);
    s.input().0.main.held = false;
    s.input().0.stance = 2;
    s.input().0.presses.stance.press();
    s.input().0.presses.reload.press();
    let switching = s.rollback_rate(120);
    s.input().0.main.held = true;
    let rifle = s.rollback_rate(120);
    let fired = s
        .local_player()
        .unwrap()
        .events
        .iter()
        .filter(|(_, e)| matches!(e, PlayerEvent::Fired { .. }))
        .count();
    eprintln!("shooting: {firing:.2} / {switching:.2} / {rifle:.2} rollbacks/s; {fired} shots in the log");
    assert!(fired > 0, "never fired");
    assert!(firing.max(switching).max(rifle) <= 0.5, "shooting mispredicted");
}

/// Wave 4's guns are predicted player state like the others: a laser's
/// heat, a sniper's zoom (the off hand's button, that hand empty) and a
/// shotgun's pellets need no correcting.
#[test]
fn new_guns_are_predicted() {
    use hullbreakers::sim::events::PlayerEvent;
    use hullbreakers::sim::items::{Family, Weapon};
    use hullbreakers::sim::weapons::{GunKind, Slot};
    let (_guard, mut s) = session(100);
    s.step(60);
    // The same kit in both copies: a laser SMG in slot 1, a sniper in
    // slot 2 (stance 3 holds it with the off hand empty), a shotgun in 3.
    for world in [s.server.world_mut(), s.client.world_mut()] {
        for mut p in world.query::<&mut Player>().iter_mut(world) {
            p.loadout.restock();
            p.loadout.inventory.weapons[0] = Some(Weapon::issue(GunKind::Smg).with_family(Family::Laser));
            p.loadout.inventory.weapons[1] = Some(Weapon::issue(GunKind::Sniper));
            p.loadout.inventory.weapons[2] = Some(Weapon::issue(GunKind::Shotgun));
            p.loadout.stances[3].main = Slot::Gun(2);
        }
    }
    s.step(60);
    s.input().0.stance = 0;
    s.input().0.presses.stance.press();
    s.input().0.main.held = true;
    // Long enough to overheat and cool.
    let laser = s.rollback_rate(400);
    s.input().0.main.held = false;
    s.input().0.stance = 2;
    s.input().0.presses.stance.press();
    s.step(60);
    s.input().0.off.held = true;
    s.step(10);
    let zoom = s.local_player().unwrap().loadout.zoom;
    s.input().0.main.held = true;
    let sniper = s.rollback_rate(120);
    s.input().0.main.held = false;
    s.input().0.off.held = false;
    s.input().0.stance = 3;
    s.input().0.presses.stance.press();
    s.step(60);
    s.input().0.main.held = true;
    let shotgun = s.rollback_rate(120);
    let fired = s
        .local_player()
        .unwrap()
        .events
        .iter()
        .filter(|(_, e)| matches!(e, PlayerEvent::Fired { .. }))
        .count();
    eprintln!("new guns: laser {laser:.2} / sniper {sniper:.2} / shotgun {shotgun:.2} rollbacks/s; zoom {zoom}; {fired} shots in the log");
    assert_eq!(zoom, 5.0, "the sniper didn't zoom");
    assert!(fired > 0, "never fired");
    assert!(laser.max(sniper).max(shotgun) <= 0.5, "mispredicted");
}

/// The carrier's robots reach a client as interpolated players with their
/// kind and health, and having them about doesn't upset the client's
/// prediction of itself.
#[test]
fn robots_reach_clients() {
    use hullbreakers::sim::bots::Robot;
    use hullbreakers::sim::mission::{Director, ScenarioKind};
    use hullbreakers::sim::vitals::Vitals;
    let (_guard, mut s) = session(100);
    s.server
        .world_mut()
        .resource_mut::<Director>()
        .start(ScenarioKind::Boarding);
    s.step(240);
    let count = |app: &mut App| {
        let world = app.world_mut();
        world.query_filtered::<(), (With<Robot>, With<Player>, With<Vitals>)>().iter(world).count()
    };
    let (server, client) = (count(&mut s.server), count(&mut s.client));
    let rate = s.rollback_rate(300);
    eprintln!("robots: server {server}, client {client}; {rate:.2} rollbacks/s");
    assert!(server > 0, "no robots on the server");
    assert_eq!(client, server, "client sees {client} of {server} robots");
    assert!(rate <= 0.5, "robots upset prediction: {rate:.1}/s");
}

/// The field's fauna reach clients: grazers as bodies posed from their
/// motion (which changes only at their pulses), where the server has them;
/// limpets as players. Neither upsets the client's prediction.
#[test]
fn fauna_reach_clients() {
    use hullbreakers::sim::SimTick;
    use hullbreakers::sim::bots::Robot;
    use hullbreakers::sim::fauna::Grazer;
    use hullbreakers::sim::motion::Motion;
    let (_guard, mut s) = session(100);
    s.step(240);
    let count = |app: &mut App| {
        let world = app.world_mut();
        let grazers = world.query_filtered::<(), With<Grazer>>().iter(world).count();
        let limpets = world.query::<&Robot>().iter(world).filter(|k| **k == Robot::Limpet).count();
        (grazers, limpets)
    };
    // Fauna stream in with the rocks, so one may be on its way to the
    // client just now: they agree within a moment.
    let mut counts = (count(&mut s.server), count(&mut s.client));
    for _ in 0..6 {
        if counts.0 == counts.1 {
            break;
        }
        s.step(10);
        counts = (count(&mut s.server), count(&mut s.client));
    }
    let (server, client) = counts;
    eprintln!("fauna: server {server:?}, client {client:?}");
    assert!(server.0 > 0 && server.1 > 0, "no fauna on the server: {server:?}");
    assert_eq!(client, server, "the client sees {client:?} of {server:?}");
    // At the client's tick, each grazer is where the server's motion for it
    // puts it.
    let tick = s.client.world().resource::<SimTick>().0;
    let world = s.server.world_mut();
    let truth: Vec<Vec3> = world
        .query_filtered::<&Motion, With<Grazer>>()
        .iter(world)
        .map(|m| m.frame_at(tick, Vec3::ZERO).center_of_mass)
        .collect();
    let world = s.client.world_mut();
    let seen: Vec<Vec3> = world
        .query_filtered::<(&Position, &Rotation, &hullbreakers::sim::bodies::BodyInertia), With<Grazer>>()
        .iter(world)
        .map(|(p, r, i)| p.0 + r.0 * i.center_of_mass)
        .collect();
    for at in &truth {
        let off = seen.iter().map(|q| q.distance(*at)).fold(f32::MAX, f32::min);
        assert!(off < 0.5, "a grazer is drawn {off} m from where the server has it");
    }
    let rate = s.rollback_rate(300);
    assert!(rate <= 0.5, "fauna upset prediction: {rate:.1}/s");
}

/// A site's devices reach clients: the derelict, where the server has it,
/// its device state as the server has it, and its doors open in the
/// client's collider once the server opens them; the mission's objective
/// follows.
#[test]
fn devices_reach_clients() {
    use hullbreakers::sim::devices::{Devices, DoorsBuilt};
    use hullbreakers::sim::mission::{Director, Mission, ScenarioKind};
    use hullbreakers::sim::ship::ShipHull;
    let (_guard, mut s) = session(100);
    s.server
        .world_mut()
        .resource_mut::<Director>()
        .start(ScenarioKind::DeadShip);
    s.step(180);
    let derelict = |app: &mut App| {
        let world = app.world_mut();
        world
            .query::<(&ShipHull, &Position, &Devices, &DoorsBuilt)>()
            .iter(world)
            .find(|(h, ..)| h.0 == "derelict")
            .map(|(_, p, d, b)| (p.0, d.clone(), b.clone()))
    };
    let (server_at, _, _) = derelict(&mut s.server).expect("no derelict on the server");
    let (client_at, _, doors) = derelict(&mut s.client).expect("no derelict on the client");
    assert!(server_at.distance(client_at) < 0.01, "server {server_at}, client {client_at}");
    assert!(doors.0.iter().filter(|&&shut| shut).count() == 3, "{doors:?}");
    // The server's crew pulls the reactor and turns junction A to the
    // port door.
    {
        let world = s.server.world_mut();
        let (hull, mut devices) = world.query::<(&ShipHull, &mut Devices)>().iter_mut(world).find(|(h, _)| h.0 == "derelict").unwrap();
        let design = hull.design();
        devices.0[design.device("reactor")].set = 1;
        devices.0[design.device("j1")].set = 1;
    }
    s.step(60);
    let (_, server_devices, _) = derelict(&mut s.server).unwrap();
    let (_, client_devices, doors) = derelict(&mut s.client).unwrap();
    assert_eq!(client_devices, server_devices);
    let design = hullbreakers::sim::design::design("derelict");
    assert!(!doors.0[design.device("port_door")], "the port door is still shut on the client");
    assert!(doors.0[design.device("bulkhead")]);
    let world = s.client.world_mut();
    let objective = world.query::<&Mission>().single(world).unwrap().objective.clone();
    assert!(objective.starts_with("Route power"), "{objective}");
}

/// Putting a gun down and taking it back, on a client: the inventory is
/// predicted, the pickup is the authority's, and the two agree.
#[test]
fn dropping_and_taking_back_is_predicted() {
    use hullbreakers::sim::pickups::Pickup;
    use hullbreakers::sim::tuning::Tuning;
    let (_guard, mut s) = session(100);
    s.step(60);
    for world in [s.server.world_mut(), s.client.world_mut()] {
        for mut p in world.query::<&mut Player>().iter_mut(world) {
            p.loadout.restock();
        }
    }
    s.input().0.stance = 0;
    s.input().0.presses.stance.press();
    s.step(60);
    let gun = s.local_player().unwrap().loadout.inventory.weapons[0].expect("armed");
    s.input().0.presses.drop.press();
    let dropping = s.rollback_rate(120);
    let pickups = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Pickup>().iter(world).cloned().collect::<Vec<_>>()
    };
    let (on_server, on_client) = (pickups(&mut s.server), pickups(&mut s.client));
    assert_eq!(s.local_player().unwrap().loadout.inventory.weapons[0], None, "still holding it");
    assert_eq!(s.server_player().loadout.inventory.weapons[0], None, "the server says still holding it");
    assert_eq!(on_server.len(), 1, "nothing left on the server");
    assert_eq!(on_client.len(), 1, "the client never saw it put down");
    // Look down at it and take it.
    let tuning = s.client.world().resource::<Tuning>().clone();
    let Mode::Booted { feet, up, pitch, .. } = s.local_player().unwrap().mode else {
        panic!("not on the deck");
    };
    let look = on_client[0].at - (feet + up * tuning.eye_height);
    let wanted = look.y.atan2(Vec2::new(look.x, look.z).length());
    s.input().0.add_look(Vec2::new(0.0, pitch - wanted));
    s.step(10);
    s.input().0.presses.interact.press();
    let taking = s.rollback_rate(120);
    eprintln!("dropping {dropping:.2}, taking {taking:.2} rollbacks/s");
    assert_eq!(s.local_player().unwrap().loadout.inventory.weapons[0], Some(gun), "didn't take it back");
    assert_eq!(s.server_player().loadout.inventory.weapons[0], Some(gun));
    assert!(pickups(&mut s.server).is_empty() && pickups(&mut s.client).is_empty(), "it's still lying there");
    assert!(dropping.max(taking) <= 0.5, "items mispredicted");
}

/// Markers name their bodies by `BodyId` and their players by deck slot, so
/// a client finds its own copy of the course under the grapple run's gate
/// marker, and the marker is its own.
#[test]
fn markers_find_their_bodies_on_clients() {
    use hullbreakers::sim::mission::{BodyId, Director, Mission, ScenarioKind};
    use hullbreakers::sim::player::DeckSlot;
    use hullbreakers::sim::ship::ShipHull;
    let (_guard, mut s) = session(100);
    s.server
        .world_mut()
        .resource_mut::<Director>()
        .start(ScenarioKind::GrappleRun);
    s.step(180);
    let world = s.client.world_mut();
    let mission = world.query::<&Mission>().single(world).unwrap().clone();
    let marker = mission.markers.first().expect("no markers on the client").clone();
    assert_eq!(marker.label, "Gate 1");
    let body = marker.body.expect("the gate marker is on the course");
    let found: Vec<&ShipHull> = world.query::<(&BodyId, &ShipHull)>().iter(world).filter(|(id, _)| **id == body).map(|(_, h)| h).collect();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "course");
    let slot = world.query_filtered::<&DeckSlot, With<LocalPlayer>>().single(world).unwrap().0;
    assert_eq!(marker.player, Some(slot));
}

/// A client's own press of a device shows at once, a round trip before the
/// authority's answer, and agrees with it once it's back.
#[test]
fn a_press_shows_before_the_round_trip() {
    use hullbreakers::sim::devices::{Devices, ShownDevices};
    let (_guard, mut s) = session(100);
    s.step(60);
    let main = hullbreakers::sim::design::design("home").device("main");
    // In the hold, facing the main bus junction on the port wall, on both
    // peers (the client's player is predicted).
    let feet = Vec3::new(-2.62, 0.01, 3.0);
    for world in [s.server.world_mut(), s.client.world_mut()] {
        let ship = world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap();
        for mut p in world.query::<&mut Player>().iter_mut(world) {
            p.mode = Mode::Booted {
                body: ship,
                feet,
                up: Vec3::Y,
                velocity: Vec3::ZERO,
                orientation: Transform::IDENTITY.looking_to(Vec3::NEG_X, Vec3::Y).rotation,
                pitch: (1.4f32 - (0.01 + 1.62)).atan2(1.0),
            };
        }
    }
    s.step(30);
    let way = |app: &mut App| {
        let world = app.world_mut();
        let (devices, shown) = world
            .query_filtered::<(&Devices, Option<&ShownDevices>), With<HomeShip>>()
            .single(world)
            .unwrap();
        (devices.0[main].set, shown.map(|s| s.0[main].set))
    };
    assert_eq!(way(&mut s.client), (0, Some(0)));
    s.input().0.presses.interact.press();
    let (mut shown_at, mut answered_at) = (None, None);
    for frame in 0..60 {
        s.step(1);
        let (authority, shown) = way(&mut s.client);
        if shown == Some(1) && shown_at.is_none() {
            shown_at = Some(frame);
        }
        if authority == 1 && answered_at.is_none() {
            answered_at = Some(frame);
        }
    }
    eprintln!("press: shown at frame {shown_at:?}, the authority's answer at frame {answered_at:?}");
    let (shown_at, answered_at) = (shown_at.expect("never shown"), answered_at.expect("never answered"));
    assert!(shown_at + 6 <= answered_at, "shown at {shown_at}, answered at {answered_at}: not ahead of the round trip");
    assert_eq!(way(&mut s.client), (1, Some(1)), "shown and answered disagree afterwards");
    assert_eq!(way(&mut s.server).0, 1);
}

/// A press the authority doesn't make (here, the client's player is
/// somewhere else on the authority, so the press misses) shows briefly,
/// then the shown state goes back to the authority's.
#[test]
fn a_mispredicted_press_goes_back() {
    use hullbreakers::sim::devices::{Devices, ShownDevices};
    let (_guard, mut s) = session(100);
    s.step(60);
    let main = hullbreakers::sim::design::design("home").device("main");
    let world = s.client.world_mut();
    let ship = world.query_filtered::<Entity, With<HomeShip>>().single(world).unwrap();
    // Only the client thinks its player faces the junction.
    let mut predicted = world.query_filtered::<&mut Player, With<LocalPlayer>>().single_mut(world).unwrap();
    predicted.mode = Mode::Booted {
        body: ship,
        feet: Vec3::new(-2.62, 0.01, 3.0),
        up: Vec3::Y,
        velocity: Vec3::ZERO,
        orientation: Transform::IDENTITY.looking_to(Vec3::NEG_X, Vec3::Y).rotation,
        pitch: (1.4f32 - (0.01 + 1.62)).atan2(1.0),
    };
    s.input().0.presses.interact.press();
    let shown = |app: &mut App| {
        let world = app.world_mut();
        let (devices, shown) = world
            .query_filtered::<(&Devices, Option<&ShownDevices>), With<HomeShip>>()
            .single(world)
            .unwrap();
        (devices.0[main].set, shown.map(|s| s.0[main].set))
    };
    s.step(3);
    let early = shown(&mut s.client);
    s.step(150);
    let late = shown(&mut s.client);
    eprintln!("mispredicted press: {early:?} at first, {late:?} later");
    assert_eq!(late, (0, Some(0)), "the shown state didn't go back to the authority's");
}

/// A death is the authority's alone. One where the respawn puts you back
/// where you stood, with nothing on you, changes nothing else a client
/// predicts; it must still reach the client's log (its HUD and camera show
/// it from there).
#[test]
fn a_death_where_you_stand_reaches_the_client() {
    use hullbreakers::sim::events::PlayerEvent;
    use hullbreakers::sim::vitals::Vitals;
    let (_guard, mut s) = session(100);
    s.step(60);
    let world = s.server.world_mut();
    let mut vitals = world.query_filtered::<&mut Vitals, (With<Player>, Without<hullbreakers::sim::bots::Robot>)>().single_mut(world).unwrap();
    vitals.health = 0.0;
    vitals.since_hurt = 0.0;
    let mut seen = false;
    for _ in 0..60 {
        s.step(1);
        seen |= s.local_player().unwrap().events.iter().any(|(_, e)| e == PlayerEvent::Died);
    }
    assert!(s.server_player().events.iter().any(|(_, e)| e == PlayerEvent::Died), "the server never killed them");
    assert!(seen, "the client never heard it died");
}

// ---------------------------------------------------------------------------
// Measurements for docs/proposal-remote-forces.md: what a push from outside
// the client's own input costs it today. Run with
// `cargo test --test netplay measure_ -- --ignored --nocapture`.

/// The camera's view of the client's player through corrections, under two
/// blend rules: the old one (exponential, a fixed `smooth_time`) and the
/// new one (`camera::blend_out`: size-scaled, critically damped). Tracks the worst per-frame jerk of the drawn eye:
/// how far one frame's step differs from the last frame's.
#[derive(Default)]
struct Views {
    offsets: [Vec3; 2],
    rate: Vec3,
    /// Per frame: the predicted eye, and where each rule draws it.
    path: Vec<[Vec3; 3]>,
    last: [Option<Vec3>; 2],
    step: [Option<Vec3>; 2],
    jerk: [f32; 2],
}

impl Views {
    /// Writes the path to `target/remote-forces/<name>.csv` (frame, then
    /// x, y, z of the predicted eye, the old rule's drawn eye, the new
    /// rule's), for plotting.
    fn save(&self, name: &str) {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/remote-forces");
        let _ = std::fs::create_dir_all(&dir);
        let mut csv = String::from("frame,eye_x,eye_y,eye_z,old_x,old_y,old_z,new_x,new_y,new_z\n");
        for (i, row) in self.path.iter().enumerate() {
            let [a, b, c] = row;
            csv += &format!("{i},{},{},{},{},{},{},{},{},{}\n", a.x, a.y, a.z, b.x, b.y, b.z, c.x, c.y, c.z);
        }
        let _ = std::fs::write(dir.join(format!("{name}.csv")), csv);
    }

    fn frame(&mut self, eye: Vec3, correction: Vec3, dt: f32, smooth_time: f32) {
        for k in 0..2 {
            let offset = self.offsets[k] + correction;
            self.offsets[k] = if k == 0 {
                offset * (-dt / smooth_time).exp()
            } else {
                let (o, r) = hullbreakers::client::camera::blend_out(offset, self.rate, dt, smooth_time);
                self.rate = r;
                o
            };
            let drawn = eye + self.offsets[k];
            if let Some(last) = self.last[k] {
                let step = drawn - last;
                if let Some(before) = self.step[k] {
                    self.jerk[k] = self.jerk[k].max(step.distance(before));
                }
                self.step[k] = Some(step);
            }
            self.last[k] = Some(drawn);
        }
        self.path.push([eye, eye + self.offsets[0], eye + self.offsets[1]]);
    }
}

/// Over `frames` frames: the rollbacks, and how far the client's predicted
/// player jumped from its own straight-line flight on rollback frames (the
/// correction the camera has to blend over), worst and summed. `views`
/// follows what the camera draws.
fn watch_corrections_drawn(s: &mut Session, frames: usize, views: &mut Views) -> (u32, f32, f32) {
    use hullbreakers::client::ViewCorrection;
    let dt = FRAME.as_secs_f32();
    let smooth_time = s.client.world().resource::<hullbreakers::sim::tuning::Tuning>().smooth_time;
    if !s.client.world().contains_resource::<ViewCorrection>() {
        s.client.world_mut().insert_resource(ViewCorrection {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
        });
    }
    let (mut worst, mut total) = (0.0f32, 0.0f32);
    let before = s.rollbacks();
    for _ in 0..frames {
        let (p0, v0) = match s.local_player().map(|p| p.mode) {
            Some(Mode::Eva { position, velocity, .. }) => (position, velocity),
            _ => (Vec3::ZERO, Vec3::ZERO),
        };
        let r0 = s.rollbacks();
        s.step(1);
        let correction = std::mem::replace(&mut s.client.world_mut().resource_mut::<ViewCorrection>().position, Vec3::ZERO);
        let Some(Mode::Eva { position, .. }) = s.local_player().map(|p| p.mode) else { continue };
        views.frame(position, correction, dt, smooth_time);
        if s.rollbacks() > r0 {
            let jump = position.distance(p0 + v0 * dt);
            worst = worst.max(jump);
            total += jump;
        }
    }
    (s.rollbacks() - before, worst, total)
}

fn watch_corrections(s: &mut Session, frames: usize) -> (u32, f32, f32) {
    watch_corrections_drawn(s, frames, &mut Views::default())
}

/// Floating still, assist off, 40 m off our ship's beam.
fn float_clear(s: &mut Session) {
    let (_, ship, rotation) = s.server_ship();
    s.teleport(ship + rotation * Vec3::new(40.0, 0.0, 0.0), Vec3::ZERO, Vec3::NEG_Z);
    s.step(120);
}

/// An impulse (N s) the authority gives the player, as a hit or a blast
/// does: only the authority knows of it.
fn measure_kick(impulse: f32, what: &str) {
    let (_guard, mut s) = session(100);
    float_clear(&mut s);
    let quiet = watch_corrections(&mut s, 60);
    let mass = s.server.world().resource::<hullbreakers::sim::tuning::Tuning>().player_mass;
    let tuning = s.server.world().resource::<hullbreakers::sim::tuning::Tuning>().clone();
    s.server_player().shove(Vec3::X * impulse / mass, None, &tuning);
    let mut views = Views::default();
    let (rollbacks, worst, total) = watch_corrections_drawn(&mut s, 120, &mut views);
    eprintln!(
        "{what}: {impulse} N s ({:.1} m/s): {rollbacks} rollbacks in 2 s (quiet before: {}), worst correction {worst:.2} m, summed {total:.2} m; \
         drawn eye's worst jerk {:.3} m/frame with the old blend, {:.3} with the new one",
        impulse / mass,
        quiet.0,
        views.jerk[0],
        views.jerk[1]
    );
    views.save(&what.replace([' ', ','], "_"));
}

#[test]
#[ignore]
fn measure_shotgun_shove() {
    // Eight 30 g pellets at 420 m/s, all landing.
    measure_kick(8.0 * 0.03 * 420.0, "shotgun shell, every pellet");
}

#[test]
#[ignore]
fn measure_blast() {
    // A Grappler going up, 3 m away (half its 900 N s at that range).
    measure_kick(450.0, "volatile Grappler blast at 3 m");
}

/// A robot (or a crewmate: robots are players) 12 m away hooks the client's
/// player and winches them in at 15 m/s for half a second (its rope
/// shortened tick by tick, as a winch does). The client sees the robot
/// interpolated, its rope too, but doesn't solve that rope for itself.
fn measure_rope(robot_mass_scale: f32, what: &str) {
    use hullbreakers::sim::bots::{Affixes, Brain, Robot, new_robot};
    use hullbreakers::sim::player::Grapple;
    let _ = robot_mass_scale;
    let (_guard, mut s) = session(100);
    float_clear(&mut s);
    let (ship, ship_at, rotation) = s.server_ship();
    let me = s.server.world_mut().query_filtered::<Entity, (With<Player>, Without<Robot>)>().single(s.server.world()).unwrap();
    let at = ship_at + rotation * Vec3::new(52.0, 0.0, 0.0);
    let tuning = s.server.world().resource::<hullbreakers::sim::tuning::Tuning>().clone();
    let mode = Mode::Eva {
        position: at,
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    let robot = s.server.world_mut().spawn(new_robot(Robot::Grappler, Affixes::NONE, mode, ship, at, &tuning, 0)).id();
    s.server.world_mut().get_mut::<Brain>(robot).unwrap().dormant = true;
    s.step(60);
    let quiet = watch_corrections(&mut s, 60);
    s.server.world_mut().get_mut::<Player>(robot).unwrap().grapple = Some(Grapple {
        target: me,
        anchor: Vec3::ZERO,
        length: 12.0,
        tension: 0.0,
    });
    let before = s.rollbacks();
    let (mut worst, mut total) = (0.0f32, 0.0f32);
    let mut views = Views::default();
    for _ in 0..30 {
        if let Some(g) = &mut s.server.world_mut().get_mut::<Player>(robot).unwrap().grapple {
            g.length = (g.length - 0.25).max(4.0);
        }
        let (_, w, t) = watch_corrections_drawn(&mut s, 1, &mut views);
        worst = worst.max(w);
        total += t;
    }
    let (_, w, t) = watch_corrections_drawn(&mut s, 150, &mut views);
    worst = worst.max(w);
    total += t;
    eprintln!(
        "{what}: {} rollbacks in 3 s (quiet before: {}), worst correction {worst:.2} m, summed {total:.2} m; \
         drawn eye's worst jerk {:.3} m/frame with the old blend, {:.3} with the new one",
        s.rollbacks() - before,
        quiet.0,
        views.jerk[0],
        views.jerk[1]
    );
    views.save("rope_yank");
}

#[test]
#[ignore]
fn measure_rope_yank() {
    measure_rope(1.0, "rope yank (winched 12 m to 4 m at 15 m/s)");
}
