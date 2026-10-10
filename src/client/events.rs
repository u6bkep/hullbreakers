//! Presentation's view of player events.
//!
//! The simulation records events in each player's [`EventLog`]; this reads the
//! local player's log with its own cursor and hands each new event to
//! presentation (sound, camera, HUD) exactly once, as a [`Presented`] message.
//! Re-simulated ticks rewrite the log rather than appending to it, so nothing
//! is presented twice.
//!
//! [`EventLog`]: crate::sim::events::EventLog

use std::collections::VecDeque;

use bevy::prelude::*;

use super::LocalPlayer;
use crate::sim::events::{EventLog, LOG_TICKS, PlayerEvent};
use crate::sim::player::Player;

/// A player event, delivered to presentation once.
#[derive(Message, Clone, Copy, Debug)]
pub struct Presented(pub PlayerEvent);

/// Which of the local player's events have been presented, and the last
/// event worth showing as text.
///
/// Under prediction the log is rewritten when the server corrects it: an
/// event can appear at a tick already passed (the server saw a landing the
/// client missed), move by a tick, or vanish. Presentation shows each event
/// once: a new entry is presented unless the same kind of event was already
/// presented within a few ticks of it (the same event, re-timed), and only
/// if it isn't too stale to mean anything (a sound half a second late is
/// worse than none).
#[derive(Resource, Default)]
pub struct EventCursor {
    presented: VecDeque<(u32, PlayerEvent)>,
    pub last_text: String,
}

/// An event this many ticks either side of a presented one of the same kind
/// is taken to be the same event.
const SAME_EVENT_TICKS: u32 = 3;
/// Events that turn up later than this (after a correction) are dropped.
const LATE_TICKS: u32 = 15;

impl EventCursor {
    /// The events in `log` not yet presented, marking them presented.
    fn take_new(&mut self, log: &EventLog) -> Vec<PlayerEvent> {
        let now = log.tick();
        self.presented
            .retain(|(t, _)| now.wrapping_sub(*t) <= LOG_TICKS);
        let mut new = Vec::new();
        for (tick, event) in log.iter() {
            let kind = std::mem::discriminant(&event);
            let seen = self.presented.iter().any(|(t, e)| {
                std::mem::discriminant(e) == kind
                    && (tick.wrapping_sub(*t) as i32).unsigned_abs() <= SAME_EVENT_TICKS
            });
            if seen || now.wrapping_sub(tick) > LATE_TICKS {
                continue;
            }
            self.presented.push_back((tick, event));
            new.push(event);
        }
        new
    }
}

pub fn present_events(
    player: Single<&Player, With<LocalPlayer>>,
    mut cursor: ResMut<EventCursor>,
    mut presented: MessageWriter<Presented>,
) {
    for event in cursor.take_new(&player.events) {
        if let Some(text) = event.caption() {
            cursor.last_text = text;
        }
        presented.write(Presented(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_with(entries: &[(u32, PlayerEvent)], now: u32) -> EventLog {
        let mut log = EventLog::default();
        for &(tick, event) in entries {
            log.begin_tick(tick);
            log.push(event);
        }
        log.begin_tick(now);
        log
    }

    #[test]
    fn a_re_timed_event_is_presented_once() {
        let mut cursor = EventCursor::default();
        let jumped = PlayerEvent::Detached(crate::sim::events::Detach::Jumped);
        assert_eq!(cursor.take_new(&log_with(&[(100, jumped)], 101)).len(), 1);
        // A correction moves the same event a tick later.
        assert!(cursor.take_new(&log_with(&[(101, jumped)], 104)).is_empty());
    }

    #[test]
    fn a_missed_event_is_presented_late_but_not_stale() {
        let mut cursor = EventCursor::default();
        let missed = PlayerEvent::RopeSnapped;
        // The server's log has an event the prediction missed, a few ticks back.
        assert_eq!(cursor.take_new(&log_with(&[(95, missed)], 100)).len(), 1);
        // Too old to be worth showing.
        let mut fresh = EventCursor::default();
        assert!(fresh.take_new(&log_with(&[(50, missed)], 100)).is_empty());
    }
}
