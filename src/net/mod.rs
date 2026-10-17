//! Networking: host-authoritative co-op over lightyear.
//!
//! One process is authoritative (a listen-server host, or a dedicated server)
//! and runs the [`SimPlugin`]; clients receive the world by replication. What
//! is replicated is the state the simulation already has: generating
//! components that every peer expands locally ([`ShipHull`], [`Asteroid`]),
//! body poses and velocities (via lightyear's Avian integration), and player
//! state, whose event log carries what happened to each player.
//!
//! For now everything a client sees is interpolated from the server,
//! including its own player; input reaches the server through lightyear's
//! input channel.

use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use core::time::Duration;

use avian3d::prelude::*;
use bevy::prelude::*;
use lightyear::netcode::NetcodeServer;
use lightyear::prelude::client::*;
use lightyear::prelude::input::native::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use serde::{Deserialize, Serialize};

use crate::sim::asteroids::{Asteroid, Grown, RockField};
use crate::sim::bots::{Affixes, Condition, Robot, Wrecked};
use crate::sim::pickups::Pickup;
use crate::sim::combat::Combatant;
use crate::sim::vitals::{HitLog, HurtLog, Vitals};
use crate::sim::flight::Flight;
use crate::sim::input::PlayerInput;
use crate::sim::devices::Devices;
use crate::sim::mission::{Debris, Mission};
use crate::sim::scenarios::gallery::GalleryAnchor;
use crate::sim::events::AuthorityLog;
use crate::sim::player::{DeckSlot, Mode, Player, PlayerTelemetry, PreviousMode, new_player};
use crate::sim::ship::{Drift, HomeShip, ShipHull, Site};
use crate::sim::tuning::Tuning;
use crate::sim::motion::{Motion, rotation_error};
use crate::sim::bodies::Bodies;
use crate::sim::{PlayerSimPlugin, SimPlugin, SimSharedPlugin, SimSystems, SimTick, TICK_HZ};

mod extrapolate;
mod profiles;
pub mod robots;

pub use profiles::ProfileRequests;

pub const DEFAULT_PORT: u16 = 5888;
const PROTOCOL_ID: u64 = 0x5ace_9a3e;
/// Shared netcode key. Direct-IP play between friends for now; a relay or
/// matchmaking service would hand out connect tokens instead.
const PRIVATE_KEY: [u8; 32] = [0; 32];
/// How often the server sends world state.
const SEND_INTERVAL: Duration = Duration::from_millis(50);
/// How far back (ticks) a client will re-simulate to take a correction. A
/// confirmed state is the client's lead over the authority plus the trip
/// back old when it arrives: about 22 ticks at 250 ms. lightyear's default
/// (20) quietly refuses anything older, so past ~220 ms of round trip a
/// client was never corrected at all (an authority's shove left it standing
/// on the deck). A second covers a round trip of about 650 ms.
const MAX_ROLLBACK_TICKS: u16 = 60;
/// The in-process client of a listen-server host.
const HOST_PEER: PeerId = PeerId::Local(0);

/// The player this machine presents and controls.
#[derive(Component)]
pub struct LocalPlayer;

/// A client that hasn't got its player yet (its clock in step with the
/// host's, the host's world arriving): what the loading veil shows.
#[derive(Resource, Clone, Copy, Debug)]
pub struct Joining {
    pub server: SocketAddr,
}

/// Client: joined once it has its player to present.
fn finish_joining(joining: Option<Res<Joining>>, local: Query<(), With<LocalPlayer>>, mut commands: Commands) {
    if joining.is_some() && !local.is_empty() {
        commands.remove_resource::<Joining>();
    }
}

/// This machine's input for its player, written by the input layer and sent
/// to the authority each tick. Presentation also reads it for what the
/// simulation hasn't applied yet (zero-latency look).
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct LocalInput(pub PlayerInput);

#[derive(Clone, Copy, Debug)]
pub enum NetMode {
    /// Play and serve at once (listen server).
    Host { port: u16 },
    /// Join a host or server.
    Client { server: SocketAddr },
    /// Serve without playing.
    Server { port: u16 },
}

impl NetMode {
    fn is_authority(self) -> bool {
        !matches!(self, NetMode::Client { .. })
    }

    fn plays(self) -> bool {
        !matches!(self, NetMode::Server { .. })
    }
}

/// The host's simulation tuning, replicated so every peer runs the same
/// numbers. Each player keeps their own camera and debug settings.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostTuning(pub Tuning);

