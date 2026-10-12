//! Things lying about to pick up: what a dead player carried, what someone
//! dropped, what a robot left, and loot crates aboard enemy ships.
//!
//! A pickup isn't a physics body. It's fixed to the body it was left on (a
//! deck), or, left in open space, drifts on at the velocity it was dropped
//! with, so every peer knows where it is from its replicated description
//! and the bodies. A player takes from one by looking at it within reach and
//! pressing interact: everything in it that fits their inventory. That's
//! part of their (predicted) simulation; the authority then takes the same
//! items out of the pickup, and removes it once it's empty.
//!
//! Dropping is the other way round: the player's simulation takes the item
//! out of the inventory and records a [`PlayerEvent::Dropped`]; the
//! authority leaves it as a pickup at their feet.

use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bodies::Bodies;
use super::bots::{BodyTuning, Robot};
use super::design::ShipDesign;
use super::events::PlayerEvent;
use super::input::TickInput;
use super::items::{Item, ItemOp, Place, Rarity, Weapon, crate_loot, draw, robot_loot};
use super::player::{Mode, Player};
use super::vitals::Vitals;
use super::weapons::Hand;
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
    pub items: Vec<Item>,
    pub kind: PickupKind,
}

/// What it looks like.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PickupKind {
    /// A case: what someone left or dropped.
    #[default]
    Dropped,
    /// A loot crate, part of a ship's fittings.
    Crate,
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
/// Half the size of a loot crate.
pub const CRATE_HALF: Vec3 = Vec3::new(0.5, 0.3, 0.35);

impl Pickup {
    /// Something left at `body`'s `at` (or adrift), from tick `since`.
    pub fn dropped((body, at, velocity): (Option<Entity>, Vec3, Vec3), since: u32, items: Vec<Item>) -> Pickup {
        Pickup {
            body,
            at,
            velocity,
            since,
            items,
            kind: PickupKind::Dropped,
        }
    }

    pub fn half(&self) -> Vec3 {
        match self.kind {
            PickupKind::Dropped => PICKUP_HALF,
            PickupKind::Crate => CRATE_HALF,
        }
    }

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

    /// Its best gun, to show on it.
    pub fn showcase(&self) -> Option<Weapon> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Item::Weapon(w) => Some(*w),
                _ => None,
            })
            .max_by_key(|w| w.rarity)
    }

    pub fn best_rarity(&self) -> Option<Rarity> {
        self.items.iter().filter_map(Item::rarity).max()
    }

    /// What taking it is called: "Take Kessler Mk3 Longshot".
    pub fn label(&self) -> String {
        match (self.kind, self.items.as_slice()) {
            (PickupKind::Crate, _) => "Open crate".into(),
            (_, [one]) => format!("Take {}", one.name()),
            (_, items) => format!("Take {} items", items.len()),
        }
    }
}

/// Where something left by a player (or robot) in `mode` lies: on the deck
/// under them, or adrift on their last velocity. (Body, place, velocity,
/// as [`Pickup::dropped`] takes them.)
pub fn left_where(mode: &Mode) -> (Option<Entity>, Vec3, Vec3) {
    match *mode {
        Mode::Booted { body, feet, up, .. } => (Some(body), feet + up * PICKUP_HALF.y, Vec3::ZERO),
        Mode::Eva { position, velocity, .. } => (None, position, velocity),
    }
}

/// Where something a player puts down goes: on the deck a step in front of
/// them, or, in EVA, tossed gently ahead.
fn dropped_where(mode: &Mode) -> (Option<Entity>, Vec3, Vec3) {
    match *mode {
        Mode::Booted { body, feet, up, orientation, .. } => {
            let ahead = orientation * Vec3::NEG_Z;
            (Some(body), feet + ahead * 0.7 + up * PICKUP_HALF.y, Vec3::ZERO)
        }
        Mode::Eva { position, velocity, orientation, pitch } => {
            let ahead = orientation * Quat::from_rotation_x(pitch) * Vec3::NEG_Z;
            (None, position + ahead * 0.8, velocity + ahead * 0.5)
        }
    }
}

/// A pickup as the player simulation sees it this tick.
#[derive(Clone, Debug)]
pub struct PickupPoint {
    pub entity: Entity,
    pub at: Vec3,
    pub turn: Quat,
    pub half: Vec3,
    pub items: Vec<Item>,
}

/// Player simulation: takes what fits from a pickup, skipping the items in
/// `claimed` (taken by someone else this tick). Returns what it took (bits
/// by index).
pub fn take_from(player: &mut Player, pickup: &PickupPoint, claimed: u32) -> u32 {
    let before = player.loadout.held();
    let mut taken = 0u32;
    for (i, item) in pickup.items.iter().enumerate().take(32) {
        let bit = 1 << i;
        if claimed & bit == 0 && player.loadout.inventory.take(*item) {
            taken |= bit;
        }
    }
    player.loadout.hands_changed(before);
    if taken == 0 {
        if pickup.items.iter().enumerate().any(|(i, _)| claimed & (1 << i) == 0) {
            player.events.push(PlayerEvent::InventoryFull);
        }
    } else {
        player.events.push(PlayerEvent::PickedUp {
            pickup: pickup.entity,
            taken,
        });
    }
    taken
}

