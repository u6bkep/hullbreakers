//! Scenarios: what the world is set up for, and how it ends.
//!
//! The crew's ship has a wall of buttons, one per scenario. Pressing one
//! tears the world down (ships, rocks, debris) and builds the scenario's
//! world, with everyone back aboard on the deck. The game starts in the
//! asteroid field, which is also where the wall's "Asteroid field" button
//! returns to.
//!
//! This module is what every scenario shares: which one is on
//! ([`Mission`], replicated for the HUD), the switch between them, the crew's
//! tally and the debrief. Each scenario is its own module under
//! [`super::scenarios`], with a common shape: it builds its world, judges
//! it (its own systems, run only while it's on), and writes its objective,
//! status, markers and report into the [`Mission`].
//!
//! The authority runs all of this; clients see the result through
//! replication (the world itself, and the [`Mission`] status).

use std::collections::BTreeMap;

use avian3d::prelude::*;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::asteroids::{self, Asteroid};
use super::bodies::{PreviousVelocity, dynamic_mass};
use super::bots::Robot;
use super::design::ShipDesign;
use super::events::{AuthorityLog, PlayerEvent};
use super::frame::BodyFrame;
use super::player::{DeckSlot, Mode, Player, PreviousMode};
use super::scenarios::{self, Stage};
use super::ship::{ControlAction, HomeShip, ShipCommand, ShipHull};
use super::tuning::Tuning;
use super::{SimMaterial, SimTick, TICK_HZ};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScenarioKind {
    /// Drifting with the asteroid stream. Where the game starts.
    #[default]
    Field,
    /// Under attack: cross to the enemy ship and drop its shield.
    Boarding,
    /// A race round a loop of rings through a rock field, against the clock.
    GrappleRun,
    /// A dark derelict: bring its power back, open its vault, get out.
    DeadShip,
    /// A wreck's crew adrift on the clock: find them by beacon, tow them home.
    Recovery,
    /// A power cell from a robot-held freighter, carried home under escort.
    Escort,
    /// Every model in the game set out behind our ship, for review.
    Gallery,
}

impl ScenarioKind {
    /// In the order of the buttons on the mission wall.
    pub const ALL: [ScenarioKind; 7] = [
        ScenarioKind::Field,
        ScenarioKind::Boarding,
        ScenarioKind::GrappleRun,
        ScenarioKind::DeadShip,
        ScenarioKind::Recovery,
        ScenarioKind::Escort,
        ScenarioKind::Gallery,
    ];

    pub fn button_label(self) -> &'static str {
        scenarios::scenario(self).label
    }

    /// What `HULLBREAKERS_SCENARIO` calls it.
    pub fn id(self) -> &'static str {
        scenarios::scenario(self).id
    }

    pub fn from_id(id: &str) -> Option<ScenarioKind> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    #[default]
    Underway,
    Won,
    Lost,
}

/// One crew member's part in a mission: the score card.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CrewCard {
    pub name: String,
    /// Standing on the crew's ship when the crew left the scenario.
    pub aboard: bool,
    /// Time spent in EVA.
    pub vacuum_seconds: f32,
    /// The farthest they travelled on one rope, in EVA (m).
    pub longest_swing: f32,
    /// Buttons and devices pressed.
    pub presses: u32,
    pub deaths: u32,
    /// What they did that the scenario thought worth a mention ("dropped
    /// the shield", "turned the port key").
    pub credits: Vec<String>,
}

/// How the last mission went, shown to the crew when they're back in the
/// field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Debrief {
    pub scenario: ScenarioKind,
    pub phase: Phase,
    /// From the start to the win or loss.
    pub seconds: f32,
    /// The banner it ended with ("CARRIER DESTROYED").
    pub outcome: String,
    /// The scenario's report, then who made it home.
    pub lines: Vec<String>,
    /// The report as numbers (see [`Mission::result`]).
    pub results: Vec<(String, f32)>,
    /// Everyone's score card, by name.
    pub crew: Vec<CrewCard>,
    /// The best of the score cards ("Longest swing: Player 2, 84 m").
    pub awards: Vec<String>,
    /// What it paid each crew member (see `sim::progression`).
    pub rewards: Vec<super::progression::Reward>,
}