/// The engine plugins a peer without a window needs (a dedicated server, or
/// a test client). `frame` paces the main loop when the app runs itself.
pub fn add_headless_plugins(app: &mut App, frame: Duration) {
    app.add_plugins((
        MinimalPlugins.set(bevy::app::ScheduleRunnerPlugin::run_loop(frame)),
        bevy::state::app::StatesPlugin,
        TransformPlugin,
        AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::scene::ScenePlugin,
    ));
}

pub struct NetPlugin {
    pub mode: NetMode,
    /// Applied to what a client receives.
    pub lag: Option<SimulatedLag>,
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        let mode = self.mode;
        let lag = self.lag;
        let tick = Duration::from_secs_f64(1.0 / TICK_HZ);
        if mode.is_authority() {
            app.add_plugins(ServerPlugins {
                tick_duration: tick,
            });
        }
        if mode.plays() {
            app.add_plugins(ClientPlugins {
                tick_duration: tick,
            });
        }
        app.add_plugins(ProtocolPlugin)
            // Wherever a player is simulated, it is from the input lightyear
            // holds for this tick.
            .add_systems(
                FixedUpdate,
                apply_network_input.before(SimSystems::RecordVelocities),
            );
        // Ticks are lightyear's on every peer, so a body's motion means the
        // same thing everywhere.
        app.add_systems(
            FixedFirst,
            follow_network_tick
                .after(lightyear::prelude::TimelineSystems::IncrementLocal)
                .after(crate::sim::advance_tick),
        );

        if mode.is_authority() {
            // A host draws its own bodies between ticks; a server draws nothing.
            let physics = if mode.plays() {
                PhysicsPlugins::default().set(PhysicsInterpolationPlugin::interpolate_all())
            } else {
                PhysicsPlugins::default().build()
            };
            app.add_plugins((
                physics,
                SimPlugin {
                    asteroids: true,
                    local_player: false,
                },
            ))
            .insert_resource(ReplicationMetadata::new(SEND_INTERVAL))
            .add_observer(on_link)
            .add_observer(replicate_body::<Drift>)
            .add_observer(replicate_rock)
            // A grazer changes its own motion only in its pulses: between
            // them it's in free flight like a rock (see `sim::fauna`).
            .add_observer(replicate_body::<crate::sim::fauna::Grazer>)
            .add_observer(replicate_body::<Debris>)
            .add_observer(replicate_body::<Site>)
            .add_observer(replicate_body::<crate::sim::scenarios::recovery::Pod>)
            .add_observer(replicate_flight)
            .add_observer(replicate_mission)
            .add_observer(replicate_gallery)
            .add_observer(replicate_ambience)
            .add_observer(replicate_robot)
            .add_observer(replicate_pickup)
            // Measured only when profiling (see `client::profile`).
            .add_systems(PostUpdate, meter_sends.after(lightyear::transport::plugin::TransportSystems::Send).before(LinkSystems::Send).run_if(resource_exists::<SendMeter>))
            .add_systems(Startup, spawn_host_tuning)
            .add_systems(Update, (publish_tuning, spawn_players, profiles::receive_profiles))
            .add_systems(
                FixedPostUpdate,
                ((replicate_touched_rocks, extrapolate::track_motion).chain(), robots::publish_robots).after(SimSystems::Players),
            );
        } else {
            // Bodies here are posed from their motion, not simulated: Avian
            // only answers spatial queries, and must not sync Transform back
            // into Position.
            app.add_plugins((
                PhysicsPlugins::default()
                    .build()
                    .disable::<PhysicsTransformPlugin>()
                    .disable::<PhysicsInterpolationPlugin>(),
                SimSharedPlugin,
                // Simulates (predicts) this client's own player.
                PlayerSimPlugin,
            ))
            .add_observer(extrapolate::prepare_extrapolated)
            .add_observer(extrapolate::prepare_flight)
            .add_systems(Update, (adopt_host_tuning, finish_joining))
            // Interpolate two send intervals behind the authority's latest,
            // not lightyear's 1.2: at 1.2 the next sample was often not in
            // yet when it was needed (the authority sends every third tick),
            // so a moving thing held a moment and then caught up, which a
            // fast one shows as a stutter (`netplay::robot_smoothness`:
            // 56-87 mm of jolt on a 450 mm step, 4 mm at 2). It costs 40 ms
            // more behind.
            .insert_resource(lightyear::prelude::InterpolationConfig::default().with_send_interval_ratio(INTERPOLATION_SEND_INTERVALS))
            .init_resource::<extrapolate::DrawClock>()
            .init_resource::<ViewBeforeRollback>()
            .configure_sets(
                FixedPostUpdate,
                SimSystems::Players.before(PredictionSystems::UpdateHistory),
            )
            .add_systems(
                FixedUpdate,
                extrapolate::follow_motion
                    .in_set(SimSystems::Drive)
                    .run_if(extrapolate::synced),
            )
            .add_systems(
                PreUpdate,
                (
                    note_view_before_rollback.before(PredictionSystems::Rollback),
                    hand_rollback_to_camera.after(PredictionSystems::Rollback),
                ),
            )
            .add_systems(Update, report_rollbacks)
            .add_systems(Update, robots::present_robots.in_set(lightyear::prelude::InterpolationSystems::Interpolate))
            .add_systems(
                PostUpdate,
                (
                    extrapolate::advance_draw_clock,
                    (extrapolate::draw_extrapolated, extrapolate::draw_flights),
                )
                    .chain()
                    .in_set(crate::client::BodyPoses),
            );
        }
        if mode.plays() {
            // Required by lightyear's client input pipeline (input history
            // for rollback), even before anything is predicted.
            app.init_resource::<LocalInput>()
                .init_resource::<ProfileRequests>()
                .add_systems(Update, profiles::send_profile(mode.is_authority()))
                .add_systems(Last, profiles::save_profile)
                .insert_resource({
                    let mut prediction = PredictionManager::default();
                    prediction.rollback_policy.max_rollback_ticks = MAX_ROLLBACK_TICKS;
                    prediction
                })
                // A client can't simulate its player before its clock (and
                // so the bodies around it) is in step with the authority.
                .add_systems(
                    PreUpdate,
                    mark_local_player.run_if(move |sync: Option<Res<LocalTimelineSync>>| {
                        mode.is_authority() || extrapolate::synced(sync)
                    }),
                )
                .add_systems(
                    FixedPreUpdate,
                    write_local_input.in_set(lightyear::prelude::client::input::InputSystems::WriteClientInputs),
                );
            order_presentation(app);
        }

