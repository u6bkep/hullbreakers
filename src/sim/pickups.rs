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

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, RigidBody, Rotation};
use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::bodies::Bodies;
use super::bots::{BodyTuning, Gait, Robot, Wrecked};
use super::design::ShipDesign;
use super::events::PlayerEvent;
use super::input::TickInput;
use super::items::{Item, ItemOp, Place, Rarity, Taken, Weapon, crate_loot, draw, robot_loot};
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
    /// Whose it is, if anyone's: the guns in it are still theirs (see
    /// `sim::progression`), and with `owner_only` (what they left where
    /// they died) nobody else can take it.
    pub owner: Option<Entity>,
    pub owner_only: bool,
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
        if let Some(owner) = &mut self.owner {
            *owner = mapper.get_mapped(*owner);
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
            owner: None,
            owner_only: false,
        }
    }

    /// What `owner` left where they died: theirs alone to take back.
    pub fn left_by(owner: Entity, place: (Option<Entity>, Vec3, Vec3), since: u32, items: Vec<Item>) -> Pickup {
        Pickup {
            owner: Some(owner),
            owner_only: true,
            ..Pickup::dropped(place, since, items)
        }
    }

    /// Whether `player` may take from it.
    pub fn open_to(&self, player: Entity) -> bool {
        !self.owner_only || self.owner == Some(player)
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
    /// Only this player may take from it (what they left where they died).
    pub only: Option<Entity>,
}