/// Player simulation: dropping what's in hand, and what the inventory
/// screen asks for.
pub fn handle_inventory(player: &mut Player, cmd: &TickInput) {
    let before = player.loadout.held();
    let mut dropped = Vec::new();
    if cmd.drop {
        // Cargo in both hands first; else the main hand's gun, else the
        // off hand's.
        let place = if player.loadout.hands_full() {
            Some(Place::Hands)
        } else {
            Hand::BOTH
                .iter()
                .find_map(|h| player.loadout.weapon_slot(*h))
                .map(|i| Place::Weapon(i as u8))
        };
        dropped.extend(place.and_then(|p| player.loadout.inventory.remove(p)));
    }
    let inventory = &mut player.loadout.inventory;
    match cmd.item_op {
        None | Some(ItemOp::None) => {}
        Some(ItemOp::Move { from, to }) => {
            inventory.move_item(from, to);
        }
        Some(ItemOp::Drop(place)) => dropped.extend(inventory.remove(place)),
        Some(ItemOp::DropAmmo(kind)) => {
            let pool = inventory.ammo_mut(kind);
            let rounds = (*pool).min(kind.box_rounds());
            if rounds > 0 {
                *pool -= rounds;
                dropped.push(Item::Ammo { kind, rounds });
            }
        }
    }
    for item in dropped {
        player.events.push(PlayerEvent::Dropped(item));
    }
    player.loadout.hands_changed(before);
}

/// Authority: what was taken from pickups this tick comes out of them; an
/// emptied one is gone. (The takers already have it.)
pub fn remove_taken(
    tick: Res<SimTick>,
    players: Query<&Player>,
    mut pickups: Query<&mut Pickup>,
    mut commands: Commands,
) {
    let mut taken: Vec<(Entity, u32)> = Vec::new();
    for player in &players {
        for (at, event) in player.events.iter() {
            if let PlayerEvent::PickedUp { pickup, taken: bits } = event
                && at == tick.0
            {
                match taken.iter_mut().find(|(e, _)| *e == pickup) {
                    Some((_, all)) => *all |= bits,
                    None => taken.push((pickup, bits)),
                }
            }
        }
    }
    for (entity, bits) in taken {
        let Ok(mut pickup) = pickups.get_mut(entity) else { continue };
        let mut i = 0;
        pickup.items.retain(|_| {
            let keep = i >= 32 || bits & (1 << i) == 0;
            i += 1;
            keep
        });
        if pickup.items.is_empty() {
            commands.entity(entity).despawn();
        }
    }
}

/// Authority: what players put down this tick is left where they put it.
pub fn spawn_drops(tick: Res<SimTick>, players: Query<&Player>, mut commands: Commands) {
    for player in &players {
        for (at, event) in player.events.iter() {
            if let PlayerEvent::Dropped(item) = event
                && at == tick.0
            {
                commands.spawn((
                    Name::new("Dropped item"),
                    Pickup::dropped(dropped_where(&player.mode), tick.0, vec![item]),
                ));
            }
        }
    }
}

/// Authority: a robot out of health leaves something behind (before it's
/// removed): mostly ammo, sometimes a gun, better from tougher robots.
pub fn drop_robot_loot(
    tick: Res<SimTick>,
    robots: Query<(Entity, &Player, &Vitals, &BodyTuning), With<Robot>>,
    mut commands: Commands,
) {
    for (entity, player, vitals, tuning) in &robots {
        if vitals.health > 0.0 {
            continue;
        }
        let bits = entity.to_bits();
        let seed = draw(tick.0, (bits ^ (bits >> 32)) as u32);
        let items = robot_loot(seed, tuning.0.player_health);
        commands.spawn((Name::new("Robot loot"), Pickup::dropped(left_where(&player.mode), tick.0, items)));
    }
}

/// Puts loot crates aboard `ship` at its design's `loot…` markers. Crates
/// deeper in (nearer the `objective` than the `entry`) roll better.
pub fn stock_ship(commands: &mut Commands, ship: Entity, design: &ShipDesign, seed: u32) {
    let (entry, objective) = (design.markers.get("entry"), design.markers.get("objective"));
    let mut spots: Vec<(&String, &Vec3)> = design.markers.iter().filter(|(name, _)| name.starts_with("loot")).collect();
    // The same crates whatever order the markers were read in.
    spots.sort_by(|a, b| a.0.cmp(b.0));
    for (i, (_, &at)) in spots.into_iter().enumerate() {
        let depth = match (entry, objective) {
            (Some(&e), Some(&o)) if e.distance(o) > 1.0 => (1.0 - at.distance(o) / e.distance(o)).clamp(0.0, 1.0),
            _ => 0.5,
        };
        let luck = (depth * 100.0) as u32;
        commands.spawn((
            Name::new("Loot crate"),
            Pickup {
                body: Some(ship),
                at: at + Vec3::Y * CRATE_HALF.y,
                velocity: Vec3::ZERO,
                since: 0,
                items: crate_loot(draw(seed, i as u32), luck),
                kind: PickupKind::Crate,
            },
        ));
    }
}