        match mode {
            NetMode::Host { port } => {
                app.add_systems(Startup, move |mut commands: Commands| {
                    let server = start_server(&mut commands, port);
                    let host = commands
                        .spawn((Name::new("Host client"), Client, LinkOf { server }))
                        .id();
                    commands.trigger(Connect { entity: host });
                });
            }
            NetMode::Server { port } => {
                app.add_systems(Startup, move |mut commands: Commands| {
                    start_server(&mut commands, port);
                });
            }
            NetMode::Client { server } => {
                app.insert_resource(Joining { server }).add_systems(Startup, move |mut commands: Commands| {
                    connect(&mut commands, server, lag);
                });
            }
        }
    }
}

/// How far behind the authority's latest a client interpolates others,
/// in the authority's send intervals (see where it's set).
const INTERPOLATION_SEND_INTERVALS: f32 = 2.0;

struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn finish(&self, app: &mut App) {
        profiles::finish(app);
    }

    fn build(&self, app: &mut App) {
        app.add_plugins(InputPlugin::<PlayerInput>::default());
        profiles::register(app);
        app.component::<Name>().replicate();
        app.component::<ShipHull>().replicate_once();
        app.component::<HomeShip>().replicate_once();
        app.component::<Site>().replicate_once();
        app.component::<Devices>().replicate();
        app.component::<crate::sim::scenarios::recovery::Pod>().replicate();
        app.component::<crate::sim::mission::BodyId>().replicate_once();
        app.component::<Asteroid>().replicate_once();
        // A whole field of rocks: every peer grows it (see `sim::asteroids`).
        app.component::<RockField>().replicate_once();
        // Its mood and when it began: every peer times its pulses from it.
        app.component::<crate::sim::fauna::Grazer>().replicate();
        app.component::<Debris>().replicate_once();
        app.component::<GalleryAnchor>().replicate_once();
        // The sky, whole (the sky preview changes it as it goes).
        app.component::<crate::sim::ambience::Ambience>().replicate();
        app.component::<Robot>().replicate_once();
        // A robot reaches a remote client as what's drawn of it, not its
        // whole player (see `robots`).
        app.component::<robots::RobotView>()
            .replicate()
            .add_interpolation_with(robots::interpolate_view);
        app.component::<robots::RobotLog>()
            .replicate()
            .add_interpolation_with(robots::snap);
        app.component::<Affixes>().replicate_once();
        app.component::<Condition>().replicate();
        app.component::<Wrecked>().replicate_once();
        app.component::<DeckSlot>().replicate_once();
        app.component::<Pickup>().replicate();
        app.component::<Flight>().replicate();
        app.component::<Combatant>().replicate();
        app.component::<Vitals>().replicate();
        app.component::<HitLog>().replicate();
        app.component::<HurtLog>().replicate();
        // What only the authority decides happens to a player: never
        // predicted, so never rolled back, and read by count (see
        // `client::events`).
        app.component::<AuthorityLog>().replicate();
        app.component::<Mission>().replicate();
        app.component::<Motion>().replicate();
        app.component::<HostTuning>().replicate();
        // Derived each tick wherever the player is simulated; never worth a
        // rollback of its own.
        app.component::<PlayerTelemetry>()
            .replicate()
            .predict()
            .with_rollback_condition(|_, _| false);
        // A client predicts its own player and interpolates the others'.
        // (Robots' go as their `RobotView`.)
        app.component::<Player>()
            .replicate_filtered::<Without<Robot>>()
            .predict()
            .with_rollback_condition(player_diverged)
            .add_interpolation_with(interpolate_player);
    }
}

