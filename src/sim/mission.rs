//! Scenarios: what the world is set up for, and how it ends.
//!
//! The crew's ship has a wall of buttons, one per scenario. Pressing one
//! tears the world down (ships, rocks, debris) and builds the scenario's
//! world, with everyone back aboard on the deck. The game starts in the
//! asteroid field, which is also where the wall's "Asteroid field" button
//! returns to.
//!
//! The authority runs all of this; clients see the result through
//! replication (the world itself, and the [`Mission`] status).

use avian3d::prelude::*;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::asteroids::{self, Asteroid};
use super::bodies::{Bodies, PreviousVelocity, dynamic_mass};
use super::combat::Combatant;
use super::events::PlayerEvent;
use super::flight::{Duel, Flight, Path};
use super::bots::{self, Robot};
use super::player::{DeckSlot, Mode, Player, PreviousMode};
use super::ship::{self, ControlAction, HomeShip, HullKind, ShipCommand, ShipHull};
use super::tuning::Tuning;
use super::{SimMaterial, SimTick, TICK_HZ};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScenarioKind {
    /// Drifting with the asteroid stream. Where the game starts.
    #[default]
    Field,
    /// Under attack: cross to the enemy ship and drop its shield.
    Boarding,
    /// Every model in the game set out behind our ship, for review.
    Gallery,
}

impl ScenarioKind {
    /// In the order of the buttons on the mission wall.
    pub const ALL: [ScenarioKind; 3] = [ScenarioKind::Field, ScenarioKind::Boarding, ScenarioKind::Gallery];

    pub fn button_label(self) -> &'static str {
        match self {
            ScenarioKind::Field => "Asteroid field",
            ScenarioKind::Boarding => "Boarding action",
            ScenarioKind::Gallery => "Model gallery",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    #[default]
    Underway,
    Won,
    Lost,
}

/// How the last mission went, shown to the crew when they're back in the
/// field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Debrief {
    pub scenario: ScenarioKind,
    pub phase: Phase,
    /// From the start to the win or loss.
    pub seconds: f32,
    /// Who dropped the shield.
    pub shield_breaker: Option<String>,
    /// Who was standing on the crew's ship when it was over, and who wasn't.
    pub aboard: Vec<String>,
    pub adrift: Vec<String>,
}

/// The scenario under way, replicated to everyone for the HUD.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mission {
    pub kind: ScenarioKind,
    pub phase: Phase,
    pub started: u32,
    /// The tick it was won or lost on.
    pub ended: Option<u32>,
    pub shield_breaker: Option<String>,
    /// The mission before this one, if it ended.
    pub debrief: Option<Debrief>,
}

/// Wreckage: a box of hull, flung off a destroyed ship. Its body is built
/// from this wherever it appears (see [`shape_debris`]), so replicating this
/// is replicating the piece.
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Debris {
    pub size: Vec3,
    /// Painted like the enemy's hull rather than ours.
    pub hostile: bool,
}

/// Wreckage is light, as hull plate goes, so a crew can shove it around.
const DEBRIS_DENSITY: f32 = 150.0;

pub fn shape_debris(add: On<Add, Debris>, pieces: Query<&Debris>, mut commands: Commands) {
    let piece = pieces.get(add.entity).expect("just added");
    let collider = Collider::cuboid(piece.size.x, piece.size.y, piece.size.z);
    commands.entity(add.entity).insert((
        dynamic_mass(&collider, DEBRIS_DENSITY),
        collider,
        // Pieces start out overlapping; they fly through each other (and
        // everything else) rather than burst apart. Players still stand on
        // and grapple them.
        CollisionLayers::new(super::Layer::World, LayerMask::NONE),
        if piece.hostile {
            SimMaterial::HostileHull
        } else {
            SimMaterial::Hull
        },
    ));
}

/// Authority: what to do next, decided by this tick's button presses and
/// battle results.
#[derive(Resource, Default)]
pub struct Director {
    /// A scenario to switch to at the end of this tick.
    next: Option<ScenarioKind>,
    /// Whether the field's rocks are part of the field scenario (tests turn
    /// them off for a quiet ship).
    pub asteroids: bool,
    /// Whether enemy ships carry robots (tests of other things turn them off).
    pub robots: bool,
}

impl Director {
    /// Switches to `kind` at the end of this tick.
    pub fn start(&mut self, kind: ScenarioKind) {
        self.next = Some(kind);
    }

    pub fn with_asteroids(asteroids: bool) -> Self {
        Self {
            next: Some(ScenarioKind::Field),
            asteroids,
            robots: true,
        }
    }
}

