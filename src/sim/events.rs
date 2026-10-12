//! Discrete things that happen to a player, recorded as tick-stamped state.
//!
//! Events are part of player state rather than fire-and-forget messages. A
//! tick that is re-simulated (rollback) rewrites its own entries instead of
//! appending duplicates, and anything that can see a player's state, whether
//! predicted, replicated, or local, can see what happened to it. Presentation
//! (sound, HUD, camera effects) reads the log with its own cursor; the
//! simulation never knows who is listening.
//!
//! What only the authority decides (a death, the respawn after it, being
//! blown off a ship) isn't predicted, so it isn't in the predicted log: it
//! goes in the player's [`AuthorityLog`], which is replicated as-is and read
//! by count, so each entry is presented once however late it arrives.

use std::collections::VecDeque;

use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::items::Item;
use super::ship::ControlAction;
use super::weapons::{GunKind, Hand, ShotKind};

/// How long entries stay in the log, in ticks. Readers must look at least
/// this often.
pub const LOG_TICKS: u32 = 120;

/// Why the boots let go.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Detach {
    Jumped,
    BootsOff,
    /// The surface accelerated away harder than adhesion (m/s²).
    RippedOff(f32),
    /// The rope out-pulled the boots (N).
    Yanked(f32),
    /// Blown off the deck: the ship underfoot came apart, or a blast or a
    /// shot beat the boots. Authority only: see [`AuthorityLog`].
    Blasted,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum PlayerEvent {
    Detached(Detach),
    /// Boots caught a surface; relative speed in m/s.
    Landed {
        speed: f32,
        body: Entity,
    },
    /// Bounced off something at this closing speed (m/s).
    Impact {
        speed: f32,
        body: Entity,
    },
    /// Climbed onto a wall or wrapped around an edge.
    SurfaceChanged {
        body: Entity,
    },
    /// Stepped from one body onto another.
    BodyChanged {
        body: Entity,
    },
    GrappleAttached {
        distance: f32,
        body: Entity,
    },
    GrappleMissed,
    GrappleReleased,
    RopeSnapped,
    BootsToggled(bool),
    AssistToggled(bool),
    ReferenceSet {
        distance: f32,
    },
    NoReferenceTarget,
    /// Pressed a button on a ship.
    Pressed {
        body: Entity,
        action: ControlAction,
    },
    /// Put back aboard (after a death, or at the start of a scenario).
    /// Authority only: see [`AuthorityLog`].
    Respawned,
    /// Shoved by a laser beam.
    BeamHit,
    /// Ran out of health (a respawn follows). Authority only: see
    /// [`AuthorityLog`].
    Died,
    /// Took from a pickup: the items whose bits are set in `taken` (by
    /// their index in it), and some of up to two boxes of ammo (their
    /// index, and the rounds left in them).
    PickedUp {
        pickup: Entity,
        taken: u32,
        partly: [Option<(u8, u16)>; 2],
    },
    /// Couldn't take anything from a pickup: no room for it.
    InventoryFull,
    /// Tried to take what someone else left where they died.
    NotYours,
    /// Put something down (the authority leaves it as a pickup).
    Dropped(Item),
    /// A laser's beam came on in `hand` (it's held state from here: see
    /// [`super::weapons::Beam`]).
    BeamStarted {
        hand: Hand,
        gun: GunKind,
    },
    /// The beam in `hand` went off (let go, overheated, put away).
    BeamStopped {
        hand: Hand,
    },
    /// A shot left a gun (a `gun` frame): from `origin` (the eye), at
    /// `velocity`, to do `damage` and push what it hits with a bullet of
    /// `mass`.
    Fired {
        hand: Hand,
        gun: GunKind,
        damage: f32,
        mass: f32,
        origin: Vec3,
        /// A flown shot's velocity; an instant one's direction (times its
        /// gun's muzzle speed).
        velocity: Vec3,
        kind: ShotKind,
    },
}

impl PlayerEvent {
    /// Whether the player's view jumped (rather than moved continuously) at
    /// this event: the capsule was relocated onto another surface, or into
    /// another frame the render interpolation can't follow it across.
    /// Stepping between bodies changes frame but not pose, so it isn't one.
    pub fn relocates_view(&self) -> bool {
        matches!(
            self,
            PlayerEvent::Landed { .. }
                | PlayerEvent::SurfaceChanged { .. }
                | PlayerEvent::Detached(_)
                | PlayerEvent::Respawned
        )
    }
}