/// Whether a predicted player state has strayed from the authority's state
/// for the same tick far enough to re-simulate. Tolerances sit above the
/// noise of extrapolated body poses (1 cm) so agreement isn't re-simulated.
/// Events don't count: what a client can't predict comes in the player's
/// [`AuthorityLog`], not by a correction.
fn player_diverged(confirmed: &Player, predicted: &Player) -> bool {
    // A body this client hasn't received yet: nothing to predict against,
    // so nothing a rollback could fix.
    let unknown = |body: Option<Entity>| body == Some(Entity::PLACEHOLDER);
    if unknown(confirmed.mode.body()) || unknown(confirmed.reference_body) {
        return false;
    }
    const DISTANCE: f32 = 0.02;
    const SPEED: f32 = 0.02;
    const ANGLE: f32 = 0.005;
    let far = |a: Vec3, b: Vec3, tolerance: f32| a.distance(b) > tolerance;
    let turned = |a: Quat, b: Quat| rotation_error(a, b) > ANGLE;
    let modes = match (confirmed.mode, predicted.mode) {
        (
            Mode::Eva {
                position: p0,
                velocity: v0,
                orientation: o0,
                pitch: a0,
            },
            Mode::Eva {
                position: p1,
                velocity: v1,
                orientation: o1,
                pitch: a1,
            },
        ) => far(p0, p1, DISTANCE) || far(v0, v1, SPEED) || turned(o0, o1) || (a0 - a1).abs() > ANGLE,
        (
            Mode::Booted {
                body: b0,
                feet: f0,
                up: u0,
                velocity: v0,
                orientation: o0,
                pitch: a0,
            },
            Mode::Booted {
                body: b1,
                feet: f1,
                up: u1,
                velocity: v1,
                orientation: o1,
                pitch: a1,
            },
        ) => {
            b0 != b1
                || far(f0, f1, DISTANCE)
                || u0.angle_between(u1) > ANGLE
                || far(v0, v1, SPEED)
                || turned(o0, o1)
                || (a0 - a1).abs() > ANGLE
        }
        _ => true,
    };
    let grapples = match (confirmed.grapple, predicted.grapple) {
        (None, None) => false,
        (Some(g0), Some(g1)) => {
            g0.target != g1.target || far(g0.anchor, g1.anchor, DISTANCE) || (g0.length - g1.length).abs() > DISTANCE
        }
        _ => true,
    };
    let diverged = modes
        || grapples
        || confirmed.boots_on != predicted.boots_on
        || confirmed.flight_assist != predicted.flight_assist
        || confirmed.reference_body != predicted.reference_body
        || confirmed.input != predicted.input
        // What's carried is discrete: a pickup the authority settled
        // otherwise (someone else got there first) must be corrected.
        || confirmed.loadout.inventory != predicted.loadout.inventory;
    if diverged {
        debug!(
            "diverged: modes {modes} grapple {grapples} input {}\n  confirmed {:?}\n  predicted {:?}",
            confirmed.input != predicted.input,
            confirmed.mode,
            predicted.mode
        );
    }
    diverged
}