/// What a marker points at, for the HUD to style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkerKind {
    /// The thing the mission is about.
    Goal,
    /// A place to go through (a ring).
    Waypoint,
    /// Something to work (a lever, a key, a junction, a board to read).
    Device,
    /// Where to go to finish: home.
    Exit,
}

/// A body's name for what refers to it across the network: given by the
/// authority when the body appears, replicated with it, never reused. A
/// marker names its body by this rather than by entity, so a client that
/// hasn't received the body yet finds no body (and draws nothing) instead
/// of a wrong one: an entity reference would arrive unmapped and could
/// alias anything.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BodyId(pub u32);

/// Authority: the next [`BodyId`].
#[derive(Resource, Default)]
pub struct NextBodyId(u32);

/// Authority: names every new body of a kind that markers can be on
/// (ships and sites, pods).
pub fn name_body<C: Component>(add: On<Add, C>, mut next: ResMut<NextBodyId>, mut commands: Commands) {
    commands.entity(add.entity).insert(BodyId(next.0));
    next.0 += 1;
}

/// Finds the bodies markers are on, wherever the markers are read.
#[derive(SystemParam)]
pub struct MarkedBodies<'w, 's> {
    bodies: Query<'w, 's, (Entity, &'static BodyId)>,
}

impl MarkedBodies<'_, '_> {
    /// The body called `id` here, if it's arrived.
    pub fn entity(&self, id: BodyId) -> Option<Entity> {
        self.bodies.iter().find(|(_, b)| **b == id).map(|(e, _)| e)
    }

    /// The name of `entity`, if it has one yet.
    pub fn id(&self, entity: Entity) -> Option<BodyId> {
        self.bodies.get(entity).ok().map(|(_, b)| *b)
    }
}

/// A place the HUD can point the crew to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    pub label: String,
    pub kind: MarkerKind,
    /// The body it's on (`at` in its coordinates), or `None` for a point in
    /// the world. Find it with [`MarkedBodies::entity`]; a marker whose body
    /// isn't there (yet) isn't anywhere.
    pub body: Option<BodyId>,
    pub at: Vec3,
    /// Only this player's marker, by their [`DeckSlot`] (`None`:
    /// everyone's).
    pub player: Option<u32>,
    /// The device it is, if it's one (an index into `body`'s design's
    /// devices), so it can be drawn as the thing to go to.
    pub device: Option<u16>,
}

impl Marker {
    /// A marker at `at` on `body`.
    pub fn on(body: BodyId, at: Vec3, label: &str, kind: MarkerKind) -> Marker {
        Marker {
            label: label.to_string(),
            kind,
            body: Some(body),
            at,
            player: None,
            device: None,
        }
    }

    /// A marker at a point in the world.
    pub fn at(at: Vec3, label: &str, kind: MarkerKind) -> Marker {
        Marker {
            body: None,
            ..Marker::on(BodyId(0), at, label, kind)
        }
    }

    /// A marker on a device of a ship.
    pub fn device(ship: BodyId, design: &ShipDesign, id: &str, kind: MarkerKind) -> Marker {
        let i = design.device(id);
        let device = &design.devices[i];
        Marker {
            device: Some(i as u16),
            ..Marker::on(ship, device.at, &device.label, kind)
        }
    }

    /// Only for the player at deck slot `slot`.
    pub fn for_player(self, slot: u32) -> Marker {
        Marker {
            player: Some(slot),
            ..self
        }
    }

    /// Where it is, given its body's frame (`None` for a marker in the
    /// world). `None` if it's on a body whose frame isn't known.
    pub fn position(&self, frame: Option<&BodyFrame>) -> Option<Vec3> {
        match (self.body, frame) {
            (None, _) => Some(self.at),
            (Some(_), Some(frame)) => Some(frame.point_to_world(self.at)),
            (Some(_), None) => None,
        }
    }
}

