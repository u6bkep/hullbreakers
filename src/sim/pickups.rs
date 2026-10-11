//! Things lying about to pick up: for now, what a dead player was carrying.
//!
//! A pickup isn't a physics body. It's fixed to the body it was left on (a
//! deck), or, left in open space, drifts on at the velocity it was dropped
//! with, so every peer knows where it is from its replicated description
//! and the bodies. A player picks one up by looking at it within reach and
//! pressing interact; that's part of their (predicted) simulation, and the
//! authority then removes it.

use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bodies::Bodies;
use super::events::PlayerEvent;
use super::player::Player;
use super::weapons::Gun;
use super::{SimTick, TICK_HZ};

#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[component(map_entities)]
pub struct Pickup {
    /// The body it's lying on, `at` in its coordinates; or `None`, adrift:
    /// at `at` in the world at tick `since`, moving at `velocity`.
    pub body: Option<Entity>,
    pub at: Vec3,
    pub velocity: Vec3,
    pub since: u32,
    pub guns: Vec<Gun>,
}

impl MapEntities for Pickup {
    fn map_entities<M: EntityMapper>(&mut self, mapper: &mut M) {
        if let Some(body) = &mut self.body {
            *body = mapper.get_mapped(*body);
        }
    }
}

/// Half the size of a pickup's case (m): what a player looks at to take it.
pub const PICKUP_HALF: Vec3 = Vec3::new(0.3, 0.2, 0.3);

impl Pickup {
    /// Where it is at `tick` (plus a fraction of one), and how it's turned,
    /// given where its body is (`frame`: position and rotation).
    pub fn pose(&self, frame: Option<(Vec3, Quat)>, tick: u32, fraction: f32) -> Option<(Vec3, Quat)> {
        match (self.body, frame) {
            (Some(_), Some((position, rotation))) => Some((position + rotation * self.at, rotation)),
            (Some(_), None) => None,
            (None, _) => {
                let t = (tick.wrapping_sub(self.since) as i32 as f32 + fraction) / TICK_HZ as f32;
                Some((self.at + self.velocity * t, Quat::IDENTITY))
            }
        }
    }

    /// Where it is now, in the simulation.
    pub fn sim_pose(&self, bodies: &Bodies, tick: u32) -> Option<(Vec3, Quat)> {
        let frame = self.body.and_then(|b| bodies.frame(b)).map(|f| (f.position, f.rotation));
        self.pose(frame, tick, 0.0)
    }
}

/// Authority: pickups taken this tick are gone. (The taker already has what
/// was in it.)
pub fn remove_taken(
    tick: Res<SimTick>,
    players: Query<&Player>,
    pickups: Query<(), With<Pickup>>,
    mut commands: Commands,
) {
    for player in &players {
        for (at, event) in player.events.iter() {
            if let PlayerEvent::PickedUp { pickup } = event
                && at == tick.0
                && pickups.contains(pickup)
            {
                commands.entity(pickup).despawn();
            }
        }
    }
}