/// Moves a player smoothly between two received states. Within one mode
/// (and one body) positions and orientations blend; across a change of mode
/// or body the state switches at the halfway point. Everything else (flags,
/// grapple, the event log) comes from the nearer state.
/// A player's (or robot's) mode, between two states: blended within a
/// mode on one body, else the nearer end.
pub(crate) fn interpolate_mode(start: Mode, end: Mode, t: f32) -> Mode {
    match (start, end) {
        (
            Mode::Eva {
                position: p0,
                velocity: v0,
                orientation: o0,
                pitch: a0,
            },
            Mode::Eva {
                position: p1,
                velocity: v1,
                orientation: o1,
                pitch: a1,
            },
        ) => Mode::Eva {
            position: p0.lerp(p1, t),
            velocity: v0.lerp(v1, t),
            orientation: o0.slerp(o1, t),
            pitch: a0 + (a1 - a0) * t,
        },
        (
            Mode::Booted {
                body: b0,
                feet: f0,
                up: u0,
                velocity: v0,
                orientation: o0,
                pitch: a0,
            },
            Mode::Booted {
                body: b1,
                feet: f1,
                up: u1,
                velocity: v1,
                orientation: o1,
                pitch: a1,
            },
        ) if b0 == b1 => Mode::Booted {
            body: b1,
            feet: f0.lerp(f1, t),
            up: u0.slerp(u1, t),
            velocity: v0.lerp(v1, t),
            orientation: o0.slerp(o1, t),
            pitch: a0 + (a1 - a0) * t,
        },
        _ if t < 0.5 => start,
        _ => end,
    }
}

fn interpolate_player(start: Player, end: Player, t: f32) -> Player {
    let mode = interpolate_mode(start.mode, end.mode, t);
    let mut player = if t < 0.5 { start } else { end };
    player.mode = mode;
    player
}

fn start_server(commands: &mut Commands, port: u16) -> Entity {
    let server = commands
        .spawn((
            Name::new("Server"),
            NetcodeServer::new(lightyear::netcode::prelude::server::NetcodeConfig {
                protocol_id: PROTOCOL_ID,
                private_key: PRIVATE_KEY,
                // A connect token names the address the client dialled. A
                // host can't know that (LAN IP, VPN IP, public IP through a
                // forwarded port), and with direct-IP play and a shared key
                // the check protects nothing.
                server_addr_check: false,
                ..default()
            }),
            LocalAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)),
            ServerUdpIo::default(),
        ))
        .id();
    commands.trigger(Start { entity: server });
    server
}

fn connect(commands: &mut Commands, server: SocketAddr, lag: Option<SimulatedLag>) {
    let auth = Authentication::Manual {
        server_addr: server,
        client_id: rand::random(),
        private_key: PRIVATE_KEY,
        protocol_id: PROTOCOL_ID,
    };
    let client = commands
        .spawn((
            Name::new("Client"),
            Client,
            ReplicationReceiver,
            LocalAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)),
            PeerAddr(server),
            Link::default().with_conditioner(lag.map(SimulatedLag::conditioner)),
            NetcodeClient::new(
                auth,
                lightyear::netcode::client_plugin::NetcodeConfig::default(),
            )
            .expect("valid netcode config"),
            UdpIo::default(),
        ))
        .id();
    commands.trigger(Connect { entity: client });
}

/// A simulated bad link: what a client receives is delayed (±10% jitter)
/// and some of it lost. For testing prediction.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SimulatedLag {
    pub latency_ms: u64,
    pub loss_percent: f32,
}

impl SimulatedLag {
    /// From `HULLBREAKERS_LAG=ms[,loss%]`.
    pub fn from_env() -> Option<Self> {
        let spec = std::env::var("HULLBREAKERS_LAG").ok()?;
        let mut parts = spec.split(',');
        Some(Self {
            latency_ms: parts.next()?.trim().parse().ok()?,
            loss_percent: parts.next().and_then(|p| p.trim().parse().ok()).unwrap_or(0.0),
        })
    }

    fn conditioner(self) -> RecvLinkConditioner {
        info!(
            "simulating {} ms latency, {}% loss on receive",
            self.latency_ms, self.loss_percent
        );
        RecvLinkConditioner::new(LinkConditionerConfig {
            incoming_latency: Duration::from_millis(self.latency_ms),
            incoming_jitter: Duration::from_millis(self.latency_ms / 10),
            good_loss: self.loss_percent / 100.0,
            ..default()
        })
    }
}

