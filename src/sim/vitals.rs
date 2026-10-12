//! Health: how much more a player can take, and dying.
//!
//! Only the authority hurts players. Vitals live apart from [`Player`]
//! (which a client predicts) because a client can't predict what will hurt
//! it; it learns its health from the authority, and a death reaches it as a
//! respawn like any other.

use std::collections::VecDeque;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::SimTick;
use super::events::PlayerEvent;
use super::bots::{BodyTuning, Robot, tuning_of};
use super::pickups::{Pickup, left_where};
use super::player::{DeckSlot, Player};
use super::ship::HomeShip;
use super::tuning::Tuning;

#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vitals {
    pub health: f32,
    /// Seconds since last hurt; health comes back after a while.
    pub since_hurt: f32,
}

impl Vitals {
    pub fn full(tuning: &Tuning) -> Vitals {
        Vitals {
            health: tuning.player_health,
            since_hurt: f32::INFINITY,
        }
    }
}

/// Something hurting something with [`Vitals`].
#[derive(Message, Clone, Copy, Debug)]
pub struct Damage {
    pub target: Entity,
    pub amount: f32,
    /// Who did it, to tell them they hit (see [`HitLog`]).
    pub source: Option<Entity>,
    /// Whether it was a weak spot.
    pub weak: bool,
}

/// What a player has recently hit, for their hit markers. The authority
/// writes it; it isn't predicted (a client can't know what its shots hit).
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HitLog {
    /// Hits counted so far; a reader keeps its own count to see new ones.
    pub count: u32,
    /// The most recent hits, newest last.
    pub recent: VecDeque<Hit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub weak: bool,
    /// The hit left the target with no health.
    pub kill: bool,
}

/// How many hits a log keeps.
const HIT_LOG: usize = 16;

impl HitLog {
    fn push(&mut self, hit: Hit) {
        self.count = self.count.wrapping_add(1);
        self.recent.push_back(hit);
        while self.recent.len() > HIT_LOG {
            self.recent.pop_front();
        }
    }

    /// Hits since a reader last saw `count` of them, oldest first.
    pub fn since(&self, count: u32) -> impl Iterator<Item = &Hit> {
        let new = (self.count.wrapping_sub(count) as usize).min(self.recent.len());
        self.recent.iter().skip(self.recent.len() - new)
    }
}

/// Authority: a laser beam a player was caught in this tick burns them.
pub fn beam_burns(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    players: Query<(Entity, &Player)>,
    mut damage: MessageWriter<Damage>,
) {
    for (entity, player) in &players {
        let hit = player
            .events
            .iter()
            .any(|(at, e)| at == tick.0 && e == PlayerEvent::BeamHit);
        if hit {
            damage.write(Damage {
                target: entity,
                amount: tuning.beam_damage,
                source: None,
                weak: false,
            });
        }
    }
}

/// Authority: applies this tick's damage, and heals whoever has gone long
/// enough unhurt.
pub fn apply_damage(
    time: Res<Time>,
    tuning: Res<Tuning>,
    mut damage: MessageReader<Damage>,
    mut vitals: Query<(&mut Vitals, Has<Robot>, Option<&BodyTuning>)>,
    mut logs: Query<&mut HitLog>,
) {
    let dt = time.delta_secs();
    // The crew's suits heal (to their own most: rank plates them); robots
    // don't.
    for (mut v, robot, own) in &mut vitals {
        v.since_hurt += dt;
        if !robot && v.since_hurt > tuning.health_regen_delay {
            v.health = (v.health + tuning.health_regen_rate * dt).min(tuning_of(&tuning, own).player_health);
        }
    }
    for hit in damage.read() {
        let Ok((mut v, ..)) = vitals.get_mut(hit.target) else {
            continue;
        };
        // Already dead this tick: no more hits to count.
        if v.health <= 0.0 {
            continue;
        }
        v.health = (v.health - hit.amount).max(0.0);
        v.since_hurt = 0.0;
        if let Some(mut log) = hit.source.and_then(|s| logs.get_mut(s).ok()) {
            log.push(Hit {
                weak: hit.weak,
                kill: v.health <= 0.0,
            });
        }
    }
}

/// Authority: a player out of health dies, and comes back aboard the home
/// ship. (If there is no home ship just now, they wait for one.)
pub fn settle_deaths(
    tick: Res<SimTick>,
    tuning: Res<Tuning>,
    home: Option<Single<Entity, With<HomeShip>>>,
    mut players: Query<(Entity, &mut Player, &mut Vitals, &DeckSlot, Option<&BodyTuning>)>,
    mut commands: Commands,
) {
    let Some(home) = home else {
        return;
    };
    for (entity, mut player, mut vitals, slot, own) in &mut players {
        if vitals.health > 0.0 {
            continue;
        }
        // What they carried stays where they fell: on the deck under them,
        // or adrift on their last velocity.
        let items = player.loadout.inventory.empty_out();
        if !items.is_empty() {
            commands.spawn((Name::new("Dropped kit"), Pickup::left_by(entity, left_where(&player.mode), tick.0, items)));
        }
        player.events.push(PlayerEvent::Died);
        player.respawn(*home, slot.0);
        *vitals = Vitals::full(tuning_of(&tuning, own));
    }
}