/// Player simulation: `me` takes what fits from a pickup (of a box of ammo,
/// as many rounds as fit), skipping the items in `claimed` (taken from by
/// someone else this tick). Returns the items it took from (bits by index).
pub fn take_from(me: Entity, player: &mut Player, pickup: &PickupPoint, claimed: u32) -> u32 {
    if pickup.only.is_some_and(|owner| owner != me) {
        player.events.push(PlayerEvent::NotYours);
        return 0;
    }
    let before = player.loadout.held();
    let mut taken = 0u32;
    let mut partly = [None; 2];
    let mut touched = 0u32;
    for (i, item) in pickup.items.iter().enumerate().take(32) {
        let bit = 1 << i;
        if claimed & bit != 0 {
            continue;
        }
        match player.loadout.inventory.take_what_fits(*item) {
            Taken::All => taken |= bit,
            // Once a pool is full, no more of its kind fits: one part-taken
            // box per kind at most.
            Taken::Part { left } => {
                if let Some(free) = partly.iter_mut().find(|p| p.is_none()) {
                    *free = Some((i as u8, left));
                }
            }
            Taken::Nothing => continue,
        }
        touched |= bit;
    }
    player.loadout.hands_changed(before);
    if touched == 0 {
        if pickup.items.iter().enumerate().any(|(i, _)| claimed & (1 << i) == 0) {
            player.events.push(PlayerEvent::InventoryFull);
        }
    } else {
        player.events.push(PlayerEvent::PickedUp {
            pickup: pickup.entity,
            taken,
            partly,
        });
    }
    touched
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
    for op in cmd.item_ops.into_iter().flatten() {
        match op {
            ItemOp::None => {}
            ItemOp::Move { from, to } => {
                inventory.move_item(from, to);
            }
            ItemOp::Drop(place) => dropped.extend(inventory.remove(place)),
            ItemOp::DropAmmo(kind) => {
                let pool = inventory.ammo_mut(kind);
                let rounds = (*pool).min(kind.box_rounds());
                if rounds > 0 {
                    *pool -= rounds;
                    dropped.push(Item::Ammo { kind, rounds });
                }
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
    mut profiles: Query<&mut super::progression::Profile>,
    mut commands: Commands,
) {
    let mut taken: Vec<(Entity, u32, Vec<(u8, u16)>)> = Vec::new();
    for player in &players {
        for (at, event) in player.events.iter() {
            if let PlayerEvent::PickedUp { pickup, taken: bits, partly } = event
                && at == tick.0
            {
                let index = match taken.iter().position(|(e, ..)| *e == pickup) {
                    Some(index) => index,
                    None => {
                        taken.push((pickup, 0, Vec::new()));
                        taken.len() - 1
                    }
                };
                taken[index].1 |= bits;
                taken[index].2.extend(partly.into_iter().flatten());
            }
        }
    }
    for (entity, bits, partly) in taken {
        let Ok(mut pickup) = pickups.get_mut(entity) else { continue };
        // Guns of someone's, taken back (or by a crewmate, whose they are
        // now), aren't out in the world any more.
        if let Some(mut profile) = pickup.owner.and_then(|o| profiles.get_mut(o).ok()) {
            for (i, item) in pickup.items.iter().enumerate().take(32) {
                if let Item::Weapon(w) = item
                    && bits & (1 << i) != 0
                    && let Some(k) = profile.out.iter().position(|o| o == w)
                {
                    profile.out.swap_remove(k);
                }
            }
        }
        // Boxes of ammo part taken keep what's left in them.
        for (index, left) in partly {
            if let Some(Item::Ammo { rounds, .. }) = pickup.items.get_mut(index as usize) {
                *rounds = left;
            }
        }
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
/// It's still theirs until someone takes it (a crewmate taking it makes it
/// theirs).
pub fn spawn_drops(tick: Res<SimTick>, players: Query<(Entity, &Player)>, mut commands: Commands) {
    for (entity, player) in &players {
        for (at, event) in player.events.iter() {
            if let PlayerEvent::Dropped(item) = event
                && at == tick.0
            {
                commands.spawn((
                    Name::new("Dropped item"),
                    Pickup {
                        owner: Some(entity),
                        ..Pickup::dropped(dropped_where(&player.mode), tick.0, vec![item])
                    },
                ));
            }
        }
    }
}

/// Authority: guns of a player's put down in the world are still theirs:
/// noted in their profile as out (and back in the stash if nobody picks
/// them up by the mission's end; see `progression::settle_missions`).
pub fn note_owned(add: On<Add, Pickup>, pickups: Query<&Pickup>, mut profiles: Query<&mut super::progression::Profile>) {
    let Ok(pickup) = pickups.get(add.entity) else { return };
    let Some(mut profile) = pickup.owner.and_then(|o| profiles.get_mut(o).ok()) else {
        return;
    };
    for item in &pickup.items {
        if let Item::Weapon(w) = item
            && w.rarity != Rarity::Standard
        {
            profile.out.push(*w);
        }
    }
}

/// Authority: a robot out of health leaves something behind (before it's
/// removed): mostly ammo, sometimes a gun, better from tougher robots. Once:
/// on the tick it's destroyed, not the tick its wreck is cleared (see
/// [`Wrecked`]). A hull turret leaves nothing: it's bolted to the outside of
/// the hull, where nothing would stay put. Nor does a creature (a limpet).
pub fn drop_robot_loot(
    tick: Res<SimTick>,
    robots: Query<(Entity, &Robot, &Player, &Vitals, &BodyTuning), Without<Wrecked>>,
    mut commands: Commands,
) {
    for (entity, kind, player, vitals, tuning) in &robots {
        if vitals.health > 0.0 || kind.gait() == Gait::Mount || kind.fauna() {
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
                owner: None,
                owner_only: false,
            },
        ));
    }
}

/// Debug: asks the authority for a loot crate at a player's feet (for
/// looking over what generation makes).
#[derive(Message, Clone, Copy, Debug)]
pub struct SpawnLoot {
    pub near: Entity,
}

/// Authority: debug loot crates, rolled from the tick, of middling luck.
pub fn debug_spawn_loot(
    tick: Res<SimTick>,
    mut orders: MessageReader<SpawnLoot>,
    players: Query<&Player>,
    mut commands: Commands,
) {
    for (i, order) in orders.read().enumerate() {
        let Ok(player) = players.get(order.near) else { continue };
        let (body, at, velocity) = dropped_where(&player.mode);
        let at = at + Vec3::Y * (CRATE_HALF.y - PICKUP_HALF.y) * body.is_some() as u32 as f32;
        let seed = draw(tick.0, i as u32);
        commands.spawn((
            Name::new("Debug loot"),
            Pickup {
                body,
                at,
                velocity,
                since: tick.0,
                items: crate_loot(seed, draw(seed, 99) % 101),
                kind: PickupKind::Crate,
                owner: None,
                owner_only: false,
            },
        ));
    }
}

/// Authority: a body going away (a wrecked ship, a recycled rock) leaves
/// what was lying on it adrift, where it was and moving as it was.
pub fn set_adrift(
    remove: On<Remove, RigidBody>,
    tick: Res<SimTick>,
    bodies: Query<(&Position, &Rotation, Option<&LinearVelocity>, Option<&AngularVelocity>)>,
    mut pickups: Query<&mut Pickup>,
) {
    let Ok((position, rotation, linear, angular)) = bodies.get(remove.entity) else { return };
    for mut pickup in &mut pickups {
        if pickup.body != Some(remove.entity) {
            continue;
        }
        let at = position.0 + rotation.0 * pickup.at;
        let spin = angular.map_or(Vec3::ZERO, |w| w.0).cross(at - position.0);
        pickup.velocity = linear.map_or(Vec3::ZERO, |v| v.0) + spin;
        pickup.body = None;
        pickup.at = at;
        pickup.since = tick.0;
    }
}