/// Client: how often prediction was wrong, every 10 s.
fn report_rollbacks(time: Res<Time>, metrics: Res<PredictionMetrics>, mut last: Local<(f32, u32, u32)>) {
    last.0 += time.delta_secs();
    if last.0 < 10.0 {
        return;
    }
    let (rollbacks, ticks) = (metrics.rollbacks - last.1, metrics.rollback_ticks - last.2);
    info!(
        "prediction: {:.1} rollbacks/s, {:.1} ticks each",
        rollbacks as f32 / last.0,
        ticks as f32 / rollbacks.max(1) as f32
    );
    *last = (0.0, metrics.rollbacks, metrics.rollback_ticks);
}

/// Authority: what's been sent to each remote client, in payloads (packets)
/// and bytes (before the transport's own headers), counted as each frame's
/// sends are handed to the transport. For measuring (see `client::profile`).
#[derive(Resource, Default, Debug)]
pub struct SendMeter {
    pub links: std::collections::HashMap<Entity, (u64, u64)>,
}

fn meter_sends(mut meter: ResMut<SendMeter>, mut links: Query<(Entity, &mut Link, &RemoteId), With<LinkOf>>) {
    for (entity, mut link, peer) in &mut links {
        // The host's own client is in-process: nothing goes over a wire.
        if peer.0 == HOST_PEER {
            continue;
        }
        // (The queue can only be read by draining it; the payloads are
        // shared buffers, so putting them back copies nothing.)
        let queued: Vec<_> = link.send.drain().collect();
        let entry = meter.links.entry(entity).or_default();
        for payload in queued {
            entry.0 += 1;
            entry.1 += payload.len() as u64;
            link.send.push(payload);
        }
    }
}

/// The most the authority sends each client a second, and in one go
/// (bytes). lightyear sends everything at once by default, and a scenario
/// that spawns thousands of things (the race's rock field: 2000 rocks are
/// 320 kB) overflowed a client's UDP receive buffer (212 kB by default on
/// Linux): the kernel dropped hundreds of datagrams, and the reliable resends
/// took the field over 20 s to arrive (measured: `RcvbufErrors` in
/// /proc/net/snmp; 8000 rocks timed the client out). Paced, reliable messages
/// wait their turn instead. Play itself sends 15-45 kB/s (docs/perf.md).
const SEND_QUOTA: u32 = 1_000_000;
const SEND_BURST: u32 = 64_000;

/// Sends aren't paced while this resource is in: for the netplay test
/// rig, whose clock is mocked (and runs several times faster than the wall
/// clock the pacing goes by, so paced, a test's waits would depend on how
/// fast the machine is).
#[derive(Resource)]
pub struct Unpaced;

fn on_link(add: On<Add, LinkOf>, unpaced: Option<Res<Unpaced>>, mut commands: Commands) {
    commands.entity(add.entity).insert(ReplicationSender);
    if unpaced.is_none() {
        // Our own transport. A remote client's link has none yet (it comes
        // with `ClientOf`, so this is the first, and gets the channels as
        // that's added); the host's own client's is replaced, and gets
        // them again as this is inserted.
        commands
            .entity(add.entity)
            .insert(Transport::new(PriorityConfig::new(SEND_QUOTA).with_burst_size(SEND_BURST)));
    }
}

/// The player a connected client controls.
#[derive(Component)]
struct HasPlayer;

/// Gives every connected client without a player one, on the ship's deck.
/// A system rather than a `Connected` observer: the host's own client
/// connects during startup, before the ship exists.
fn spawn_players(
    clients: Query<(Entity, &RemoteId), (With<ClientOf>, With<Connected>, Without<HasPlayer>)>,
    ship: Query<Entity, With<HomeShip>>,
    slots: Query<&DeckSlot>,
    tuning: Res<Tuning>,
    mut commands: Commands,
) {
    let Ok(ship) = ship.single() else { return };
    let mut taken: Vec<u32> = slots.iter().map(|s| s.0).collect();
    for (client, peer) in &clients {
        let index = (0..).find(|i| !taken.contains(i)).expect("unbounded");
        taken.push(index);
        info!("client {client:?} joined as player {}", index + 1);
        commands.entity(client).insert(HasPlayer);
        let owner = peer.0;
        // The owner predicts its player; everyone else (but the host, who
        // shares the authority's world) interpolates it. The host's own
        // player is simply authoritative.
        let interpolate = InterpolationTarget::to_clients(NetworkTarget::AllExcept(
            [owner, HOST_PEER].into_iter().collect(),
        ));
        let mut player = commands.spawn((
            new_player(ship, index),
            Vitals::full(&tuning),
            ActionState::<PlayerInput>::default(),
            Replicate::to_clients(NetworkTarget::All),
            interpolate,
            ControlledBy {
                owner: client,
                lifetime: Default::default(),
            },
        ));
        if owner != HOST_PEER {
            player.insert((PredictionTarget::to_clients(NetworkTarget::Single(owner)), profiles::ProfileOwner(client)));
        }
    }
}

