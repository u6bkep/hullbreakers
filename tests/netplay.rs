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

/// Bytes the client has received from the server, as its link has them
/// (counted before the transport reads them).
#[derive(Resource, Default)]
struct Received(usize);

fn count_received(mut links: Query<&mut Link>, mut received: ResMut<Received>) {
    for mut link in &mut links {
        let mut packets = Vec::new();
        while let Some(p) = link.recv.pop() {
            received.0 += p.len();
            packets.push(p);
        }
        for p in packets {
            link.recv.push_raw(p);
        }
    }
}

fn peer(mode: NetMode, lag: Option<SimulatedLag>) -> App {
    let mut app = App::new();
    add_headless_plugins(&mut app, FRAME);
    app.init_resource::<Received>();
    app.add_systems(
        PreUpdate,
        count_received.after(LinkSystems::Receive).before(lightyear::transport::plugin::TransportSystems::Receive),
    );
    app.add_plugins(NetPlugin { mode, lag });
    app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    // The rig's clock is mocked; pacing goes by the wall clock.
    app.insert_resource(hullbreakers::net::Unpaced);
    app.finish();
    app.cleanup();
    app
}

impl Session {
    /// Connects a client to a fresh server and waits until it is simulating
    /// its own player.
    fn start(lag: Option<SimulatedLag>) -> Self {
        Self::start_with(lag, |_| {})
    }