/// The scenario under way, replicated to everyone for the HUD.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mission {
    pub kind: ScenarioKind,
    pub phase: Phase,
    pub started: u32,
    /// The tick it was won or lost on.
    pub ended: Option<u32>,
    /// What to do now, in a line.
    pub objective: String,
    /// The scenario's own board: a few lines of live status (gates passed,
    /// circuits live, a countdown).
    pub status: Vec<String>,
    /// Where the things to do are.
    pub markers: Vec<Marker>,
    /// The banner once it's decided ("CARRIER DESTROYED").
    pub outcome: String,
    /// Lines for the debrief, written as things happen.
    pub report: Vec<String>,
    /// The same, as numbers, for whatever pays out on them (see
    /// [`Mission::result`]).
    pub results: Vec<(String, f32)>,
    /// The mission before this one, if it ended.
    pub debrief: Option<Debrief>,
    /// What it paid, once it's over (for the debrief).
    pub rewards: Vec<super::progression::Reward>,
}

impl Mission {
    /// Records a result, by name (`"gates"`, `"pods_rescued"`, ...),
    /// replacing one of the same name. Every scenario records `"won"` (1 or
    /// 0) and `"seconds"` when it's decided; see each scenario for the rest.
    pub fn result(&mut self, name: &str, value: f32) {
        match self.results.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => *v = value,
            None => self.results.push((name.to_string(), value)),
        }
    }

    /// The result called `name`, if there is one.
    pub fn result_of(&self, name: &str) -> Option<f32> {
        self.results.iter().find(|(n, _)| n == name).map(|(_, v)| *v)
    }

    /// Decides it, at `tick`, unless it already is.
    pub fn decide(&mut self, phase: Phase, tick: u32, outcome: &str) {
        if self.phase == Phase::Underway {
            self.phase = phase;
            self.ended = Some(tick);
            self.outcome = outcome.to_string();
            self.result("won", if phase == Phase::Won { 1.0 } else { 0.0 });
            self.result("seconds", self.elapsed(tick));
        }
    }

    /// Seconds since it started, at `tick`.
    pub fn elapsed(&self, tick: u32) -> f32 {
        tick.wrapping_sub(self.started) as f32 / TICK_HZ as f32
    }

    /// Sets what the HUD shows, touching the (replicated) mission only if
    /// something changed.
    pub fn show(mission: &mut Mut<Mission>, objective: String, status: Vec<String>, markers: Vec<Marker>) {
        if mission.objective != objective {
            mission.objective = objective;
        }
        if mission.status != status {
            mission.status = status;
        }
        if mission.markers != markers {
            mission.markers = markers;
        }
    }
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

/// Something a scenario put in the world besides ships, rocks, robots and
/// debris (its own state, a gallery's anchor): cleared away with the world
/// when the scenario changes.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Staged;

/// Authority: what to do next, decided by this tick's button presses and
/// battle results.
#[derive(Resource, Default)]
pub struct Director {
    /// A scenario to switch to at the end of this tick.
    next: Option<ScenarioKind>,
    /// Whether the field's rocks are part of the field scenario (tests turn
    /// them off for a quiet ship).
    pub asteroids: bool,
    /// Whether enemy ships carry robots, and the field has fauna (tests of
    /// other things turn them off).
    pub robots: bool,
}

impl Director {
    /// Switches to `kind` at the end of this tick.
    pub fn start(&mut self, kind: ScenarioKind) {
        self.next = Some(kind);
    }

    /// The scenario the world switches to at the end of this tick, if it does.
    pub fn switching(&self) -> Option<ScenarioKind> {
        self.next
    }

    pub fn with_asteroids(asteroids: bool) -> Self {
        Self {
            next: Some(ScenarioKind::Field),
            asteroids,
            robots: true,
        }
    }
}

/// The parts of the authority's mission tick, in order.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum MissionSystems {
    /// Damage and deaths are settled.
    Settle,
    /// Presses are acted on and devices settle; the tally is kept.
    React,
    /// The scenario on judges how it's going (see [`super::scenarios`]).
    Judge,
    /// A switch, if one was asked for, and arrivals.
    Switch,
}

/// This tick's presses of `action`s, by whom: (player, body, action).
pub fn presses<'a>(
    tick: u32,
    players: impl IntoIterator<Item = (Entity, &'a Player)>,
) -> impl Iterator<Item = (Entity, Entity, ControlAction)> {
    players.into_iter().flat_map(move |(who, player)| {
        player.events.iter().filter_map(move |(at, event)| match event {
            PlayerEvent::Pressed { body, action } if at == tick => Some((who, body, action)),
            _ => None,
        })
    })
}

