//! Presentation's view of player events.
//!
//! The simulation records events in each player's [`EventLog`]; this reads the
//! local player's log with its own cursor and hands each new event to
//! presentation (sound, camera, HUD) exactly once, as a [`Presented`] message.
//! Re-simulated ticks rewrite the log rather than appending to it, so nothing
//! is presented twice.
//!
//! [`EventLog`]: crate::sim::events::EventLog

use bevy::prelude::*;

use super::LocalPlayer;
use crate::sim::events::PlayerEvent;
use crate::sim::player::Player;

/// A player event, delivered to presentation once.
#[derive(Message, Clone, Copy, Debug)]
pub struct Presented(pub PlayerEvent);

/// How far presentation has read the local player's log, and the last
/// event worth showing as text.
#[derive(Resource, Default)]
pub struct EventCursor {
    through: Option<u32>,
    pub last_text: String,
}

pub fn present_events(
    player: Single<&Player, With<LocalPlayer>>,
    mut cursor: ResMut<EventCursor>,
    mut presented: MessageWriter<Presented>,
) {
    let new: Vec<(u32, PlayerEvent)> = match cursor.through {
        Some(through) => player.events.since(through).collect(),
        None => player.events.iter().collect(),
    };
    // Events a re-simulation adds at ticks already shown are dropped; once
    // there is prediction, choosing per event whether to play those late is
    // presentation policy that belongs here.
    for &(_, event) in &new {
        if let Some(text) = event.caption() {
            cursor.last_text = text;
        }
        presented.write(Presented(event));
    }
    if let Some(&(tick, _)) = new.last() {
        cursor.through = Some(tick);
    }
}