    /// As [`Self::start`], with `before` run on the server (and a second
    /// of it stepped) before the client connects: a late joiner.
    fn start_with(lag: Option<SimulatedLag>, before: impl FnOnce(&mut App)) -> Self {
        let port = free_port();
        let mut server = peer(NetMode::Server { port }, None);
        before(&mut server);
        for _ in 0..60 {
            MockClock::advance(FRAME);
            server.update();
        }
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

    /// Bytes per second the client receives over `frames` frames.
    fn received_rate(&mut self, frames: usize) -> f32 {
        let before = self.client.world().resource::<Received>().0;
        self.step(frames);
        (self.client.world().resource::<Received>().0 - before) as f32 / (frames as f32 * FRAME.as_secs_f32())
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
                Asteroid { radius, seed: 7, fixed: false },
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
    // Mostly adrift; now and then a drifting rock is in the way and both
    // land on it. Either way, the same.
    let (c, v) = match (client, server) {
        (Mode::Eva { position: c, .. }, Mode::Eva { position: v, .. }) => (c, v),
        (Mode::Booted { feet: c, .. }, Mode::Booted { feet: v, .. }) => (c, v),
        _ => panic!("client and server in different modes: client {client:?}, server {server:?}"),
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

/// A held laser costs the link little more than standing still: the beam
/// is state that starts and stops, not a shot in the event log every tick.
#[test]
fn a_held_laser_is_cheap_on_the_wire() {
    use hullbreakers::sim::items::{Family, Weapon};
    use hullbreakers::sim::weapons::Slot;
    let (_guard, mut s) = session(0);
    s.step(60);
    for world in [s.server.world_mut(), s.client.world_mut()] {
        for mut p in world.query::<&mut Player>().iter_mut(world) {
            p.loadout.restock();
            p.loadout.inventory.weapons[0] = Some(Weapon::issue(hullbreakers::sim::weapons::GunKind::Smg).with_family(Family::Laser));
            p.loadout.stances[0].off = Slot::Empty;
        }
    }
    s.input().0.stance = 0;
    s.input().0.presses.stance.press();
    s.step(120);
    let idle = s.received_rate(180);
    s.input().0.main.held = true;
    // Three seconds of beam: under the overheat.
    let beam = s.received_rate(180);
    s.input().0.main.held = false;
    eprintln!("laser on the wire: standing {idle:.0} B/s, beam held {beam:.0} B/s");
    // Measured 2026-10-12 (before the fauna came): 6.3 kB/s standing; a beam
    // as a `Fired` entry a tick added 65.5 kB/s, as held state 0.8. With the
    // fauna in the world: 45.2 kB/s standing, the beam adding 5.4; with idle
    // robots and creatures no longer re-sent every tick (docs/perf.md):
    // 14.4 standing, the beam adding nothing measurable.
    assert!(beam - idle < 10_000.0, "a beam costs {beam:.0} B/s against {idle:.0} standing");
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

/// A robot reaches a client as what's drawn of it (`net::robots`), not its
/// whole player: the client's `Player` for it is built from that, standing
/// where the server has it, with the events it had; and the boarding action
/// (the carrier's crew all moving and aiming) is cheap on the wire.
#[test]
fn robots_reach_clients_as_what_is_drawn_of_them() {
    use hullbreakers::sim::bots::Robot;
    use hullbreakers::sim::events::PlayerEvent;
    use hullbreakers::sim::mission::{Director, ScenarioKind};
    let (_guard, mut s) = session(0);
    s.step(240);
    // Where each still limpet's feet are on its rock (they lurk; one
    // lunging is drawn where it was an interpolation delay ago).
    let feet = |app: &mut App| -> Vec<Vec3> {
        let world = app.world_mut();
        world
            .query::<(&Robot, &Player)>()
            .iter(world)
            .filter_map(|(k, p)| match (*k, p.mode) {
                (Robot::Limpet, Mode::Booted { feet, velocity, .. }) if velocity.length() < 0.01 => Some(feet),
                _ => None,
            })
            .collect()
    };
    let (truth, seen) = (feet(&mut s.server), feet(&mut s.client));
    for at in &truth {
        let off = seen.iter().map(|q| q.distance(*at)).fold(f32::MAX, f32::min);
        assert!(off < 0.005, "a still limpet is drawn {off} m from where the server has it");
    }
    // Something a robot does reaches the client in its log.
    let world = s.server.world_mut();
    let mut limpets = world.query_filtered::<&mut Player, With<Robot>>();
    limpets.iter_mut(world).next().unwrap().events.push(PlayerEvent::Respawned);
    s.step(30);
    let world = s.client.world_mut();
    let heard = world
        .query_filtered::<&Player, With<Robot>>()
        .iter(world)
        .any(|p| p.events.iter().any(|(_, e)| matches!(e, PlayerEvent::Respawned)));
    assert!(heard, "a robot's event never reached the client");
    // The boarding action.
    s.server.world_mut().resource_mut::<Director>().start(ScenarioKind::Boarding);
    s.step(240);
    // Its robots (some with affixes that change their size) are hit where
    // they are on a client: their capsules there are the server's.
    let sizes = |app: &mut App| -> Vec<(i32, i32)> {
        let world = app.world_mut();
        let mut v: Vec<(i32, i32)> = world
            .query_filtered::<&Collider, (With<Robot>, With<Player>)>()
            .iter(world)
            .filter_map(|c| c.shape().as_capsule().map(|k| ((k.radius * 1000.0) as i32, (k.half_height() * 1000.0) as i32)))
            .collect();
        v.sort();
        v
    };
    let (truth, seen) = (sizes(&mut s.server), sizes(&mut s.client));
    assert!(!truth.is_empty());
    assert_eq!(seen, truth, "robots' capsules on the client differ from the server's");
    let rate = s.received_rate(180);
    eprintln!("robots on the wire: boarding, standing aboard our ship, {rate:.0} B/s");
    // Measured 2026-10-12: 37.8 kB/s with robots sent as whole players,
    // 12.9 as their views.
    assert!(rate < 20_000.0, "boarding costs {rate:.0} B/s");
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
/// a client finds its own copy of the course under the grapple run's ring
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
    // (The client grows the race's field first.)
    let mut mission = None;
    for _ in 0..40 {
        s.step(30);
        let world = s.client.world_mut();
        mission = world.query::<&Mission>().single(world).ok().filter(|m| !m.markers.is_empty()).cloned();
        if mission.is_some() {
            break;
        }
    }
    let world = s.client.world_mut();
    let marker = mission.expect("no markers on the client").markers[0].clone();
    assert_eq!(marker.label, "Start");
    let body = marker.body.expect("the ring marker is on the course");
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

/// What the client's presentation was handed (see
/// [`hullbreakers::client::events`]), in order.
#[derive(Resource, Default)]
struct Shown(Vec<hullbreakers::sim::events::PlayerEvent>);

impl Session {
    /// Runs the client's event presentation, collecting what it presents
    /// into [`Shown`].
    fn present(&mut self) {
        use hullbreakers::client::events::{EventCursor, Presented, present_events};
        let collect = |mut presented: MessageReader<Presented>, mut shown: ResMut<Shown>| shown.0.extend(presented.read().map(|p| p.0));
        self.client
            .init_resource::<EventCursor>()
            .init_resource::<Shown>()
            .add_message::<Presented>()
            .add_systems(Update, (present_events, collect).chain());
    }

    fn shown(&self, event: hullbreakers::sim::events::PlayerEvent) -> usize {
        self.client.world().resource::<Shown>().0.iter().filter(|&&e| e == event).count()
    }

    fn server_crew(&mut self) -> Entity {
        let world = self.server.world_mut();
        world
            .query_filtered::<Entity, (With<Player>, Without<hullbreakers::sim::bots::Robot>)>()
            .single(world)
            .unwrap()
    }
}

/// A death, and the respawn after it, are the authority's alone. Where the
/// respawn puts you back where you stood, with nothing on you, nothing a
/// client predicts differs, so no correction carries them: they reach the
/// client in its player's authority log, and are presented once each
/// (however long the round trip: at 250 ms they arrive further back than a
/// corrected event would be shown).
fn a_death_where_you_stand_is_presented(lag_ms: u64) {
    use hullbreakers::sim::events::{AuthorityLog, PlayerEvent};
    use hullbreakers::sim::vitals::Vitals;
    let (_guard, mut s) = session(lag_ms);
    s.present();
    s.step(60);
    let crew = s.server_crew();
    let before = s.server_player().mode;
    let mut vitals = s.server.world_mut().get_mut::<Vitals>(crew).unwrap();
    vitals.health = 0.0;
    vitals.since_hurt = 0.0;
    s.step(120);
    let told = s.server.world().get::<AuthorityLog>(crew).unwrap();
    let said: Vec<_> = told.iter().map(|(_, e)| e).collect();
    assert_eq!(said, vec![PlayerEvent::Died, PlayerEvent::Respawned], "the server's log");
    // Nothing a rollback would notice (it stood on its spawn).
    let after = s.server_player().mode;
    assert!(after.body() == before.body() && after.world_kinematics(&Default::default(), None).0.distance(before.world_kinematics(&Default::default(), None).0) < 0.01, "the respawn moved it: {before:?} to {after:?}");
    assert_eq!(s.shown(PlayerEvent::Died), 1, "the client was told of its death {} times", s.shown(PlayerEvent::Died));
    assert_eq!(s.shown(PlayerEvent::Respawned), 1, "the client was told of its respawn {} times", s.shown(PlayerEvent::Respawned));
}

#[test]
fn a_death_where_you_stand_is_presented_at_100_ms() {
    a_death_where_you_stand_is_presented(100);
}

#[test]
fn a_death_where_you_stand_is_presented_at_250_ms() {
    a_death_where_you_stand_is_presented(250);
}

/// A blast that knocks you off the deck is the authority's to decide: at
/// 250 ms the client learns of it a quarter second after the tick it
/// happened on, past what presentation shows of a corrected event, and must
/// still present it, once. And it must still correct the client's
/// prediction (which had it standing): the correction is older than
/// lightyear's default rollback limit at that lag (see
/// `MAX_ROLLBACK_TICKS`).
#[test]
fn a_blast_at_250_ms_is_presented() {
    use hullbreakers::sim::events::{Detach, PlayerEvent};
    let (_guard, mut s) = session(250);
    s.present();
    s.step(60);
    assert!(matches!(s.server_player().mode, Mode::Booted { .. }));
    let (_, _, rotation) = s.server_ship();
    let crew = s.server_crew();
    let mass = s.server.world().resource::<hullbreakers::sim::tuning::Tuning>().player_mass;
    // Straight up off the deck at 12 m/s: past any boots' grip.
    s.server.world_mut().resource_mut::<hullbreakers::sim::shots::Shoves>().0.push((crew, rotation * Vec3::Y * 12.0 * mass));
    s.step(40);
    assert!(matches!(s.server_player().mode, Mode::Eva { .. }), "the boots held");
    let mine = s.local_player().unwrap().mode;
    assert!(matches!(mine, Mode::Eva { .. }), "the client still stands: {mine:?} (server: {:?})", s.server_player().mode);
    s.step(80);
    let blasted = PlayerEvent::Detached(Detach::Blasted);
    assert_eq!(s.shown(blasted), 1, "the client was told of the blast {} times", s.shown(blasted));
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
    let crew = s.server_crew();
    s.server.world_mut().resource_mut::<hullbreakers::sim::shots::Shoves>().0.push((crew, Vec3::X * impulse));
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


/// A measuring tool, not a check (`cargo test --test netplay
/// replicated_changes -- --ignored --nocapture`): what a client receives
/// standing still in the field and in the boarding action, and how often
/// each replicated component is re-marked changed on the server over two
/// seconds (each mark sends it again), split between robots and the rest.
/// See `docs/perf.md`.
#[test]
#[ignore]
fn replicated_changes() {
    use hullbreakers::sim::bots::{Condition, Robot};
    use hullbreakers::sim::combat::Combatant;
    use hullbreakers::sim::devices::Devices;
    use hullbreakers::sim::fauna::Grazer;
    use hullbreakers::sim::mission::{Director, Mission, ScenarioKind};
    use hullbreakers::sim::motion::Motion;
    use hullbreakers::sim::pickups::Pickup;
    use hullbreakers::sim::player::PlayerTelemetry;
    use hullbreakers::sim::vitals::{HitLog, HurtLog, Vitals};
    use std::collections::HashMap;
    for scenario in [None, Some(ScenarioKind::Boarding)] {
        let (_guard, mut s) = session(0);
        if let Some(k) = scenario {
            s.server.world_mut().resource_mut::<Director>().start(k);
        }
        s.step(240);
        let mut last: HashMap<(Entity, &'static str), u32> = HashMap::new();
        let mut counts: HashMap<&'static str, (usize, usize)> = HashMap::new();
        let frames = 120;
        for _ in 0..frames {
            s.step(1);
            let world = s.server.world_mut();
            macro_rules! track {
                ($t:ty, $name:literal) => {{
                    let mut q = world.query::<(Entity, Ref<$t>, Option<&Robot>)>();
                    for (e, c, robot) in q.iter(world) {
                        let tick = c.last_changed().get();
                        let entry = counts.entry($name).or_default();
                        if last.insert((e, $name), tick).is_some_and(|t| t != tick) {
                            if robot.is_some() { entry.1 += 1 } else { entry.0 += 1 }
                        }
                    }
                }};
            }
            track!(Name, "Name");
            track!(Devices, "Devices");
            track!(Grazer, "Grazer");
            track!(Condition, "Condition");
            track!(Pickup, "Pickup");
            track!(Combatant, "Combatant");
            track!(Vitals, "Vitals");
            track!(HitLog, "HitLog");
            track!(HurtLog, "HurtLog");
            track!(Mission, "Mission");
            track!(Motion, "Motion");
            track!(PlayerTelemetry, "PlayerTelemetry");
            track!(Player, "Player");
            track!(hullbreakers::net::robots::RobotView, "RobotView");
            track!(hullbreakers::net::robots::RobotLog, "RobotLog");
        }
        let rate = s.received_rate(180);
        let mut v: Vec<_> = counts.into_iter().collect();
        v.sort();
        eprintln!("PROBE {scenario:?}: {rate:.0} B/s; changes in {frames} frames (non-robot, robot): {v:?}");
    }
}

/// Robots flying beside their ship (the carrier's, in the boarding action)
/// keep their place by it on a client at 100 ms, and move as smoothly as on
/// the server. Measured in the ship's frame, as one is seen beside it: how
/// far a robot is drawn from where the server has it relative to the ship
/// at that moment, and the change in its step from one frame to the next (a
/// jolt). Measured 2026-10-12, posed in the world: 7.6-7.8 m off (the
/// interpolation delay and the client's lead, at the ship's 27 m/s), jolts
/// up to 527 mm; in the ship's frame, with two send intervals' delay: 0.13-
/// 0.17 m, jolts up to 25 mm. See docs/perf.md.
#[test]
fn robot_smoothness() {
    use hullbreakers::sim::bots::Robot;
    use hullbreakers::sim::mission::{Director, ScenarioKind};
    use std::collections::HashMap;
    let (_guard, mut s) = session(100);
    s.server.world_mut().resource_mut::<Director>().start(ScenarioKind::Boarding);
    s.step(300);
    // Each robot in EVA, in its ship's frame.
    let sample = |app: &mut App| -> HashMap<Entity, Vec3> {
        let world = app.world_mut();
        // The nearest ship (the carrier its crew fly by).
        let ships: Vec<(Vec3, Quat)> = world
            .query_filtered::<(&Position, &Rotation), With<hullbreakers::sim::ship::ShipHull>>()
            .iter(world)
            .map(|(p, r)| (p.0, r.0))
            .collect();
        world
            .query_filtered::<(Entity, &Player), With<Robot>>()
            .iter(world)
            .filter_map(|(e, p)| match p.mode {
                Mode::Eva { position, .. } => {
                    let (at, rot) = ships.iter().min_by(|a, b| a.0.distance(position).total_cmp(&b.0.distance(position)))?;
                    Some((e, rot.inverse() * (position - *at)))
                }
                _ => None,
            })
            .collect()
    };
    let mut paths: [HashMap<Entity, Vec<Vec3>>; 2] = Default::default();
    for _ in 0..300 {
        s.step(1);
        for (i, app) in [&mut s.server, &mut s.client].into_iter().enumerate() {
            for (e, a) in sample(app) {
                paths[i].entry(e).or_default().push(a);
            }
        }
    }
    let jolts = |paths: &HashMap<Entity, Vec<Vec3>>| -> (usize, f32, f32) {
        let (mut n, mut sum, mut worst) = (0, 0.0f32, 0.0f32);
        for path in paths.values() {
            for w in path.windows(3) {
                let j = ((w[2] - w[1]) - (w[1] - w[0])).length();
                n += 1;
                sum += j;
                worst = worst.max(j);
            }
        }
        (paths.len(), sum * 1000.0 / n.max(1) as f32, worst * 1000.0)
    };
    // Each client robot's server twin: the nearest at the start.
    let mut offsets = Vec::new();
    for path in paths[1].values() {
        let Some(twin) = paths[0]
            .values()
            .filter(|p| p.len() == path.len())
            .min_by(|a, b| a[0].distance(path[0]).total_cmp(&b[0].distance(path[0])))
        else {
            continue;
        };
        offsets.extend(path.iter().zip(twin).map(|(a, b)| a.distance(*b)));
    }
    let mean_offset = offsets.iter().sum::<f32>() / offsets.len().max(1) as f32;
    for (name, paths) in ["server", "client"].iter().zip(&paths) {
        let (n, mean, worst) = jolts(paths);
        eprintln!("robots beside their ship, {name}: {n} in EVA; jolt mean {mean:.1} mm, worst {worst:.1} mm");
    }
    eprintln!("robots beside their ship, client: {:.0} mm from where the server has them", mean_offset * 1000.0);
    let (n, _, worst) = jolts(&paths[1]);
    assert!(n > 0, "no robots in EVA beside their ship");
    assert!(mean_offset < 0.6, "robots drawn {mean_offset:.2} m from their place by the ship");
    assert!(worst < 100.0, "a robot jolted {worst:.0} mm from one frame to the next");
}

/// A robot's shots reach a client as it fires them (its tracers and sounds
/// are drawn from them), each one there, at 100 ms.
#[test]
fn a_robots_shots_reach_a_client() {
    use hullbreakers::sim::bots::{Robot, SpawnRobots};
    use hullbreakers::sim::events::PlayerEvent;
    use std::collections::{HashMap, HashSet};
    let (_guard, mut s) = session(100);
    s.step(120);
    // A gunner on our bridge, facing aft at the crew.
    s.server.world_mut().write_message(SpawnRobots(Robot::Gunner));
    let shots = |app: &mut App, seen: &mut HashMap<&'static str, HashSet<(u32, u32)>>, side: &'static str| {
        let world = app.world_mut();
        for p in world.query_filtered::<&Player, With<Robot>>().iter(world) {
            for (tick, e) in p.events.iter() {
                if let PlayerEvent::Fired { origin, .. } = e {
                    // Where from, to the centimetre, and when: the same shot
                    // on either side.
                    let key = ((origin.x * 100.0).round() as i32 as u32 ^ (origin.z * 100.0).round() as i32 as u32, tick);
                    seen.entry(side).or_default().insert(key);
                }
            }
        }
    };
    let mut seen = HashMap::new();
    for _ in 0..480 {
        s.step(1);
        shots(&mut s.server, &mut seen, "server");
        shots(&mut s.client, &mut seen, "client");
    }
    let (server, client) = (seen.remove("server").unwrap_or_default(), seen.remove("client").unwrap_or_default());
    // The last few may still be on their way.
    let newest = server.iter().map(|k| k.1).max().unwrap_or(0);
    let missed = server.iter().filter(|k| newest.wrapping_sub(k.1) > 30 && !client.contains(k)).count();
    eprintln!("robot shots: server {}, client {}, missed {missed}", server.len(), client.len());
    assert!(server.len() > 3, "the gunner hardly fired: {}", server.len());
    assert_eq!(missed, 0, "the client missed {missed} of {} shots", server.len());
}

/// How fast a client receives the race's rock field (all of it, at once,
/// at the scenario's start). `RACE_ROCKS=n` sizes it (default the game's).
#[test]
#[ignore]
fn race_field_reaches_a_client() {
    use hullbreakers::sim::asteroids::Asteroid;
    use hullbreakers::sim::mission::{Director, ScenarioKind};
    if let Ok(n) = std::env::var("RACE_ROCKS") {
        // SAFETY: an ignored test, run alone.
        unsafe { std::env::set_var("HULLBREAKERS_RACE_ROCKS", n) };
    }
    let (_guard, mut s) = session(50);
    s.server.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
    let count = |app: &mut App| {
        let world = app.world_mut();
        world.query::<&Asteroid>().iter(world).count()
    };
    for second in 0..30 {
        let rate = s.received_rate(60) / 1000.0;
        let (host, client) = (count(&mut s.server), count(&mut s.client));
        eprintln!("{second:>3} s: host {host}, client {client}, {rate:.1} kB/s");
        if client == host && host > 0 {
            break;
        }
    }
    // And it stays.
    for _ in 0..10 {
        s.step(60);
        let (host, client) = (count(&mut s.server), count(&mut s.client));
        assert_eq!(client, host, "the client's field changed");
    }
}

/// Starts the race on the server and waits until the client has grown its
/// field, then a second more.
fn race(s: &mut Session) {
    use hullbreakers::sim::asteroids::Grown;
    use hullbreakers::sim::mission::{Director, ScenarioKind};
    s.server.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
    for _ in 0..40 {
        s.step(30);
        let world = s.client.world_mut();
        if world.query_filtered::<(), With<Grown>>().iter(world).next().is_some() {
            s.step(60);
            return;
        }
    }
    panic!("the client never grew the field");
}

/// A race's field reaches a client as its seed: the client grows the rocks
/// itself, and none at rest is sent (before, 6000 rocks at about 160 B
/// each, a second's worth at the pace).
#[test]
fn a_grown_field_is_not_sent() {
    use hullbreakers::sim::asteroids::Grown;
    use hullbreakers::sim::scenarios::grapple_run::{MOVERS, rock_count};
    let (_guard, mut s) = session(100);
    let before = s.client.world().resource::<Received>().0;
    race(&mut s);
    let received = s.client.world().resource::<Received>().0 - before;
    let world = s.client.world_mut();
    let grown = world.query_filtered::<(), With<Grown>>().iter(world).count();
    let sent = world.query_filtered::<(), (With<Asteroid>, With<Replicated>)>().iter(world).count();
    eprintln!("the race's start: {received} B to the client, {grown} rocks grown, {sent} sent");
    assert_eq!(grown, rock_count());
    // Only the movers (adrift from the start) are sent, onto the client's copies.
    assert_eq!(sent, MOVERS, "rocks at rest reached the client by replication");
    assert!(received < 100_000, "the race's start cost the client {received} B");
}

/// A rope on a grown rock: the client predicts it on its own copy of the
/// rock, and the authority's answer names the same rock (mapped onto it by
/// its signature), so nothing rolls back and the rock stays one rock.
#[test]
fn grappling_a_grown_rock_is_predicted() {
    use hullbreakers::sim::asteroids::Grown;
    use hullbreakers::sim::scenarios::grapple_run::{COURSE_AT, anchors, rock_count};
    let (_guard, mut s) = session(100);
    race(&mut s);
    // 15 m off the first anchor's reach, facing it.
    let (at, radius) = anchors()[0];
    let dir = (COURSE_AT - at).normalize();
    s.teleport(at + dir * (radius * 2.0 + 15.0), Vec3::ZERO, -dir);
    s.step(90);
    let rollbacks = s.rollbacks();
    s.input().0.presses.main.press();
    s.step(90);
    let player = s.local_player().unwrap();
    let grapple = player.grapple.expect("the grapple missed the anchor");
    let world = s.client.world_mut();
    let rocks = world.query_filtered::<(), With<Asteroid>>().iter(world).count();
    let grown = world.get::<Grown>(grapple.target).expect("hooked something not grown");
    assert_eq!(grown.index, 0, "hooked the wrong rock");
    assert!(world.get::<Replicated>(grapple.target).is_some(), "the authority's rock didn't map onto the client's");
    assert_eq!(rocks, rock_count(), "the client has a second copy of a rock");
    let server_target = s.server_player().grapple.expect("the authority's rope isn't on").target;
    let world = s.server.world_mut();
    assert_eq!(world.get::<Grown>(server_target).unwrap().index, 0);
    assert!(world.get::<Replicate>(server_target).is_some(), "the authority isn't replicating the hooked rock");
    let rolled_back = s.rollbacks() - rollbacks;
    eprintln!("grapple on a grown anchor: {rolled_back} rollbacks");
    assert_eq!(rolled_back, 0);
}

/// A grown rock knocked adrift reaches the client as its motion, on the
/// client's own copy of it.
#[test]
fn a_knocked_rock_reaches_the_client() {
    use hullbreakers::sim::asteroids::Grown;
    use hullbreakers::sim::motion::Motion;
    use hullbreakers::sim::scenarios::grapple_run::rock_count;
    let (_guard, mut s) = session(100);
    race(&mut s);
    // The first rock that isn't fixed (a handhold), given a shove.
    let world = s.server.world_mut();
    let (rock, index) = world
        .query::<(Entity, &Grown, &Asteroid)>()
        .iter(world)
        .filter(|(_, _, a)| !a.fixed)
        .min_by_key(|(_, g, _)| g.index)
        .map(|(e, g, _)| (e, g.index))
        .unwrap();
    let before = world.get::<Position>(rock).unwrap().0;
    world.get_mut::<LinearVelocity>(rock).unwrap().0 = Vec3::new(2.0, 0.0, 0.0);
    s.step(120);
    let world = s.server.world_mut();
    let moved = world.get::<Position>(rock).unwrap().0;
    assert!((moved - before).length() > 3.0, "the rock didn't drift on the server ({:.2} m)", (moved - before).length());
    let world = s.client.world_mut();
    let rocks = world.query_filtered::<(), With<Asteroid>>().iter(world).count();
    let (client_rock, _) = world.query::<(Entity, &Grown)>().iter(world).find(|(_, g)| g.index == index).expect("the client lacks the rock");
    assert!(world.get::<Motion>(client_rock).is_some(), "the knocked rock's motion didn't reach the client");
    // Its physics state follows its motion (the rig has no presentation,
    // so nothing draws it).
    let posed = world.get::<Position>(client_rock).unwrap().0;
    eprintln!("knocked rock: server {moved:.2}, client {posed:.2}, started {before:.2}");
    assert!((posed - moved).length() < 1.0, "the client has the rock {:.2} m from the server", (posed - moved).length());
    assert_eq!(rocks, rock_count(), "the client has a second copy of a rock");
}

/// A client joining a race in progress finds rocks already in play (the
/// movers, one knocked before it came) in its world before it grows the
/// field: it grows onto them, so it ends up with one rock each, moving.
#[test]
fn a_late_joiner_grows_onto_the_rocks_already_in_play() {
    use hullbreakers::sim::asteroids::Grown;
    use hullbreakers::sim::mission::{Director, ScenarioKind};
    use hullbreakers::sim::motion::Motion;
    use hullbreakers::sim::scenarios::grapple_run::{MOVERS, rock_count};
    let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let lag = SimulatedLag {
        latency_ms: 100,
        loss_percent: 0.0,
    };
    let mut s = Session::start_with(Some(lag), |server| {
        server.world_mut().resource_mut::<Director>().start(ScenarioKind::GrappleRun);
        for _ in 0..60 {
            MockClock::advance(FRAME);
            server.update();
        }
        // The first rock at rest, knocked before anyone joins.
        let world = server.world_mut();
        let rock = world
            .query::<(Entity, &Grown, &Asteroid, &LinearVelocity)>()
            .iter(world)
            .filter(|(_, _, a, v)| !a.fixed && v.0 == Vec3::ZERO)
            .min_by_key(|(_, g, _, _)| g.index)
            .map(|(e, ..)| e)
            .unwrap();
        world.get_mut::<LinearVelocity>(rock).unwrap().0 = Vec3::new(1.0, 0.0, 0.0);
    });
    race_grown(&mut s);
    s.step(120);
    let world = s.client.world_mut();
    let rocks = world.query_filtered::<(), With<Asteroid>>().iter(world).count();
    let grown = world.query_filtered::<(), With<Grown>>().iter(world).count();
    let moving = world.query_filtered::<(), (With<Grown>, With<Motion>)>().iter(world).count();
    eprintln!("late joiner: {rocks} rocks, {grown} grown, {moving} moving");
    assert_eq!(rocks, rock_count(), "a rock in play came twice");
    assert_eq!(grown, rock_count());
    // The movers, the knocked rock, and whatever a mover has stirred by now.
    assert!(moving >= MOVERS + 1, "the rocks in play aren't moving on the client ({moving})");
    drop(guard);
}

/// Waits until the client has grown its field.
fn race_grown(s: &mut Session) {
    use hullbreakers::sim::asteroids::Grown;
    for _ in 0..40 {
        s.step(30);
        let world = s.client.world_mut();
        if world.query_filtered::<(), With<Grown>>().iter(world).next().is_some() {
            return;
        }
    }
    panic!("the client never grew the field");
}