/// Authority: acts on the controls players pressed this tick.
pub fn handle_presses(
    tick: Res<SimTick>,
    players: Query<(&Player, Option<&Name>)>,
    mut director: ResMut<Director>,
    mut mission: Single<&mut Mission>,
    mut ships: Query<&mut Combatant>,
    mut orders: MessageWriter<ShipCommand>,
) {
    for (player, name) in &players {
        for (at, event) in player.events.iter() {
            let PlayerEvent::Pressed { body, action } = event else {
                continue;
            };
            if at != tick.0 {
                continue;
            }
            match action {
                ControlAction::Scenario(kind) => director.next = Some(kind),
                ControlAction::Lurch => {
                    orders.write(ShipCommand::Lurch);
                }
                // The player's own simulation armed them already.
                ControlAction::Armory => {}
                ControlAction::DisableShields => {
                    if let Ok(mut ship) = ships.get_mut(body)
                        && ship.shield
                    {
                        ship.shield = false;
                        mission.shield_breaker = name.map(|n| n.to_string());
                    }
                }
            }
        }
    }
}

/// Authority: wrecks any ship worn down to nothing, and decides the
/// mission.
pub fn settle_battle(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    bodies: Bodies,
    mut commands: Commands,
    mut director: ResMut<Director>,
    mut mission: Single<&mut Mission>,
    mut ships: Query<(Entity, &mut Combatant, &ShipHull, Has<HomeShip>, Option<&mut Flight>)>,
    mut players: Query<(Entity, &mut Player, Has<Robot>)>,
) {
    let wrecked: Vec<(Entity, HullKind, bool)> = ships
        .iter()
        .filter(|(_, c, ..)| c.integrity <= 0.0)
        .map(|(e, _, hull, home, _)| (e, hull.0, home))
        .collect();
    for (ship, kind, home) in wrecked {
        let Some(frame) = bodies.frame(ship) else {
            continue;
        };
        info!("{kind:?} ship destroyed");
        wreck(&mut commands, ship, kind, &frame, tick.0);
        for (entity, mut player, robot) in &mut players {
            // Robots aboard go down with their ship.
            if robot {
                if player.mode.body() == Some(ship) {
                    commands.entity(entity).despawn();
                }
                continue;
            }
            player.thrown_off(ship, &frame, &tuning);
        }
        // Nobody shoots at wreckage, and it can't shake anyone.
        for (_, mut combatant, _, _, flight) in &mut ships {
            if combatant.target == Some(ship) {
                combatant.target = None;
                if let Some(mut flight) = flight {
                    flight.shaken = false;
                }
            }
        }
        if mission.phase == Phase::Underway {
            mission.phase = if home { Phase::Lost } else { Phase::Won };
            mission.ended = Some(tick.0);
        }
    }
    // After a loss, the crew drifts a while, then it's back to the field.
    if mission.phase == Phase::Lost
        && let Some(ended) = mission.ended
        && tick.0.wrapping_sub(ended) as f32 >= tuning.wreck_return_seconds * TICK_HZ as f32
    {
        director.next = Some(ScenarioKind::Field);
    }
}

/// Breaks a ship into tumbling pieces of its hull.
fn wreck(commands: &mut Commands, ship: Entity, kind: HullKind, frame: &super::frame::BodyFrame, tick: u32) {
    let mut rng = ChaCha8Rng::seed_from_u64(tick as u64);
    for (center, size, turn, material) in ship::hull_pieces(kind) {
        if material == SimMaterial::Trim && size.max_element() < 1.5 {
            continue;
        }
        let at = frame.point_to_world(center);
        let out = (at - frame.center_of_mass).normalize_or(Vec3::Y);
        let scatter = Vec3::new(
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
        );
        let spin = Vec3::new(
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
        ) * rng.random_range(0.2..1.2);
        commands.spawn((
            Name::new("Debris"),
            Debris {
                size,
                hostile: kind == HullKind::Carrier,
            },
            RigidBody::Dynamic,
            SleepingDisabled,
            PreviousVelocity::default(),
            LinearVelocity(frame.point_velocity(at) + out * rng.random_range(3.0..9.0) + scatter * 1.5),
            AngularVelocity(spin),
            Position(at),
            Rotation(frame.rotation * turn),
        ));
    }
    commands.entity(ship).despawn();
}