/// Bodies reach clients by their motion (see [`extrapolate`]). The ship's
/// visual parts are children every peer builds for itself.
fn replicate_body<C: Component>(add: On<Add, C>, mut commands: Commands) {
    commands.entity(add.entity).insert((
        Replicate::to_clients(NetworkTarget::All),
        extrapolate::Extrapolated,
        DisableReplicateHierarchy,
    ));
}

/// Rocks reach clients like any body, except a field's grown ones (see
/// [`RockField`]): a client grows those itself, and hears about one only
/// once something happens to it ([`replicate_touched_rocks`]).
fn replicate_rock(add: On<Add, Asteroid>, grown: Query<(), With<Grown>>, mut commands: Commands) {
    if grown.contains(add.entity) {
        return;
    }
    commands.entity(add.entity).insert((
        Replicate::to_clients(NetworkTarget::All),
        extrapolate::Extrapolated,
        DisableReplicateHierarchy,
    ));
}

/// A grown rock something has happened to is replicated from then on, like
/// any body: a player (a robot too) stands on it, holds a rope to it or
/// rides it as their frame, so their replicated state refers to it; or it's
/// been knocked adrift, so its motion matters. It reaches a client mapped
/// onto the client's own copy of it (by its signature: see
/// [`RockField`]), so what's sent lands on the rock the client already
/// has, and a player's reference to it resolves to the same rock the
/// client's prediction chose. (Knocked, it also never sleeps again:
/// `asteroids::keep_knocked_adrift`.)
fn replicate_touched_rocks(
    players: Query<&Player>,
    rocks: Query<(Entity, &RigidBody, &LinearVelocity, &AngularVelocity), (With<Grown>, Without<Replicate>)>,
    mut commands: Commands,
) {
    let mut touched: Vec<Entity> = players
        .iter()
        .flat_map(|p| [p.mode.body(), p.grapple.map(|g| g.target), p.reference_body])
        .flatten()
        .filter(|e| rocks.contains(*e))
        .collect();
    for (rock, body, linear, angular) in &rocks {
        if body.is_dynamic() && (linear.0 != Vec3::ZERO || angular.0 != Vec3::ZERO) {
            touched.push(rock);
        }
    }
    for rock in touched {
        commands.entity(rock).insert((
            Replicate::to_clients(NetworkTarget::All),
            extrapolate::Extrapolated,
            DisableReplicateHierarchy,
        ));
    }
}

/// Ships flying a scripted path reach clients as that path (see
/// [`crate::sim::flight`]), sent when it changes.
fn replicate_flight(add: On<Add, Flight>, mut commands: Commands) {
    commands.entity(add.entity).insert((
        Replicate::to_clients(NetworkTarget::All),
        DisableReplicateHierarchy,
    ));
}

fn replicate_mission(add: On<Add, Mission>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(Replicate::to_clients(NetworkTarget::All));
}

/// The gallery reaches clients as just its anchor; each sets out the models
/// it has (see `client::gallery`).
fn replicate_gallery(add: On<Add, GalleryAnchor>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(Replicate::to_clients(NetworkTarget::All));
}

/// A place's sky reaches clients as its whole description, which each
/// draws (see `client::ambience`).
fn replicate_ambience(add: On<Add, crate::sim::ambience::Ambience>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(Replicate::to_clients(NetworkTarget::All));
}

/// Robots reach clients interpolated, never predicted, as what's drawn of
/// them (see [`robots`]). (The host shares the authority's world.)
fn replicate_robot(add: On<Add, Robot>, mut commands: Commands) {
    commands.entity(add.entity).insert((
        Replicate::to_clients(NetworkTarget::All),
        InterpolationTarget::to_clients(NetworkTarget::AllExcept([HOST_PEER].into_iter().collect())),
    ));
}

