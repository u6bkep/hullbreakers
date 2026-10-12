//! Headless simulation: everything that would run on a server.
//!
//! Nothing here touches rendering, windows, or input devices. Input arrives as
//! [`input::PlayerInput`] components; the client layer reads state back out,
//! including each player's tick-stamped [`events::EventLog`].

pub mod asteroids;
pub mod bodies;
pub mod bots;
pub mod combat;
pub mod design;
pub mod devices;
pub mod events;
pub mod fauna;
pub mod flight;
pub mod frame;
pub mod input;
pub mod items;
pub mod mission;
pub mod motion;
pub mod nav;
pub mod pickups;
pub mod player;
pub mod progression;
pub mod rope;
pub mod scenarios;
pub mod ship;
pub mod shots;
pub mod tuning;
pub mod vitals;
pub mod weapons;

use avian3d::prelude::*;
use bevy::prelude::*;

pub const TICK_HZ: f64 = 60.0;

/// The number of the simulation tick being run. Everything the sim does is a
/// function of state, input, and this, so re-running a tick reproduces it.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimTick(pub u32);

pub fn advance_tick(mut tick: ResMut<SimTick>) {
    tick.0 = tick.0.wrapping_add(1);
}

/// Collision layers. Players' capsules are only for aiming at (grappling a
/// teammate); world geometry is everything else.
#[derive(PhysicsLayer, Clone, Copy, Debug, Default)]
pub enum Layer {
    #[default]
    World,
    Players,
}

/// Visual material class of a collider. The sim only tags; the client decides
/// what it looks like.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SimMaterial {
    Hull,
    /// The enemy's hull paint.
    HostileHull,
    Trim,
    Rock,
}

/// Fixed-tick ordering within `FixedUpdate`, before the physics step.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SimSystems {
    /// Snapshot velocities so frame accelerations can be measured across
    /// everything that changes them this tick (scripted drive and the solver).
    RecordVelocities,
    /// Script-driven bodies set their velocities and the field is maintained.
    Drive,
    /// Beams are traced, in `FixedPostUpdate` after the physics step.
    Beams,
    /// Players are simulated, after the beams.
    Players,
    /// The authority reacts to what players did and what the battle did.
    Mission,
}

/// What every peer needs to make sense of the world, authoritative or not:
/// tuning, and the observers that build bodies (hulls, rocks) from their
/// generating components.
pub struct SimSharedPlugin;

impl Plugin for SimSharedPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<tuning::Tuning>() {
            let tuning = tuning::Tuning::load_or_default(std::path::Path::new(tuning::TUNING_PATH));
            app.insert_resource(tuning);
        }
        app.add_message::<ship::ShipCommand>()
            .add_message::<bots::SpawnRobots>()
            .add_message::<pickups::SpawnLoot>()
            .init_resource::<nav::NavGrids>()
            .init_resource::<SimTick>()
            .add_observer(player::add_player_hull)
            .add_observer(bots::add_body_tuning)
            .add_observer(bots::add_affix_tuning)
            .add_observer(ship::build_hull)
            .add_observer(asteroids::shape_asteroid)
            .add_observer(fauna::shape_grazer)
            .add_observer(mission::shape_debris)
            .add_observer(scenarios::recovery::shape_pod)
            // How its devices look here (a client's own presses at once).
            .add_systems(Update, devices::show_devices)
            // A ship's closed doors are part of its collider, wherever it is.
            .add_systems(
                FixedPostUpdate,
                devices::fit_doors
                    .in_set(SimSystems::Mission)
                    .after(mission::MissionSystems::React),
            );
    }
}

/// Player simulation: every player that has [`input::PlayerInput`] here is
/// simulated here. The authority simulates all of them; a network client
/// simulates (predicts) only its own.
pub struct PlayerSimPlugin;

impl Plugin for PlayerSimPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            FixedUpdate,
            (SimSystems::RecordVelocities, SimSystems::Drive).chain(),
        )
        .configure_sets(
            FixedPostUpdate,
            (SimSystems::Beams, SimSystems::Players, SimSystems::Mission)
                .chain()
                .after(PhysicsSystems::Last),
        )
        .init_resource::<combat::Beams>()
        .add_systems(
            FixedUpdate,
            (
                bodies::record_previous_velocity.in_set(SimSystems::RecordVelocities),
                // Scripted ships are where their flight says, on every peer.
                flight::fly.in_set(SimSystems::Drive),
            ),
        )
        .add_systems(
            FixedPostUpdate,
            (
                combat::update_beams.in_set(SimSystems::Beams),
                player::player_tick.in_set(SimSystems::Players),
                progression::tune_to_rank.before(SimSystems::Players),
                player::pose_player_hulls.after(SimSystems::Players),
            ),
        );
    }
}

/// The authoritative simulation.
pub struct SimPlugin {
    /// Spawn the asteroid field. Tests turn it off for a quiet ship.
    pub asteroids: bool,
    /// Spawn a single local player at startup. A server spawns players as
    /// they connect instead.
    pub local_player: bool,
}

