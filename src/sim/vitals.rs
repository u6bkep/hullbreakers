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
use super::bots::{self, Affixes, BodyTuning, Brain, Condition, Robot, tuning_of};
use super::pickups::{Pickup, left_where};
use super::combat::Beams;
use super::player::{DeckSlot, Mode, Player};
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
    /// Where it came from, in the world (a shot's muzzle, a beam's turret),
    /// for the victim's [`HurtLog`].
    pub from: Option<Vec3>,
    /// From something that hurts every tick (a crew laser's beam): it's
    /// applied every tick, but the logs (the shooter's hit markers and
    /// sounds, the victim's damage arcs, and what they cost on the wire)
    /// get one entry per [`STEADY_LOG_SECONDS`] of it, its damage summed.
    pub steady: bool,
}

/// How often a steady source's hits on one target are logged (s).
pub const STEADY_LOG_SECONDS: f32 = 0.15;

/// Steady hits gathered since they were last logged, by source and target:
/// when that was, and the damage since.
#[derive(Resource, Default)]
pub struct SteadyHits(std::collections::HashMap<(Option<Entity>, Entity), (f32, f32)>);

/// What has recently hurt a player, and from where: their HUD's damage
/// arcs. The authority writes it; a client can't know what hit it.
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HurtLog {
    /// Hurts counted so far; a reader keeps its own count to see new ones.
    pub count: u32,
    /// The most recent, newest last.
    pub recent: VecDeque<Hurt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hurt {
    /// Where it came from, in the world, if anywhere.
    pub from: Option<Vec3>,
    pub amount: f32,
    /// It was the killing blow.
    pub kill: bool,
}

/// How many hurts a log keeps.
const HURT_LOG: usize = 8;

impl HurtLog {
    fn push(&mut self, hurt: Hurt) {
        self.count = self.count.wrapping_add(1);
        self.recent.push_back(hurt);
        while self.recent.len() > HURT_LOG {
            self.recent.pop_front();
        }
    }

    /// Hurts since a reader last saw `count` of them, oldest first.
    pub fn since(&self, count: u32) -> impl Iterator<Item = &Hurt> {
        let new = (self.count.wrapping_sub(count) as usize).min(self.recent.len());
        self.recent.iter().skip(self.recent.len() - new)
    }
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
    beams: Option<Res<Beams>>,
    players: Query<(Entity, &Player)>,
    mut damage: MessageWriter<Damage>,
) {
    for (entity, player) in &players {
        let hit = player
            .events
            .iter()
            .any(|(at, e)| at == tick.0 && e == PlayerEvent::BeamHit);
        if hit {
            // Beams catch players in EVA: the nearest one is the turret.
            let from = match (player.mode, beams.as_deref()) {
                (Mode::Eva { position, .. }, Some(beams)) => beams
                    .0
                    .iter()
                    .map(|b| {
                        let d = (b.to - b.from).normalize_or_zero();
                        let along = (position - b.from).dot(d).clamp(0.0, b.from.distance(b.to));
                        ((position - (b.from + d * along)).length(), b.from)
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, from)| from),
                _ => None,
            };
            damage.write(Damage {
                target: entity,
                amount: tuning.beam_damage,
                source: None,
                weak: false,
                from,
                steady: false,
            });
        }
    }
}

/// Authority: applies this tick's damage, and heals whoever has gone long
/// enough unhurt. A robot's affixes have their say first (a shell, armour:
/// see [`bots::absorb`]).
pub fn apply_damage(
    time: Res<Time>,
    tuning: Res<Tuning>,
    mut damage: MessageReader<Damage>,
    mut vitals: Query<(&mut Vitals, Has<Robot>, Has<Player>, Option<&BodyTuning>, Option<(&Affixes, &mut Brain, &mut Condition)>)>,
    mut logs: Query<&mut HitLog>,
    mut hurts: Query<&mut HurtLog>,
    mut steady: ResMut<SteadyHits>,
) {
    let dt = time.delta_secs();
    let now = time.elapsed_secs();
    // A beam that's moved off (or stopped) starts afresh next time.
    steady.0.retain(|_, (logged, _)| now - *logged < 2.0 * STEADY_LOG_SECONDS);
    // The crew's suits heal (to their own most: rank plates them); robots
    // and creatures don't.
    for (mut v, robot, player, own, _) in &mut vitals {
        v.since_hurt += dt;
        if player && !robot && v.since_hurt > tuning.health_regen_delay {
            v.health = (v.health + tuning.health_regen_rate * dt).min(tuning_of(&tuning, own).player_health);
        }
    }
    for hit in damage.read() {
        let Ok((mut v, _, _, _, guard)) = vitals.get_mut(hit.target) else {
            continue;
        };
        // Already dead this tick: no more hits to count.
        if v.health <= 0.0 {
            continue;
        }
        let (amount, weak) = match guard {
            Some((affixes, mut brain, mut condition)) => bots::absorb(*affixes, &mut brain, &mut condition, hit.source, hit.amount, hit.weak),
            None => (hit.amount, hit.weak),
        };
        v.health = (v.health - amount).max(0.0);
        v.since_hurt = 0.0;
        let kill = v.health <= 0.0;
        // A steady source logs a hit now and then, its damage summed (its
        // first touch and a kill at once).
        let amount = if hit.steady {
            let entry = steady.0.entry((hit.source, hit.target)).or_insert((f32::MIN, 0.0));
            entry.1 += amount;
            if !kill && now - entry.0 < STEADY_LOG_SECONDS {
                continue;
            }
            let sum = entry.1;
            *entry = (now, 0.0);
            sum
        } else {
            amount
        };
        if let Ok(mut log) = hurts.get_mut(hit.target) {
            log.push(Hurt {
                from: hit.from,
                amount,
                kill: v.health <= 0.0,
            });
        }
        if let Some(mut log) = hit.source.and_then(|s| logs.get_mut(s).ok()) {
            log.push(Hit {
                weak,
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
        // Their own gear is theirs to come back for; mission cargo is
        // anyone's, in a case of its own.
        let (cargo, items): (Vec<_>, Vec<_>) = player
            .loadout
            .inventory
            .empty_out()
            .into_iter()
            .partition(|i| matches!(i, crate::sim::items::Item::Cargo(_)));
        if !items.is_empty() {
            commands.spawn((Name::new("Dropped kit"), Pickup::left_by(entity, left_where(&player.mode), tick.0, items)));
        }
        if !cargo.is_empty() {
            commands.spawn((Name::new("Dropped cargo"), Pickup::dropped(left_where(&player.mode), tick.0, cargo)));
        }
        player.events.push(PlayerEvent::Died);
        player.respawn(*home, slot.0);
        *vitals = Vitals::full(tuning_of(&tuning, own));
    }
}
