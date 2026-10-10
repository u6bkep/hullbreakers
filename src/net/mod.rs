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

use crate::sim::asteroids::Asteroid;
use crate::sim::input::PlayerInput;
use crate::sim::player::{Mode, Player, PlayerTelemetry, new_player};
use crate::sim::ship::{Ship, ShipHull};
use crate::sim::tuning::Tuning;
use crate::sim::motion::Motion;
use crate::sim::{SimPlugin, SimSharedPlugin, SimSystems, SimTick, TICK_HZ};

mod extrapolate;

pub const DEFAULT_PORT: u16 = 5888;
const PROTOCOL_ID: u64 = 0x5ace_9a3e;
/// Shared netcode key. Direct-IP play between friends for now; a relay or
/// matchmaking service would hand out connect tokens instead.
const PRIVATE_KEY: [u8; 32] = [0; 32];
/// How often the server sends world state.
const SEND_INTERVAL: Duration = Duration::from_millis(50);
/// The in-process client of a listen-server host.
const HOST_PEER: PeerId = PeerId::Local(0);

/// The player this machine presents and controls.
#[derive(Component)]
pub struct LocalPlayer;

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

pub struct NetPlugin {
    pub mode: NetMode,
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        let mode = self.mode;
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
        app.add_plugins(ProtocolPlugin);
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
            .add_observer(replicate_body::<Ship>)
            .add_observer(replicate_body::<Asteroid>)
            .add_systems(Startup, spawn_host_tuning)
            .add_systems(Update, (publish_tuning, spawn_players))
            .add_systems(
                FixedUpdate,
                apply_network_input.before(SimSystems::RecordVelocities),
            )
            .add_systems(
                FixedPostUpdate,
                extrapolate::track_motion.after(SimSystems::Players),
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
            ))
            .add_observer(extrapolate::prepare_extrapolated)
            .add_systems(Update, adopt_host_tuning)
            .init_resource::<extrapolate::DrawClock>()
            .add_systems(
                FixedUpdate,
                extrapolate::follow_motion.run_if(extrapolate::synced),
            )
            .add_systems(
                PostUpdate,
                (extrapolate::advance_draw_clock, extrapolate::draw_extrapolated)
                    .chain()
                    .in_set(crate::client::BodyPoses),
            );
        }
        if mode.plays() {
            // Required by lightyear's client input pipeline (input history
            // for rollback), even before anything is predicted.
            app.init_resource::<LocalInput>()
                .insert_resource(PredictionManager::default())
                .add_systems(PreUpdate, mark_local_player)
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
                app.add_systems(Startup, move |mut commands: Commands| {
                    connect(&mut commands, server);
                });
            }
        }
    }
}

struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(InputPlugin::<PlayerInput>::default());
        app.component::<Name>().replicate();
        app.component::<ShipHull>().replicate_once();
        app.component::<Asteroid>().replicate_once();
        app.component::<Motion>().replicate();
        app.component::<HostTuning>().replicate();
        app.component::<PlayerTelemetry>().replicate();
        app.component::<Player>()
            .replicate()
            .add_interpolation_with(interpolate_player);
    }
}

/// Moves a player smoothly between two received states. Within one mode
/// (and one body) positions and orientations blend; across a change of mode
/// or body the state switches at the halfway point. Everything else (flags,
/// grapple, the event log) comes from the nearer state.
fn interpolate_player(start: Player, end: Player, t: f32) -> Player {
    let mode = match (start.mode, end.mode) {
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
        _ if t < 0.5 => start.mode,
        _ => end.mode,
    };
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
                ..default()
            }),
            LocalAddr(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)),
            ServerUdpIo::default(),
        ))
        .id();
    commands.trigger(Start { entity: server });
    server
}

fn connect(commands: &mut Commands, server: SocketAddr) {
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
            Link::default(),
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

/// Players are replicated to every client, and every client but the host's
/// own interpolates them.
fn to_clients() -> (Replicate, InterpolationTarget) {
    (
        Replicate::to_clients(NetworkTarget::All),
        InterpolationTarget::to_clients(NetworkTarget::AllExceptSingle(HOST_PEER)),
    )
}

fn on_link(add: On<Add, LinkOf>, mut commands: Commands) {
    commands.entity(add.entity).insert(ReplicationSender);
}

/// The player a connected client controls.
#[derive(Component)]
struct HasPlayer;

/// Which deck spot a player spawned on; freed when they leave.
#[derive(Component)]
struct Slot(u32);

/// Gives every connected client without a player one, on the ship's deck.
/// A system rather than a `Connected` observer: the host's own client
/// connects during startup, before the ship exists.
fn spawn_players(
    clients: Query<Entity, (With<ClientOf>, With<Connected>, Without<HasPlayer>)>,
    ship: Query<Entity, With<Ship>>,
    slots: Query<&Slot>,
    mut commands: Commands,
) {
    let Ok(ship) = ship.single() else { return };
    let mut taken: Vec<u32> = slots.iter().map(|s| s.0).collect();
    for client in &clients {
        let index = (0..).find(|i| !taken.contains(i)).expect("unbounded");
        taken.push(index);
        info!("client {client:?} joined as player {}", index + 1);
        commands.entity(client).insert(HasPlayer);
        commands.spawn((
            new_player(ship, index),
            Slot(index),
            ActionState::<PlayerInput>::default(),
            to_clients(),
            ControlledBy {
                owner: client,
                lifetime: Default::default(),
            },
        ));
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
    players: Query<Entity, (With<Controlled>, With<Player>, Without<LocalPlayer>)>,
    mut commands: Commands,
) {
    for entity in &players {
        commands
            .entity(entity)
            .insert((LocalPlayer, InputMarker::<PlayerInput>::default()));
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