impl PlayerEvent {
    /// A line for the HUD, if this event deserves one.
    pub fn caption(&self) -> Option<String> {
        Some(match *self {
            PlayerEvent::Detached(Detach::Jumped) => "jumped".into(),
            PlayerEvent::Detached(Detach::BootsOff) => "boots off".into(),
            PlayerEvent::Detached(Detach::RippedOff(a)) => format!("ripped off ({a:.1} m/s²)"),
            PlayerEvent::Detached(Detach::Yanked(n)) => format!("yanked off by rope ({n:.0} N)"),
            PlayerEvent::Detached(Detach::Blasted) => "blown clear".into(),
            PlayerEvent::Landed { speed, .. } => format!("landed at {speed:.1} m/s"),
            PlayerEvent::Impact { speed, .. } => format!("bounced at {speed:.1} m/s"),
            PlayerEvent::SurfaceChanged { .. } => return None,
            PlayerEvent::BodyChanged { .. } => "stepped onto another body".into(),
            PlayerEvent::GrappleAttached { distance, .. } => format!("grappled at {distance:.1} m"),
            PlayerEvent::GrappleMissed => "grapple missed".into(),
            PlayerEvent::GrappleReleased => "grapple released".into(),
            PlayerEvent::RopeSnapped => "rope snapped".into(),
            PlayerEvent::BootsToggled(on) => format!("boots {}", if on { "on" } else { "off" }),
            PlayerEvent::AssistToggled(on) => {
                format!("flight assist {}", if on { "on" } else { "off" })
            }
            PlayerEvent::ReferenceSet { distance } => format!("reference set at {distance:.0} m"),
            PlayerEvent::NoReferenceTarget => "no body under crosshair".into(),
            PlayerEvent::Pressed { action, .. } => format!("pressed: {}", action.label()),
            PlayerEvent::Respawned => return None,
            PlayerEvent::BeamHit => "caught in a laser beam".into(),
            PlayerEvent::Died => "killed".into(),
            PlayerEvent::Fired { .. } | PlayerEvent::BeamStarted { .. } | PlayerEvent::BeamStopped { .. } => return None,
            PlayerEvent::PickedUp { .. } => "picked up".into(),
            PlayerEvent::InventoryFull => "no room: drop something first".into(),
            PlayerEvent::NotYours => "not yours: a crewmate's kit".into(),
            PlayerEvent::Dropped(item) => format!("dropped {}", item.name()),
        })
    }
}

impl MapEntities for PlayerEvent {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        match self {
            PlayerEvent::Landed { body, .. }
            | PlayerEvent::Impact { body, .. }
            | PlayerEvent::SurfaceChanged { body }
            | PlayerEvent::BodyChanged { body }
            | PlayerEvent::GrappleAttached { body, .. }
            | PlayerEvent::Pressed { body, .. } => *body = mapper.get_mapped(*body),
            PlayerEvent::PickedUp { pickup, .. } => *pickup = mapper.get_mapped(*pickup),
            PlayerEvent::Detached(_)
            | PlayerEvent::GrappleMissed
            | PlayerEvent::GrappleReleased
            | PlayerEvent::RopeSnapped
            | PlayerEvent::BootsToggled(_)
            | PlayerEvent::AssistToggled(_)
            | PlayerEvent::ReferenceSet { .. }
            | PlayerEvent::NoReferenceTarget
            | PlayerEvent::Respawned
            | PlayerEvent::BeamHit
            | PlayerEvent::Died
            | PlayerEvent::InventoryFull
            | PlayerEvent::NotYours
            | PlayerEvent::Dropped(_)
            | PlayerEvent::Fired { .. }
            | PlayerEvent::BeamStarted { .. }
            | PlayerEvent::BeamStopped { .. } => {}
        }
    }
}

impl MapEntities for EventLog {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        for (_, event) in &mut self.entries {
            event.map_entities(mapper);
        }
    }
}

impl MapEntities for AuthorityLog {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        for (_, event) in &mut self.recent {
            event.map_entities(mapper);
        }
    }
}