/// Pickups reach clients as their description (see `sim::pickups`).
fn replicate_pickup(add: On<Add, Pickup>, mut commands: Commands) {
    commands
        .entity(add.entity)
        .insert(Replicate::to_clients(NetworkTarget::All));
}

fn spawn_host_tuning(mut commands: Commands, tuning: Res<Tuning>) {
    commands.spawn((
        Name::new("Host tuning"),
        HostTuning(tuning.clone()),
        Replicate::to_clients(NetworkTarget::All),
    ));
}

fn publish_tuning(tuning: Res<Tuning>, mut host: Single<&mut HostTuning>) {
    if tuning.is_changed() && host.0 != *tuning {
        host.0 = tuning.clone();
    }
}

fn adopt_host_tuning(host: Query<&HostTuning, Changed<HostTuning>>, mut tuning: ResMut<Tuning>) {
    for host in &host {
        tuning.adopt_shared(&host.0);
    }
}

/// The authority simulates each player from the input lightyear received
/// for this tick.
fn apply_network_input(mut players: Query<(&ActionState<PlayerInput>, &mut PlayerInput)>) {
    for (action, mut input) in &mut players {
        *input = action.0;
    }
}

/// Marks the player this machine controls. A system, not an observer: on a
/// client, an interpolated `Player` appears some time after `Controlled`.
fn mark_local_player(
    players: Query<(Entity, &Player), (With<Controlled>, Without<LocalPlayer>)>,
    mut commands: Commands,
) {
    for (entity, player) in &players {
        // Input makes the player simulated here (predicted, on a client).
        commands
            .entity(entity)
            .insert((LocalPlayer, InputMarker::<PlayerInput>::default()))
            .insert_if_new((PlayerInput::default(), PreviousMode(player.mode)));
    }
}

fn write_local_input(
    local: Res<LocalInput>,
    mut actions: Query<&mut ActionState<PlayerInput>, With<InputMarker<PlayerInput>>>,
) {
    for mut action in &mut actions {
        action.0 = local.0;
    }
}

/// The local player's mode before this frame's rollback, if any, so the
/// camera can be handed the difference.
#[derive(Resource, Default)]
struct ViewBeforeRollback {
    mode: Option<Mode>,
    rollbacks: u32,
}

fn note_view_before_rollback(
    player: Query<&Player, With<LocalPlayer>>,
    mut before: ResMut<ViewBeforeRollback>,
) {
    before.mode = player.single().ok().map(|p| p.mode);
}

/// After a rollback, the view the re-simulation arrived at differs from the
/// one shown; the camera blends from one to the other.
fn hand_rollback_to_camera(
    metrics: Res<PredictionMetrics>,
    mut before: ResMut<ViewBeforeRollback>,
    player: Query<&Player, With<LocalPlayer>>,
    bodies: Bodies,
    tuning: Res<Tuning>,
    // Absent in a headless client (tests).
    correction: Option<ResMut<crate::client::ViewCorrection>>,
) {
    if metrics.rollbacks == before.rollbacks {
        return;
    }
    let Some(mut correction) = correction else {
        before.rollbacks = metrics.rollbacks;
        return;
    };
    before.rollbacks = metrics.rollbacks;
    let (Some(old), Ok(new)) = (before.mode, player.single()) else {
        return;
    };
    let view = |mode: Mode| {
        let frame = mode.body().and_then(|b| bodies.frame(b));
        mode.world_view(&tuning, frame.as_ref())
    };
    let ((old_eye, old_rot), (new_eye, new_rot)) = (view(old), view(new.mode));
    correction.position += old_eye - new_eye;
    correction.rotation = (correction.rotation * old_rot * new_rot.inverse()).normalize();
}

/// The sim counts ticks in lightyear's timeline.
fn follow_network_tick(timeline: Res<LocalTimeline>, mut tick: ResMut<SimTick>) {
    tick.0 = timeline.tick().0;
}

/// Lightyear's interpolation writes the local player's state in `Update`;
/// presenting it in the same frame needs that done first.
fn order_presentation(app: &mut App) {
    app.configure_sets(
        Update,
        crate::client::Present
            .after(lightyear::prelude::InterpolationSystems::Interpolate),
    );
}
