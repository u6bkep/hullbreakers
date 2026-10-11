//! Health: how much more a player can take, and dying.
//!
//! Only the authority hurts players. Vitals live apart from [`Player`]
//! (which a client predicts) because a client can't predict what will hurt
//! it; it learns its health from the authority, and a death reaches it as a
//! respawn like any other.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::SimTick;
use super::events::PlayerEvent;
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
    mut vitals: Query<&mut Vitals>,
) {
    let dt = time.delta_secs();
    for mut v in &mut vitals {
        v.since_hurt += dt;
        if v.since_hurt > tuning.health_regen_delay {
            v.health = (v.health + tuning.health_regen_rate * dt).min(tuning.player_health);
        }
    }
    for hit in damage.read() {
        if let Ok(mut v) = vitals.get_mut(hit.target) {
            v.health = (v.health - hit.amount).max(0.0);
            v.since_hurt = 0.0;
        }
    }
}

/// Authority: a player out of health dies, and comes back aboard the home
/// ship. (If there is no home ship just now, they wait for one.)
pub fn settle_deaths(
    tuning: Res<Tuning>,
    home: Option<Single<Entity, With<HomeShip>>>,
    mut players: Query<(&mut Player, &mut Vitals, &DeckSlot)>,
) {
    let Some(home) = home else {
        return;
    };
    for (mut player, mut vitals, slot) in &mut players {
        if vitals.health > 0.0 {
            continue;
        }
        player.events.push(PlayerEvent::Died);
        player.respawn(*home, slot.0);
        *vitals = Vitals::full(&tuning);
    }
}