/// Authority: at the end of a tick that asked for one, replaces the world
/// with a new scenario's and puts every player back aboard.
pub fn switch_scenario(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut director: ResMut<Director>,
    mut commands: Commands,
    old_world: Query<
        Entity,
        Or<(
            With<ShipHull>,
            With<Asteroid>,
            With<Debris>,
            With<GalleryAnchor>,
            With<Robot>,
            With<super::pickups::Pickup>,
        )>,
    >,
    homes: Query<(), With<HomeShip>>,
    mut mission: Single<&mut Mission>,
    mut players: Query<
        (&mut Player, Option<&DeckSlot>, Option<&mut PreviousMode>, Option<&Name>),
        Without<Robot>,
    >,
    mut shots: ResMut<super::shots::Shots>,
) {
    let Some(kind) = director.next.take() else {
        return;
    };
    // Bullets in flight belonged to the old world.
    shots.0.clear();
    info!("starting scenario {kind:?}");
    let debrief = (mission.phase != Phase::Underway).then(|| {
        let ended = mission.ended.unwrap_or(tick.0);
        let mut debrief = Debrief {
            scenario: mission.kind,
            phase: mission.phase,
            seconds: ended.wrapping_sub(mission.started) as f32 / TICK_HZ as f32,
            shield_breaker: mission.shield_breaker.clone(),
            ..default()
        };
        for (player, _, _, name) in &players {
            let name = name.map_or("?".to_string(), |n| n.to_string());
            if player.mode.body().is_some_and(|b| homes.contains(b)) {
                debrief.aboard.push(name);
            } else {
                debrief.adrift.push(name);
            }
        }
        debrief
    });
    for entity in &old_world {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<asteroids::FieldRng>();
    let home = match kind {
        ScenarioKind::Field => {
            let home = commands.spawn(ship::drifting_home_ship()).id();
            if director.asteroids {
                asteroids::spawn_field(&mut commands, &tuning);
            }
            home
        }
        ScenarioKind::Boarding => spawn_duel(&mut commands, &tuning, tick.0, director.robots),
        ScenarioKind::Gallery => {
            commands.spawn((
                Name::new("Gallery"),
                GalleryAnchor,
                Transform::from_translation(GALLERY_AT),
            ));
            commands.spawn(ship::drifting_home_ship()).id()
        }
    };
    **mission = Mission {
        kind,
        phase: Phase::Underway,
        started: tick.0,
        ended: None,
        shield_breaker: None,
        debrief,
    };
    for (index, (mut player, slot, previous, _)) in players.iter_mut().enumerate() {
        let slot = slot.map_or(index as u32, |s| s.0);
        player.respawn(home, slot);
        if kind == ScenarioKind::Gallery {
            to_gallery(&mut player, slot);
        }
        if let Some(mut previous) = previous {
            previous.0 = player.mode;
        }
    }
}

/// Where the model gallery starts: a row behind our ship's stern, centred
/// on its cargo door, at standing eye height.
pub const GALLERY_AT: Vec3 = Vec3::new(0.0, 1.5, 24.0);

/// Puts a player in front of the gallery's row, facing it (−Z), floating
/// beside the others.
fn to_gallery(player: &mut Player, slot: u32) {
    player.mode = Mode::Eva {
        position: GALLERY_AT + Vec3::new(slot as f32 * 1.5, 0.0, 7.0),
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    player.boots_on = false;
}

/// Authority: players who join during the gallery start at it too.
pub fn gallery_arrivals(
    mission: Single<&Mission>,
    mut players: Query<(&mut Player, &DeckSlot, Option<&mut PreviousMode>), Added<Player>>,
) {
    if mission.kind != ScenarioKind::Gallery {
        return;
    }
    for (mut player, slot, previous) in &mut players {
        to_gallery(&mut player, slot.0);
        if let Some(mut previous) = previous {
            previous.0 = player.mode;
        }
    }
}

/// Where the gallery's models are set out (by the client, which knows what
/// models there are).
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GalleryAnchor;

/// Rocks around a battle.
const BATTLE_ROCKS: usize = 320;
/// How far (m) a rock's surface keeps from where a ship's origin passes: a
/// ship's half-length and some.
const SHIP_CLEARANCE: f32 = 40.0;
/// The ships' paths are sampled every half second for ten minutes when
/// placing rocks.
const SAMPLE_TICKS: u32 = TICK_HZ as u32 / 2;
const BATTLE_SAMPLES: u32 = 1200;

/// Two ships in a duel: ours, and the carrier weaving in and out around it.
/// Returns ours.
fn spawn_duel(commands: &mut Commands, tuning: &Tuning, tick: u32, robots: bool) -> Entity {
    let duel = Duel {
        center: Vec3::ZERO,
        radius: 300.0,
        lap: 120.0,
        bob: 20.0,
        near: tuning.duel_near,
        far: tuning.duel_far,
        pass: tuning.carrier_pass_seconds,
        sweep: 240.0,
    };
    let (orbit, weave) = (
        Flight {
            start: tick,
            path: Path::Orbit(duel),
            shaken: true,
        },
        Flight {
            start: tick,
            path: Path::Weave(duel),
            shaken: true,
        },
    );
    // Rocks among the fight, for the crew to swing between, clear of
    // everywhere either ship goes for longer than any fight lasts.
    let swept: Vec<Vec3> = (0..BATTLE_SAMPLES)
        .flat_map(|i| {
            let at = tick + i * SAMPLE_TICKS;
            [orbit.pose(at, 0.0).0, weave.pose(at, 0.0).0]
        })
        .collect();
    let near: Vec<Vec3> = swept.iter().step_by(7).copied().collect();
    asteroids::spawn_battlefield(commands, &near, BATTLE_ROCKS, |at, radius| {
        let keep_out = radius + SHIP_CLEARANCE;
        swept.iter().all(|p| p.distance_squared(at) > keep_out * keep_out)
    });
    let flying = |flight: Flight| {
        let frame = flight.frame(tick, Vec3::ZERO);
        (
            flight,
            RigidBody::Static,
            PreviousVelocity::default(),
            Position(frame.position),
            Rotation(frame.rotation),
            LinearVelocity(frame.linear_velocity),
            AngularVelocity(frame.angular_velocity),
        )
    };
    let home = commands
        .spawn((
            Name::new("Ship"),
            HomeShip,
            ShipHull(HullKind::Home),
            flying(orbit),
        ))
        .id();
    let carrier = commands
        .spawn((
            Name::new("Carrier"),
            ShipHull(HullKind::Carrier),
            flying(weave),
            Combatant {
                target: Some(home),
                integrity: 1.0,
                shield: true,
                shielded_seconds: tuning.enemy_shielded_seconds,
                exposed_seconds: tuning.enemy_exposed_seconds,
            },
        ))
        .id();
    commands.entity(home).insert(Combatant {
        target: Some(carrier),
        integrity: 1.0,
        shield: true,
        shielded_seconds: tuning.home_survival_seconds,
        exposed_seconds: tuning.home_survival_seconds,
    });
    if robots {
        crew_the_carrier(commands, tuning, carrier, &weave, tick);
    }
    home
}

/// Gunners posted along the way from the carrier's hangar door to its
/// bridge, facing aft (toward boarders), and Swarmers flying escort.
fn crew_the_carrier(commands: &mut Commands, tuning: &Tuning, carrier: Entity, flight: &Flight, tick: u32) {
    use ship::carrier::UPPER;
    let aft = Quat::from_rotation_y(std::f32::consts::PI);
    let posts = [
        // The hangar.
        Vec3::new(-2.5, 0.0, 19.0),
        Vec3::new(3.5, 0.0, 17.0),
        // The lower corridor.
        Vec3::new(0.0, 0.0, -2.0),
        // The upper port room, inside the ramp's door.
        Vec3::new(-4.0, UPPER, 8.0),
        // The upper corridor.
        Vec3::new(0.0, UPPER, -10.0),
        // The bridge.
        Vec3::new(-2.5, UPPER, -25.0),
        Vec3::new(2.5, UPPER, -25.0),
    ];
    for (i, post) in posts.into_iter().enumerate() {
        let feet = post + Vec3::Y * 0.01;
        let mode = Mode::Booted {
            body: carrier,
            feet,
            up: Vec3::Y,
            velocity: Vec3::ZERO,
            orientation: aft,
            pitch: 0.0,
        };
        commands.spawn(bots::new_robot(Robot::Gunner, mode, carrier, feet, tuning, i as u32));
    }
    let frame = flight.frame(tick, Vec3::ZERO);
    for (i, post) in [
        Vec3::new(18.0, 6.0, -20.0),
        Vec3::new(-18.0, 6.0, -20.0),
        Vec3::new(18.0, -4.0, 5.0),
        Vec3::new(-18.0, -4.0, 5.0),
        Vec3::new(10.0, 4.0, 40.0),
        Vec3::new(-10.0, 4.0, 40.0),
    ]
    .into_iter()
    .enumerate()
    {
        let position = frame.point_to_world(post);
        let mode = Mode::Eva {
            position,
            velocity: frame.point_velocity(position),
            orientation: frame.rotation,
            pitch: 0.0,
        };
        commands.spawn(bots::new_robot(Robot::Swarmer, mode, carrier, post, tuning, 10 + i as u32));
    }
}

/// Authority: the replicated mission status.
pub fn spawn_mission(mut commands: Commands) {
    commands.spawn((Name::new("Mission"), Mission::default()));
}
