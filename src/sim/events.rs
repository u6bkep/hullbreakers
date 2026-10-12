//! Discrete things that happen to a player, recorded as tick-stamped state.
//!
//! Events are part of player state rather than fire-and-forget messages. A
//! tick that is re-simulated (rollback) rewrites its own entries instead of
//! appending duplicates, and anything that can see a player's state, whether
//! predicted, replicated, or local, can see what happened to it. Presentation
//! (sound, HUD, camera effects) reads the log with its own cursor; the
//! simulation never knows who is listening.

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
    /// The ship underfoot came apart.
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
    /// Put back aboard at the start of a scenario.
    Respawned,
    /// Shoved by a laser beam.
    BeamHit,
    /// Ran out of health (a respawn follows).
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

    /// Whether the two hold the same entries (whatever tick each is on).
    pub fn same_entries(&self, other: &EventLog) -> bool {
        self.entries == other.entries
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
    fn old_entries_expire() {
        let mut log = EventLog::default();
        log.begin_tick(1);
        log.push(PlayerEvent::GrappleMissed);
        log.begin_tick(1 + LOG_TICKS + 1);
        assert_eq!(log.iter().count(), 0);
    }
}