/// Authority: acts on the controls every scenario shares (the mission wall,
/// the lurch). Scenarios act on their own (see [`presses`]); devices on
/// theirs (see [`super::devices`]).
pub fn handle_presses(
    tick: Res<SimTick>,
    players: Query<(Entity, &Player)>,
    mut director: ResMut<Director>,
    mut orders: MessageWriter<ShipCommand>,
) {
    for (_, _, action) in presses(tick.0, &players) {
        match action {
            ControlAction::Scenario(kind) => director.next = Some(kind),
            ControlAction::Lurch => {
                orders.write(ShipCommand::Lurch);
            }
            // The player's own simulation armed them already; the rest are
            // the scenario's and the devices'.
            ControlAction::Armory | ControlAction::Workbench | ControlAction::DisableShields | ControlAction::Device(_) => {}
        }
    }
}

/// One crew member's running tally, for the score card.
#[derive(Clone, Debug, Default)]
pub struct CrewStats {
    pub vacuum_ticks: u32,
    /// The rope travel of the swing under way, and where they were last
    /// tick on it.
    swing: f32,
    last: Option<Vec3>,
    pub longest_swing: f32,
    pub presses: u32,
    pub deaths: u32,
    pub credits: Vec<String>,
}

/// Authority: every crew member's tally this mission.
#[derive(Resource, Default)]
pub struct CrewTally(pub BTreeMap<Entity, CrewStats>);

impl CrewTally {
    /// Credits `player` with something they did (once per mission).
    pub fn credit(&mut self, player: Entity, what: impl Into<String>) {
        let what = what.into();
        let credits = &mut self.0.entry(player).or_default().credits;
        if !credits.contains(&what) {
            credits.push(what);
        }
    }
}

/// Authority: keeps the crew's tally (time in vacuum, swings, presses,
/// deaths).
pub fn tally_crew(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    players: Query<(Entity, &Player, &AuthorityLog), Without<Robot>>,
    mut tally: ResMut<CrewTally>,
) {
    for (entity, player, told) in &players {
        let stats = tally.0.entry(entity).or_default();
        let eva = matches!(player.mode, Mode::Eva { .. });
        if eva {
            stats.vacuum_ticks += 1;
        }
        if eva && player.grapple.is_some() {
            let (at, _) = player.mode.world_kinematics(&tuning, None);
            if let Some(last) = stats.last {
                stats.swing += at.distance(last);
                stats.longest_swing = stats.longest_swing.max(stats.swing);
            }
            stats.last = Some(at);
        } else {
            stats.swing = 0.0;
            stats.last = None;
        }
        for (at, event) in player.events.iter() {
            if at != tick.0 {
                continue;
            }
            if let PlayerEvent::Pressed { .. } = event {
                stats.presses += 1;
            }
        }
        stats.deaths += told.iter().filter(|&(at, event)| at == tick.0 && event == PlayerEvent::Died).count() as u32;
    }
}

/// Authority: once a scenario is decided, its crew goes back to the field
/// after a while, if the scenario says so (otherwise they choose, at the
/// mission wall).
pub fn return_when_decided(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mission: Single<&Mission>,
    mut director: ResMut<Director>,
) {
    let Some(ended) = mission.ended else { return };
    let Some(after) = (scenarios::scenario(mission.kind).return_after)(mission.phase, &tuning) else {
        return;
    };
    if tick.0.wrapping_sub(ended) as f32 >= after * TICK_HZ as f32 && director.next.is_none() {
        director.next = Some(ScenarioKind::Field);
    }
}