impl Default for SimPlugin {
    fn default() -> Self {
        Self {
            asteroids: true,
            local_player: true,
        }
    }
}

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((SimSharedPlugin, PlayerSimPlugin));
        let in_field = resource_exists::<asteroids::FieldRng>;
        app.add_message::<vitals::Damage>()
            .add_message::<bots::Detonation>()
            .init_resource::<shots::Shots>()
            .init_resource::<shots::Shoves>()
            .init_resource::<asteroids::FieldInterest>()
            .add_observer(pickups::set_adrift)
            .add_observer(pickups::note_owned)
            .add_observer(progression::leave_with_gear)
            .init_resource::<progression::ProfileOrders>()
            .init_resource::<mission::NextBodyId>()
            .add_observer(mission::name_body::<ship::ShipHull>)
            .add_observer(mission::name_body::<scenarios::recovery::Pod>)
            .insert_resource(mission::Director::with_asteroids(self.asteroids))
            .init_resource::<mission::CrewTally>()
            .add_systems(
                FixedUpdate,
                (
                    asteroids::apply_field_speed,
                    asteroids::recycle_field
                        .after(update_pins)
                        .after(update_field_interest),
                    update_field_interest,
                )
                    .run_if(in_field)
                    .in_set(SimSystems::Drive),
            )
            .add_systems(
                FixedUpdate,
                (
                    fauna::swim,
                    (fauna::populate_field, fauna::cull_fauna)
                        .after(asteroids::recycle_field)
                        .run_if(resource_exists::<fauna::FaunaField>.and_then(in_field)),
                )
                    .in_set(SimSystems::Drive),
            );
        // The first scenario is built at the end of the first tick; the
        // local player joins it then.
        app.add_systems(Startup, mission::spawn_mission);
        if self.local_player {
            app.add_systems(
                FixedPostUpdate,
                player::spawn_player
                    .after(SimSystems::Mission)
                    .run_if(run_once),
            );
        }
        // Bevy's default fixed clock is 64 Hz. A network layer must use the
        // same rate for its ticks.
        app.insert_resource(Time::<Fixed>::from_hz(TICK_HZ))
            .insert_resource(Gravity(Vec3::ZERO))
            .add_systems(FixedFirst, advance_tick)
            .add_systems(
                FixedUpdate,
                (ship::drive_ship, update_pins).in_set(SimSystems::Drive),
            )
            .add_systems(
                FixedPostUpdate,
                (bots::debug_spawn_robots, bots::robot_upkeep, bots::think, bots::limpet_bites)
                    .chain()
                    .after(SimSystems::Beams)
                    .before(SimSystems::Players),
            )
            .configure_sets(
                FixedPostUpdate,
                (
                    mission::MissionSystems::Settle,
                    mission::MissionSystems::React,
                    mission::MissionSystems::Judge,
                    mission::MissionSystems::Switch,
                )
                    .chain()
                    .in_set(SimSystems::Mission),
            )
            .add_systems(
                FixedPostUpdate,
                (
                    (
                        combat::apply_damage,
                        shots::launch_shots,
                        shots::fly_shots,
                        shots::apply_shoves,
                        vitals::beam_burns,
                        vitals::apply_damage,
                        fauna::alarm,
                        fauna::trample,
                        fauna::settle_grazer_deaths,
                        pickups::remove_taken,
                        pickups::spawn_drops,
                        pickups::debug_spawn_loot,
                        vitals::settle_deaths,
                        pickups::drop_robot_loot,
                        bots::settle_robot_deaths,
                        bots::detonate,
                    )
                        .chain()
                        .in_set(mission::MissionSystems::Settle),
                    (mission::handle_presses, devices::run_devices, mission::tally_crew, progression::apply_profile_orders, progression::open_slots)
                        .chain()
                        .in_set(mission::MissionSystems::React),
                    (
                        mission::return_when_decided,
                        progression::settle_missions,
                        progression::leave_cargo,
                        mission::switch_scenario,
                        mission::arrivals,
                        progression::snapshot_profiles,
                    )
                        .chain()
                        .in_set(mission::MissionSystems::Switch),
                ),
            );
        scenarios::add_scenarios(app);
    }
}

/// Keeps rocks the crew are standing on or grappled to from being recycled
/// (not robots': a limpet goes with its rock).
fn update_pins(
    mut commands: Commands,
    players: Query<&player::Player, Without<bots::Robot>>,
    pinned: Query<Entity, With<asteroids::Pinned>>,
    rocks: Query<(), With<asteroids::Asteroid>>,
) {
    let wanted: Vec<Entity> = players
        .iter()
        .flat_map(|p| [p.mode.body(), p.grapple.map(|g| g.target)])
        .flatten()
        .filter(|e| rocks.contains(*e))
        .collect();
    for e in &pinned {
        if !wanted.contains(&e) {
            commands.entity(e).remove::<asteroids::Pinned>();
        }
    }
    for e in wanted {
        if !pinned.contains(e) {
            commands.entity(e).insert(asteroids::Pinned);
        }
    }
}

/// The field is kept populated around the ship and every crew member.
fn update_field_interest(
    mut interest: ResMut<asteroids::FieldInterest>,
    ships: Query<&Position, With<ship::HomeShip>>,
    players: Query<&player::Player, Without<bots::Robot>>,
    bodies: bodies::Bodies,
    tuning: Res<tuning::Tuning>,
) {
    interest.0.clear();
    interest
        .0
        .extend(ships.iter().map(|p| asteroids::InterestPoint {
            position: p.0,
            corridor: true,
        }));
    for p in &players {
        let frame = p.mode.body().and_then(|b| bodies.frame(b));
        let (eye, _) = p.mode.world_view(&tuning, frame.as_ref());
        interest.0.push(asteroids::InterestPoint {
            position: eye,
            corridor: false,
        });
    }
}
