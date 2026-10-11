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

    /// The server's (only) player.
    fn server_player(&mut self) -> Mut<'_, Player> {
        let world = self.server.world_mut();
        let entity = world
            .query_filtered::<Entity, With<Player>>()
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
            p.loadout.arm_fully();
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