/// What the authority alone decides happens to a player, newest last, each
/// stamped with the (authority's) tick: [`PlayerEvent::Died`],
/// [`PlayerEvent::Respawned`], [`Detach::Blasted`], and a rope snapped by a
/// ship coming apart. A client can't predict these, so in the predicted
/// [`EventLog`] it would learn of them only by a correction: late (past what
/// presentation will show) or not at all (a respawn on the spot you stood
/// on corrects nothing). The authority writes this one; it's replicated, not
/// predicted, and a reader keeps its own count to see new entries.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[component(map_entities)]
pub struct AuthorityLog {
    /// Entries written so far.
    pub count: u32,
    recent: VecDeque<(u32, PlayerEvent)>,
}

/// How many entries an [`AuthorityLog`] keeps.
const AUTHORITY_LOG: usize = 16;

impl AuthorityLog {
    pub fn push(&mut self, tick: u32, event: PlayerEvent) {
        self.count = self.count.wrapping_add(1);
        self.recent.push_back((tick, event));
        while self.recent.len() > AUTHORITY_LOG {
            self.recent.pop_front();
        }
    }

    /// Entries since a reader last saw `count` of them, oldest first.
    pub fn since(&self, count: u32) -> impl Iterator<Item = (u32, PlayerEvent)> + '_ {
        let new = (self.count.wrapping_sub(count) as usize).min(self.recent.len());
        self.recent.iter().skip(self.recent.len() - new).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, PlayerEvent)> + '_ {
        self.recent.iter().copied()
    }
}

/// The recent events of one player, oldest first, each stamped with the tick
/// it happened on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EventLog {
    tick: u32,
    entries: VecDeque<(u32, PlayerEvent)>,
}

impl EventLog {
    /// Starts recording `tick`: forgets old entries and any recorded for
    /// this tick or later (a re-simulated tick replaces its events).
    pub fn begin_tick(&mut self, tick: u32) {
        self.tick = tick;
        self.entries
            .retain(|(t, _)| (1..=LOG_TICKS).contains(&tick.wrapping_sub(*t)));
    }

    /// The tick being (or last) recorded.
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// A copy holding only the entries of the last `ticks` ticks.
    pub fn since(&self, ticks: u32) -> EventLog {
        EventLog {
            tick: self.tick,
            entries: self.entries.iter().filter(|(t, _)| self.tick.wrapping_sub(*t) < ticks).copied().collect(),
        }
    }

    pub fn push(&mut self, event: PlayerEvent) {
        self.entries.push_back((self.tick, event));
    }

    /// The most recent entry, if any.
    pub fn last(&self) -> Option<(u32, PlayerEvent)> {
        self.entries.back().copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u32, PlayerEvent)> + '_ {
        self.entries.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resimulated_tick_replaces_its_events() {
        let mut log = EventLog::default();
        log.begin_tick(10);
        log.push(PlayerEvent::GrappleMissed);
        log.begin_tick(11);
        log.push(PlayerEvent::RopeSnapped);
        // Roll back to 11 and re-simulate it differently.
        log.begin_tick(11);
        log.push(PlayerEvent::GrappleReleased);
        let all: Vec<_> = log.iter().collect();
        assert_eq!(
            all,
            vec![
                (10, PlayerEvent::GrappleMissed),
                (11, PlayerEvent::GrappleReleased)
            ]
        );
    }

    #[test]
    fn authority_entries_are_read_once_however_many_arrive() {
        let mut log = AuthorityLog::default();
        log.push(5, PlayerEvent::Died);
        log.push(5, PlayerEvent::Respawned);
        let seen = log.count;
        assert_eq!(log.since(0).count(), 2);
        assert_eq!(log.since(seen).count(), 0);
        for t in 0..40 {
            log.push(t, PlayerEvent::RopeSnapped);
        }
        // A reader that fell far behind sees what's kept.
        assert_eq!(log.since(seen).count(), AUTHORITY_LOG);
        assert_eq!(log.since(log.count - 1).collect::<Vec<_>>(), vec![(39, PlayerEvent::RopeSnapped)]);
    }

    #[test]
    fn old_entries_expire() {
        let mut log = EventLog::default();
        log.begin_tick(1);
        log.push(PlayerEvent::GrappleMissed);
        log.begin_tick(1 + LOG_TICKS + 1);
        assert_eq!(log.iter().count(), 0);
    }
}