/// Breaks a ship into tumbling pieces of its hull. Whoever is standing on
/// it is thrown clear; robots aboard go down with it.
pub fn break_up<'a>(
    commands: &mut Commands,
    ship: Entity,
    design: &ShipDesign,
    frame: &BodyFrame,
    tick: u32,
    tuning: &Tuning,
    players: impl IntoIterator<Item = (Entity, Mut<'a, Player>, bool)>,
    told: &mut Query<&mut AuthorityLog>,
) {
    info!("{} destroyed", design.id);
    for (entity, mut player, robot) in players {
        if robot {
            if player.mode.body() == Some(ship) {
                commands.entity(entity).despawn();
            }
            continue;
        }
        if let Ok(mut told) = told.get_mut(entity) {
            player.thrown_off(ship, frame, tuning, &mut told, tick);
        }
    }
    let mut rng = ChaCha8Rng::seed_from_u64(tick as u64);
    for b in &design.boxes {
        let (center, size, turn, material) = (b.center, b.size, b.rotation, b.material);
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
                hostile: design.hostile,
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

/// Who was on which ship, for the debrief: a player standing on the home
/// ship is aboard.
fn aboard(player: &Player, homes: &Query<(), With<HomeShip>>) -> bool {
    player.mode.body().is_some_and(|b| homes.contains(b))
}

/// The debrief of a decided mission: its report, the crew's cards and the
/// awards.
fn debrief(
    mission: &Mission,
    tick: u32,
    tally: &CrewTally,
    crew: impl Iterator<Item = (Entity, String, bool)>,
) -> Debrief {
    let ended = mission.ended.unwrap_or(tick);
    let mut cards: Vec<CrewCard> = crew
        .map(|(entity, name, aboard)| {
            let stats = tally.0.get(&entity).cloned().unwrap_or_default();
            CrewCard {
                name,
                aboard,
                vacuum_seconds: stats.vacuum_ticks as f32 / TICK_HZ as f32,
                longest_swing: stats.longest_swing,
                presses: stats.presses,
                deaths: stats.deaths,
                credits: stats.credits,
            }
        })
        .collect();
    cards.sort_by(|a, b| a.name.cmp(&b.name));
    let mut lines = mission.report.clone();
    let names = |aboard: bool| cards.iter().filter(|c| c.aboard == aboard).map(|c| c.name.as_str()).collect::<Vec<_>>();
    let (home, adrift) = (names(true), names(false));
    if scenarios::scenario(mission.kind).homecoming {
        if !home.is_empty() {
            lines.push(format!("Made it home: {}", home.join(", ")));
        }
        if !adrift.is_empty() {
            lines.push(format!("Left drifting: {}. Send a postcard.", adrift.join(", ")));
        }
    }
    let mut awards = Vec::new();
    let best = |key: &dyn Fn(&CrewCard) -> f32| {
        cards
            .iter()
            .filter(|c| key(c) > 0.0)
            .max_by(|a, b| key(a).total_cmp(&key(b)))
    };
    if let Some(c) = best(&|c| c.longest_swing) {
        awards.push(format!("Longest swing: {}, {:.0} m on one rope", c.name, c.longest_swing));
    }
    if let Some(c) = best(&|c| c.vacuum_seconds) {
        awards.push(format!("Most time in vacuum: {}, {}", c.name, clock(c.vacuum_seconds)));
    }
    if let Some(c) = best(&|c| c.presses as f32) {
        awards.push(format!("Busiest hands: {}, {} presses", c.name, c.presses));
    }
    if let Some(c) = best(&|c| c.deaths as f32) {
        awards.push(format!("Most deaths: {}, {}", c.name, c.deaths));
    }
    Debrief {
        scenario: mission.kind,
        phase: mission.phase,
        seconds: ended.wrapping_sub(mission.started) as f32 / TICK_HZ as f32,
        outcome: mission.outcome.clone(),
        lines,
        results: mission.results.clone(),
        crew: cards,
        awards,
        rewards: mission.rewards.clone(),
    }
}

/// A time as minutes and seconds.
pub fn clock(seconds: f32) -> String {
    // Rounded to tenths first, so 59.96 s is 1:00.0, not 0:60.0.
    let tenths = (seconds.max(0.0) * 10.0).round() as u32;
    format!("{}:{:02}.{}", tenths / 600, tenths % 600 / 10, tenths % 10)
}

#[cfg(test)]
#[test]
fn clocks_round_before_they_carry() {
    assert_eq!(clock(59.96), "1:00.0");
    assert_eq!(clock(72.34), "1:12.3");
    assert_eq!(countdown(59.2), "1:00");
    assert_eq!(countdown(0.0), "0:00");
}

/// A countdown in whole seconds, rounded up (so it reads 0:00 only at the
/// end).
pub fn countdown(seconds: f32) -> String {
    let s = seconds.max(0.0).ceil() as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Authority: at the end of a tick that asked for one, replaces the world
/// with a new scenario's and puts every player back aboard.
#[allow(clippy::too_many_arguments)]
pub fn switch_scenario(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    mut director: ResMut<Director>,
    mut tally: ResMut<CrewTally>,
    mut commands: Commands,
    old_world: Query<
        Entity,
        Or<(
            With<ShipHull>,
            With<Asteroid>,
            With<Debris>,
            With<Staged>,
            With<Robot>,
            With<super::pickups::Pickup>,
        )>,
    >,
    homes: Query<(), With<HomeShip>>,
    mut mission: Single<&mut Mission>,
    mut players: Query<
        (Entity, &mut Player, Option<&DeckSlot>, Option<&mut PreviousMode>, Option<&Name>),
        Without<Robot>,
    >,
    mut told: Query<&mut AuthorityLog>,
    mut shots: ResMut<super::shots::Shots>,
    mut damage: ResMut<Messages<super::vitals::Damage>>,
) {
    let Some(kind) = director.next.take() else {
        return;
    };
    // Bullets in flight belonged to the old world, and so does damage not
    // yet dealt (a blast, a trampling, written after this tick's damage).
    shots.0.clear();
    damage.clear();
    info!("starting scenario {kind:?}");
    let debrief = (mission.phase != Phase::Underway).then(|| {
        let crew = players.iter().map(|(entity, player, _, _, name)| {
            (entity, name.map_or("?".to_string(), |n| n.to_string()), aboard(player, &homes))
        });
        debrief(&mission, tick.0, &tally, crew)
    });
    tally.0.clear();
    for entity in &old_world {
        commands.entity(entity).despawn();
    }
    commands.remove_resource::<asteroids::FieldRng>();
    commands.remove_resource::<super::fauna::FaunaField>();
    let scenario = scenarios::scenario(kind);
    let home = (scenario.build)(&mut Stage {
        commands: &mut commands,
        tuning: &tuning,
        tick: tick.0,
        asteroids: director.asteroids,
        robots: director.robots,
        players: players.iter().count().max(1),
    });
    **mission = Mission {
        kind,
        started: tick.0,
        debrief,
        ..default()
    };
    for (index, (entity, mut player, slot, previous, _)) in players.iter_mut().enumerate() {
        let slot = slot.map_or(index as u32, |s| s.0);
        let Ok(mut told) = told.get_mut(entity) else { continue };
        player.respawn(home, slot, &mut told, tick.0);
        if let Some(arrive) = scenario.arrive {
            arrive(&mut player, slot);
        }
        if let Some(mut previous) = previous {
            previous.0 = player.mode;
        }
    }
}

/// Authority: players who join during a scenario that starts its crew
/// somewhere other than the deck start there too.
pub fn arrivals(
    mission: Single<&Mission>,
    mut players: Query<(&mut Player, &DeckSlot, Option<&mut PreviousMode>), (Added<Player>, Without<Robot>)>,
) {
    let Some(arrive) = scenarios::scenario(mission.kind).arrive else {
        return;
    };
    for (mut player, slot, previous) in &mut players {
        arrive(&mut player, slot.0);
        if let Some(mut previous) = previous {
            previous.0 = player.mode;
        }
    }
}

/// Authority: a player who leaves the game (or is otherwise taken out of
/// it) holding mission cargo leaves it where they were, for anyone.
pub fn drop_cargo_on_leaving(
    remove: On<Remove, Player>,
    tick: Res<SimTick>,
    players: Query<&Player>,
    mut commands: Commands,
) {
    let Ok(player) = players.get(remove.entity) else { return };
    let inventory = &player.loadout.inventory;
    let cargo: Vec<super::items::Item> = inventory
        .carrying
        .map(super::items::Item::Cargo)
        .into_iter()
        .chain(inventory.pack.iter().flatten().copied().filter(|i| matches!(i, super::items::Item::Cargo(_))))
        .collect();
    if !cargo.is_empty() {
        commands.spawn((
            Name::new("Dropped cargo"),
            super::pickups::Pickup::dropped(super::pickups::left_where(&player.mode), tick.0, cargo),
        ));
    }
}

/// Authority: the replicated mission status.
pub fn spawn_mission(mut commands: Commands) {
    commands.spawn((Name::new("Mission"), Mission::default()));
}
