//! What a player keeps: their profile.
//!
//! Each player's profile lives on their own machine (`profile.ron`, see
//! `client::save`): the guns they own, what they carry, their salvage and
//! experience. Co-op is between friends, so a client's profile is trusted,
//! as Deep Rock trusts its players'. On joining, the client sends it to the
//! host ([`ProfileWire::Upload`]); from then on the host is authoritative
//! over it: it's a [`Profile`] component on the player, replicated, and
//! what the player carries is their inventory, as for anyone. The client
//! saves what it's sent (the replicated profile and its own inventory),
//! so if the host vanishes it keeps the last of it.
//!
//! Everything a client asks of its profile (the armory, the workbench)
//! goes to the host as a [`ProfileWire::Op`], which the host applies in
//! [`apply_profile_orders`]. A host's own player's go there directly.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::items::{self, AmmoKind, Inventory, Item, Place, Rarity, Weapon};
use super::bots::{BodyTuning, Robot};
use super::tuning::Tuning;
use super::mission::{CrewTally, Director, Mission, Phase, ScenarioKind};
use super::scenarios;
use super::pickups::Pickup;
use super::player::Player;
use super::ship::HomeShip;
use super::weapons::GunKind;

/// What a player owns besides what they carry, kept by the host while
/// they play. Replicated (the armory and workbench show it).
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Guns in the armory.
    pub stash: Vec<Weapon>,
    /// Currency, from missions and scrapping.
    pub salvage: u32,
    /// Experience: rank comes from it.
    pub xp: u32,
    /// Guns of theirs lying out in the world (dropped, or left where they
    /// died), which are still theirs.
    pub out: Vec<Weapon>,
    /// Missions finished.
    pub missions: u32,
    /// Part variants unlocked for refits, by scrapping guns that had them
    /// (a bit per variant: its slot times four, plus its index in the
    /// slot; see [`items::SLOTS`]).
    #[serde(default)]
    pub parts: u32,
    /// Who it is, for good: made at random with the profile, never shown.
    /// What's kept of a player across sessions (a personal best) is kept
    /// by this, not by a name or a slot. 0: none yet (a file from before;
    /// the machine that owns it gives it one on loading).
    #[serde(default)]
    pub id: u64,
    /// What the crew call them: chosen in the menu; a callsign made from
    /// the id until then (see [`callsign`]).
    #[serde(default)]
    pub name: String,
}

/// The longest a player's name may be (characters).
pub const NAME_MAX: usize = 20;

/// A callsign for a new profile, from its id: "Kestrel 42".
pub fn callsign(id: u64) -> String {
    const WORDS: [&str; 32] = [
        "Kestrel", "Osprey", "Talon", "Vesper", "Halcyon", "Rook", "Corsair", "Lantern", "Sable", "Quill", "Ember", "Tern",
        "Bramble", "Cinder", "Drift", "Flint", "Gannet", "Harrow", "Ibis", "Jackdaw", "Kite", "Lark", "Merlin", "Nomad",
        "Petrel", "Raven", "Skua", "Thistle", "Umber", "Vireo", "Wren", "Yarrow",
    ];
    let mixed = id.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    format!("{} {}", WORDS[(mixed >> 59) as usize % WORDS.len()], 10 + (mixed >> 20) % 90)
}

/// A name as a player typed it, made fit to show: trimmed, no control
/// characters, at most [`NAME_MAX`] characters; `None` if nothing's left.
pub fn clean_name(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !c.is_control()).take(NAME_MAX).collect();
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}

impl Profile {
    /// The profile as it's saved: guns still out in the world are back in
    /// the stash (or gone, if unrecovered gear is lost; see
    /// [`RECOVER_UNCLAIMED`]).
    pub fn settled(&self) -> Profile {
        let mut p = self.clone();
        let out = std::mem::take(&mut p.out);
        if RECOVER_UNCLAIMED {
            p.stash.extend(out);
        }
        p
    }
}

/// Whether gear a player left in the world (where they died, or dropped)
/// and didn't pick up again goes back to their stash when the mission ends.
/// `false`: it's lost for good.
pub const RECOVER_UNCLAIMED: bool = true;

/// What's in `profile.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ProfileFile {
    pub version: u32,
    pub profile: Profile,
    /// What they carried when it was saved.
    pub carried: Inventory,
}

impl ProfileFile {
    pub const VERSION: u32 = 1;

    pub fn fresh() -> ProfileFile {
        ProfileFile {
            version: Self::VERSION,
            ..default()
        }
    }

    /// The file for a player with `profile`, carrying `carried` (less any
    /// mission cargo).
    pub fn of(profile: &Profile, carried: &Inventory) -> ProfileFile {
        let mut carried = *carried;
        carried.strip_cargo();
        ProfileFile {
            version: Self::VERSION,
            profile: profile.settled(),
            carried,
        }
    }

    /// The same but for what's in mags and pools (which change with every
    /// shot).
    fn same_gear(&self, other: &ProfileFile) -> bool {
        let gear = |f: &ProfileFile| {
            let mut f = f.clone();
            f.carried.ammo = [0; AmmoKind::COUNT];
            f.carried.all_weapons_mut().for_each(|w| w.mag = 0);
            f
        };
        gear(self) == gear(other)
    }
}

/// The host's snapshot of a player's profile as it's to be saved: their
/// profile and what they carry, at one tick (so a gun is never both carried
/// and out in the world, or neither). Replicated; the player's own machine
/// saves it (see `net::profiles`).
#[derive(Component, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SavedProfile {
    pub file: ProfileFile,
    /// The tick it was taken.
    pub at: u32,
}

/// Seconds between snapshots while only ammo changes; a change of gear (or
/// a mission's end) is snapshotted at once.
const SNAPSHOT_SECONDS: f32 = 5.0;

/// Authority, at the end of each tick: every profile's snapshot, kept up.
pub fn snapshot_profiles(
    tick: Res<super::SimTick>,
    players: Query<(Entity, &Player, &Profile, Option<&SavedProfile>)>,
    mut commands: Commands,
) {
    for (entity, player, profile, saved) in &players {
        let file = ProfileFile::of(profile, &player.loadout.inventory);
        let due = match saved {
            None => true,
            Some(saved) if saved.file == file => false,
            Some(saved) => {
                !saved.file.same_gear(&file)
                    || tick.0.wrapping_sub(saved.at) as f32 >= SNAPSHOT_SECONDS * super::TICK_HZ as f32
            }
        };
        if due {
            commands.entity(entity).insert(SavedProfile { file, at: tick.0 });
        }
    }
}

/// Authority: a player leaving (their profile going with them) takes their
/// guns out of the world: their last save has them back in the stash (see
/// [`Profile::settled`]), so a crewmate (or they, back as someone new) can't
/// have them twice. What's left of their cases is anyone's.
pub fn leave_with_gear(remove: On<Remove, Profile>, mut pickups: Query<(Entity, &mut Pickup)>, mut commands: Commands) {
    for (entity, mut pickup) in &mut pickups {
        if pickup.owner != Some(remove.entity) {
            continue;
        }
        if RECOVER_UNCLAIMED {
            pickup.items.retain(|i| !matches!(i, Item::Weapon(w) if w.rarity != Rarity::Standard));
        }
        pickup.owner = None;
        pickup.owner_only = false;
        if pickup.items.is_empty() {
            commands.entity(entity).despawn();
        }
    }
}

/// Authority: when the world is about to change, mission cargo is left in
/// it (a data core brought home has done its job).
pub fn leave_cargo(director: Res<Director>, mut players: Query<&mut Player, Without<Robot>>) {
    if director.switching().is_none() {
        return;
    }
    for mut player in &mut players {
        let before = player.loadout.held();
        if player.loadout.inventory.strip_cargo() {
            player.loadout.hands_changed(before);
        }
    }
}

/// What a client sends the host about its profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProfileWire {
    /// Its profile, on joining.
    Upload(ProfileFile),
    /// Something done at the armory or workbench.
    Op(ProfileOp),
    /// A new name, chosen in the menu.
    Rename(String),
}

/// Something asked of a profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileOp {
    /// The armory: a gun from the stash into the inventory (the first free
    /// place, as a pickup would go).
    Take(Weapon),
    /// The armory: a standard-issue gun of a frame, free.
    TakeIssue(GunKind),
    /// The armory: a carried gun into the stash (standard issue just goes
    /// back on the rack).
    Store(Holding),
    /// The workbench: a gun up a tier of rarity (see [`tune_cost`]).
    Tune(Holding),
    /// The workbench: one part slot (in [`items::SLOTS`] order) fitted with
    /// a variant (its index in the slot; 0 is the stock part) the profile
    /// has unlocked, or back to what it was made with (`None`).
    Refit(Holding, u8, Option<u8>),
    /// The workbench: one of the crew's paints ([`items::PAINTS`]), or its
    /// maker's (`None`).
    Repaint(Holding, Option<u8>),
    /// The workbench: a gun broken down for salvage, unlocking its parts.
    Scrap(Holding),
}

/// A gun a player has: carried (where, and which gun was there: an order
/// travels apart from the inventory moves sent with input, so by the time
/// the host applies it another gun may be in that place, and then it's
/// refused), or in their stash (by what it is, not where, so a click on a
/// stash that has just changed can't act on the gun beside it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Holding {
    Carried(Place, Weapon),
    Stash(Weapon),
}

impl Holding {
    /// The gun it is, as it is now, if it's still there.
    pub fn weapon(self, inventory: &Inventory, profile: &Profile) -> Option<Weapon> {
        match self {
            Holding::Carried(place, was) => match inventory.get(place) {
                Some(Item::Weapon(w)) if w.same_make(&was) => Some(w),
                _ => None,
            },
            Holding::Stash(w) => profile.stash.contains(&w).then_some(w),
        }
    }

    /// The gun carried at `place`, if there's one.
    pub fn carried(inventory: &Inventory, place: Place) -> Option<Holding> {
        match inventory.get(place) {
            Some(Item::Weapon(w)) => Some(Holding::Carried(place, w)),
            _ => None,
        }
    }

    /// Whether two pick out the same gun (a carried one by its place and
    /// make: the workbench changing it doesn't make it another).
    pub fn same(self, other: Holding) -> bool {
        match (self, other) {
            (Holding::Carried(p, a), Holding::Carried(q, b)) => p == q && a.same_make(&b),
            (a, b) => a == b,
        }
    }
}

/// What tuning a gun up to `to` costs (salvage).
pub fn tune_cost(to: Rarity) -> u32 {
    match to {
        Rarity::Uncommon => 120,
        Rarity::Rare => 350,
        Rarity::Exotic => 900,
        Rarity::Standard | Rarity::Common => 0,
    }
}

/// What a refit of one part costs, and a new paint job.
pub const REFIT_COST: u32 = 60;
pub const REPAINT_COST: u32 = 20;

/// What scrapping a gun gives (salvage). Standard issue is worth nothing.
pub fn scrap_value(w: &Weapon) -> u32 {
    match w.rarity {
        Rarity::Standard => 0,
        Rarity::Common => 15,
        Rarity::Uncommon => 40,
        Rarity::Rare => 100,
        Rarity::Exotic => 250,
    }
}

/// The bit for a part variant in [`Profile::parts`].
pub fn part_bit(slot: usize, variant: usize) -> u32 {
    1 << (slot * 4 + variant)
}

impl Profile {
    /// Whether a refit can fit this variant: stock parts always can.
    pub fn has_part(&self, slot: usize, variant: usize) -> bool {
        variant == 0 || self.parts & part_bit(slot, variant) != 0
    }

    /// Spends `cost` salvage, if there's that much.
    fn spend(&mut self, cost: u32) -> bool {
        let ok = self.salvage >= cost;
        if ok {
            self.salvage -= cost;
        }
        ok
    }
}

/// The most guns a stash holds (past it, scrap some at the workbench).
pub const STASH_CAP: usize = 40;

/// Authority: what players asked of their profiles this frame, to apply
/// at the next tick. (A resource, not a message: frames and ticks don't
/// line up.)
#[derive(Resource, Default)]
pub struct ProfileOrders(pub Vec<(Entity, ProfileWire)>);

/// A profile from a file, made sound for play: only the crew's frames,
/// mags and pools within their sizes.
fn sanitized(mut file: ProfileFile) -> ProfileFile {
    let crew = |w: &Weapon| GunKind::CREW.contains(&w.frame);
    let fix = |w: &mut Weapon| w.mag = w.mag.min(w.stats().mag);
    file.profile.stash.retain(crew);
    file.profile.stash.iter_mut().for_each(fix);
    let inv = &mut file.carried;
    for slot in &mut inv.weapons {
        if slot.is_some_and(|w| !crew(&w)) {
            *slot = None;
        }
    }
    for slot in &mut inv.pack {
        if matches!(slot, Some(Item::Weapon(w)) if !crew(w)) {
            *slot = None;
        }
    }
    inv.all_weapons_mut().for_each(fix);
    inv.strip_cargo();
    file.profile.name = clean_name(&file.profile.name).unwrap_or_else(|| callsign(file.profile.id));
    file.profile.salvage = file.profile.salvage.min(u32::MAX / 2);
    file.profile.xp = file.profile.xp.min(u32::MAX / 2);
    for kind in AmmoKind::ALL {
        let pool = inv.ammo_mut(kind);
        *pool = (*pool).min(kind.cap());
    }
    // As many slots as their rank gives: no more (what's past them goes to
    // the stash), and no fewer.
    let open = unlocks(rank(file.profile.xp));
    inv.slots = (open.weapon_slots as u8, open.pack_slots as u8);
    let past: Vec<Weapon> = inv
        .overflow()
        .into_iter()
        .filter_map(|i| match i {
            Item::Weapon(w) => Some(w),
            _ => None,
        })
        .collect();
    file.profile.stash.extend(past);
    file
}

/// Authority: applies what players asked of their profiles.
pub fn apply_profile_orders(
    mut orders: ResMut<ProfileOrders>,
    mut players: Query<(&mut Player, Option<&mut Profile>)>,
    homes: Query<(), With<HomeShip>>,
    mut commands: Commands,
) {
    for (entity, wire) in orders.0.drain(..) {
        let Ok((mut player, profile)) = players.get_mut(entity) else {
            continue;
        };
        match (wire, profile) {
            // The first upload is who they are; later ones are ignored.
            (ProfileWire::Upload(file), None) => {
                let file = sanitized(file);
                info!(
                    "profile arrived: {} guns stashed, {} salvage, {} xp",
                    file.profile.stash.len(),
                    file.profile.salvage,
                    file.profile.xp
                );
                let before = player.loadout.held();
                player.loadout.inventory = file.carried;
                player.loadout.hands_changed(before);
                // They're called what their profile calls them.
                commands.entity(entity).insert((Name::new(file.profile.name.clone()), file.profile));
            }
            (ProfileWire::Upload(_), Some(_)) => {}
            (ProfileWire::Rename(name), Some(mut profile)) => {
                if let Some(name) = clean_name(&name) {
                    profile.name = name.clone();
                    commands.entity(entity).insert(Name::new(name));
                }
            }
            (ProfileWire::Rename(_), None) => {}
            // The armory and workbench are aboard our ship.
            (ProfileWire::Op(op), Some(mut profile)) => {
                if player.mode.body().is_some_and(|b| homes.contains(b)) {
                    apply(op, &mut player, &mut profile);
                }
            }
            (ProfileWire::Op(_), None) => {}
        }
    }
}

/// Does `op` to a player's profile and inventory, if it can be done.
/// Whether it was.
pub fn apply(op: ProfileOp, player: &mut Player, profile: &mut Profile) -> bool {
    let before = player.loadout.held();
    let inv = &mut player.loadout.inventory;
    let done = match op {
        ProfileOp::Take(w) => match profile.stash.iter().position(|s| *s == w) {
            Some(i) if inv.take(Item::Weapon(w)) => {
                profile.stash.remove(i);
                true
            }
            _ => false,
        },
        ProfileOp::TakeIssue(frame) => frames(rank(profile.xp)).contains(&frame) && inv.take(Item::Weapon(Weapon::issue(frame))),
        ProfileOp::Store(h) => match (h, h.weapon(inv, profile)) {
            (Holding::Carried(place, _), Some(w)) if w.rarity == Rarity::Standard => inv.remove(place).is_some(),
            (Holding::Carried(place, _), Some(w)) if profile.stash.len() < STASH_CAP => {
                inv.remove(place);
                profile.stash.push(w);
                true
            }
            _ => false,
        },
        ProfileOp::Tune(h) => workbench(h, inv, profile, |w, profile| {
            let Some(next) = w.rarity.next() else { return false };
            if !profile.spend(tune_cost(next)) {
                return false;
            }
            w.rarity = next;
            w.tune += 1;
            true
        }),
        ProfileOp::Refit(h, slot, variant) => workbench(h, inv, profile, |w, profile| {
            let slot = slot as usize;
            let Some(options) = items::SLOTS.get(slot) else { return false };
            let fit = match variant {
                Some(v) if (v as usize) < options.len() && profile.has_part(slot, v as usize) => v + 1,
                Some(_) => return false,
                None => 0,
            };
            // Nothing to do if that part is what it has already.
            let now = w.build().parts.slots()[slot];
            let same = match variant {
                Some(v) => options.get(v as usize) == Some(&now),
                None => w.refit.parts[slot] == 0,
            };
            if same || !profile.spend(REFIT_COST) {
                return false;
            }
            w.refit.parts[slot] = fit;
            true
        }),
        ProfileOp::Repaint(h, paint) => workbench(h, inv, profile, |w, profile| {
            let fit = match paint {
                Some(i) if items::PAINTS.get(i as usize).is_some_and(|p| rank(profile.xp) >= p.3) => i + 1,
                Some(_) => return false,
                None => 0,
            };
            if w.refit.paint == fit || !profile.spend(REPAINT_COST) {
                return false;
            }
            w.refit.paint = fit;
            true
        }),
        ProfileOp::Scrap(h) => match h.weapon(inv, profile) {
            Some(w) => {
                match h {
                    Holding::Carried(place, _) => {
                        inv.remove(place);
                    }
                    Holding::Stash(w) => {
                        if let Some(i) = profile.stash.iter().position(|s| *s == w) {
                            profile.stash.remove(i);
                        }
                    }
                }
                profile.salvage = profile.salvage.saturating_add(scrap_value(&w));
                if w.rarity != Rarity::Standard {
                    for part in w.build().parts.slots() {
                        let (slot, variant) = part.place();
                        if variant != 0 {
                            profile.parts |= part_bit(slot, variant);
                        }
                    }
                }
                true
            }
            None => false,
        },
    };
    player.loadout.hands_changed(before);
    done
}

/// Changes a gun at the workbench by `change`, if it's a gun that can be
/// changed (standard issue can't) and `change` says it was done; its mag
/// then holds no more than it can.
fn workbench(h: Holding, inv: &mut Inventory, profile: &mut Profile, change: impl FnOnce(&mut Weapon, &mut Profile) -> bool) -> bool {
    let Some(mut w) = h.weapon(inv, profile) else { return false };
    if w.rarity == Rarity::Standard || !change(&mut w, profile) {
        return false;
    }
    w.mag = w.mag.min(w.stats().mag);
    match h {
        Holding::Carried(place, _) => {
            inv.remove(place);
            if !inv.put_back(place, Item::Weapon(w)) {
                profile.stash.push(w);
            }
        }
        Holding::Stash(old) => match profile.stash.iter().position(|s| *s == old) {
            Some(i) => profile.stash[i] = w,
            None => return false,
        },
    }
    true
}

// ---------------------------------------------------------------------------
// Rank.

/// Experience to reach each rank from the one before (rank 1 is where
/// everyone starts): 500, 1000, 1500, ...
pub fn xp_for(rank: u8) -> u32 {
    (1..rank as u32).map(|r| 500 * r).sum()
}

/// The rank `xp` makes.
pub fn rank(xp: u32) -> u8 {
    (1..=MAX_RANK).rev().find(|r| xp >= xp_for(*r)).unwrap_or(1)
}

pub const MAX_RANK: u8 = 10;

/// What a rank has given (everything the ranks up to it brought).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unlocks {
    pub weapon_slots: usize,
    pub pack_slots: usize,
    /// Suit health, on top of the crew's.
    pub health: f32,
    /// Jetpack thrust and reel speed, as multiples of the crew's.
    pub thrust: f32,
    pub reel: f32,
}

/// What each rank brings, for the screens (paints are listed with their
/// ranks in [`items::PAINTS`]).
pub const LADDER: [(u8, &str); 10] = [
    (2, "Paint: the crew's first paint jobs, at the workbench"),
    (3, "A fourth weapon slot"),
    (4, "Two more pack slots"),
    (4, "The shotgun, as standard issue at the armory"),
    (5, "Suit plating: +25 health"),
    (6, "Jetpack: 15% more thrust"),
    (7, "Winch: reels 25% faster"),
    (8, "Two more pack slots"),
    (9, "Suit plating: +25 more health"),
    (10, "The sniper, as standard issue at the armory"),
];

pub fn unlocks(rank: u8) -> Unlocks {
    let at = |r: u8| rank >= r;
    Unlocks {
        weapon_slots: items::WEAPON_SLOTS + at(3) as usize,
        pack_slots: items::PACK_SLOTS + 2 * at(4) as usize + 2 * at(8) as usize,
        health: 25.0 * (at(5) as u8 + at(9) as u8) as f32,
        thrust: if at(6) { 1.15 } else { 1.0 },
        reel: if at(7) { 1.25 } else { 1.0 },
    }
}

/// The frames a rank can draw from the armory as standard issue: the SMG
/// and rifle from the start, the shotgun at rank 4, the sniper at rank 10
/// (see [`LADDER`]). Any frame can be found at any rank.
pub fn frames(rank: u8) -> &'static [GunKind] {
    match rank {
        ..4 => &GunKind::CREW[..2],
        4..10 => &GunKind::CREW[..3],
        _ => &GunKind::CREW,
    }
}

/// The crew's tuning as a player of `rank` has it: their suit, jetpack and
/// winch.
pub fn rank_tuning(base: &Tuning, rank: u8) -> Tuning {
    let open = unlocks(rank);
    let mut t = base.clone();
    t.player_health += open.health;
    t.thrust_accel *= open.thrust;
    t.reel_speed *= open.reel;
    t
}

/// A player's rank, public: every peer tunes their body by it (see
/// [`tune_to_rank`]). The rest of a profile goes only to its owner (see
/// `net::profiles`).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rank(pub u8);

/// Authority: a player's rank, published from their profile.
pub fn publish_rank(players: Query<(Entity, &Profile, Option<&Rank>), Changed<Profile>>, mut commands: Commands) {
    for (entity, profile, published) in &players {
        let r = Rank(rank(profile.xp));
        if published != Some(&r) {
            commands.entity(entity).insert(r);
        }
    }
}

/// The rank a player's body was last tuned for.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TunedFor(pub u8);

/// Every peer, before players move: a player with a profile moves by their
/// rank's tuning (as a [`BodyTuning`]: their suit, jetpack and winch), kept
/// up with the crew's tuning and their rank.
pub fn tune_to_rank(
    base: Res<Tuning>,
    players: Query<(Entity, &Rank, Option<&TunedFor>), (With<Player>, Without<Robot>)>,
    mut commands: Commands,
) {
    for (entity, rank, tuned) in &players {
        let r = rank.0;
        if tuned != Some(&TunedFor(r)) || base.is_changed() {
            commands.entity(entity).insert((BodyTuning(rank_tuning(&base, r)), TunedFor(r)));
        }
    }
}

/// Authority: a player's inventory has the slots their rank gives.
pub fn open_slots(mut players: Query<(&mut Player, &Profile), Changed<Profile>>) {
    for (mut player, profile) in &mut players {
        let open = unlocks(rank(profile.xp));
        let inv = &player.loadout.inventory;
        if inv.weapon_slots() < open.weapon_slots || inv.pack_slots() < open.pack_slots {
            player.loadout.inventory.grow(open.weapon_slots, open.pack_slots);
        }
    }
}

/// Authority: at the end of each scenario (when the world it was in goes),
/// guns of a player's still out in it go back to their stash (or are lost:
/// see [`RECOVER_UNCLAIMED`]).
pub fn settle_out(profile: &mut Profile) {
    let out = std::mem::take(&mut profile.out);
    if RECOVER_UNCLAIMED {
        profile.stash.extend(out);
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    #[test]
    fn names_are_cleaned_and_callsigns_are_stable() {
        assert_eq!(clean_name("  Ripley \n"), Some("Ripley".into()));
        assert_eq!(clean_name(" \t "), None);
        assert_eq!(clean_name(&"x".repeat(50)).unwrap().chars().count(), NAME_MAX);
        assert_eq!(callsign(7), callsign(7));
        let calls: std::collections::HashSet<String> = (1..200u64).map(|i| callsign(i.wrapping_mul(0x1234_5678_9abc))).collect();
        assert!(calls.len() > 150, "only {} callsigns", calls.len());
        // An unnamed profile is called its callsign when it's made sound.
        let mut file = ProfileFile::fresh();
        file.profile.id = 99;
        assert_eq!(sanitized(file).profile.name, callsign(99));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::items::Rarity;

    #[test]
    fn a_profile_file_reads_back() {
        let mut file = ProfileFile::fresh();
        file.profile.stash.push(Weapon::new(GunKind::Rifle, 9, Rarity::Rare));
        file.profile.salvage = 340;
        file.carried.take(Item::Weapon(Weapon::new(GunKind::Smg, 4, Rarity::Common)));
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
        let again: ProfileFile = ron::from_str(&text).unwrap();
        assert_eq!(again, file);
    }

    #[test]
    fn a_mission_pays_for_how_it_ended_what_you_did_and_speed() {
        let results = |v: &[(&str, f32)]| v.iter().map(|(k, x)| (k.to_string(), *x)).collect::<Vec<_>>();
        let slow = reward(ScenarioKind::DeadShip, &results(&[("won", 1.0), ("seconds", 1000.0)]), &[], false, "A".into()).unwrap();
        assert_eq!((slow.salvage, slow.xp), pay_for(ScenarioKind::DeadShip).unwrap().won);
        let credits = ["restarted the reactor".to_string()];
        let quick = reward(ScenarioKind::DeadShip, &results(&[("won", 1.0), ("seconds", 60.0), ("core_home", 1.0)]), &credits, true, "A".into()).unwrap();
        let (w, c, h) = (pay_for(ScenarioKind::DeadShip).unwrap().won, CREDIT_PAY, ALL_HOME_PAY);
        let base = w.0 + 100 + c.0 + h.0;
        assert_eq!(quick.salvage, base + base * QUICK_BONUS / 100, "{:#?}", quick.lines);
        assert!(quick.lines.iter().any(|l| l.starts_with("Restarted the reactor")));
        let lost = reward(ScenarioKind::Boarding, &results(&[("won", 0.0), ("seconds", 10.0)]), &[], false, "A".into()).unwrap();
        assert_eq!(lost.salvage, pay_for(ScenarioKind::Boarding).unwrap().lost.0, "a loss got the quick bonus");
        // Recovery pays by the survivor.
        let six = reward(ScenarioKind::Recovery, &results(&[("won", 1.0), ("seconds", 900.0), ("survivors_home", 6.0)]), &[], false, "A".into()).unwrap();
        assert_eq!(six.salvage, pay_for(ScenarioKind::Recovery).unwrap().won.0 + 6 * 25, "{:#?}", six.lines);
        assert!(reward(ScenarioKind::Field, &results(&[("won", 1.0)]), &[], true, "A".into()).is_none());
        assert!(reward(ScenarioKind::Boarding, &[], &[], true, "A".into()).is_none(), "paid undecided");
    }

    #[test]
    fn ranks_come_with_experience_and_each_brings_something() {
        assert_eq!(rank(0), 1);
        assert_eq!(rank(xp_for(2) - 1), 1);
        assert_eq!(rank(xp_for(2)), 2);
        assert_eq!(rank(u32::MAX), MAX_RANK);
        for (at, _) in LADDER {
            assert!(at > 1 && at <= MAX_RANK);
        }
        // Every step of the ladder that's built changes something.
        let all: Vec<Unlocks> = (1..=MAX_RANK).map(unlocks).collect();
        assert_ne!(all[2], all[1], "rank 3");
        assert_eq!(all[2].weapon_slots, 4);
        assert_eq!(all[9].pack_slots, items::MAX_PACK_SLOTS);
        assert!(items::PAINTS.iter().all(|p| p.3 <= MAX_RANK));
    }

    #[test]
    fn a_file_from_a_higher_rank_doesnt_open_slots_it_hasnt_earned() {
        let mut file = ProfileFile::fresh();
        file.carried.slots = (4, 10);
        let gun = Weapon::new(GunKind::Smg, 3, Rarity::Rare);
        file.carried.weapons[3] = Some(gun);
        let file = sanitized(file);
        assert_eq!(file.carried.slots, (3, 6));
        assert!(file.carried.weapons[3].is_none());
        assert_eq!(file.profile.stash, vec![gun], "the fourth gun went missing");
    }

    #[test]
    fn a_saved_profile_has_its_guns_out_in_the_world_back() {
        let mut p = Profile::default();
        let lost = Weapon::new(GunKind::Smg, 1, Rarity::Rare);
        p.out.push(lost);
        let settled = p.settled();
        assert!(settled.out.is_empty());
        assert_eq!(settled.stash.contains(&lost), RECOVER_UNCLAIMED);
    }

    #[test]
    fn an_uploaded_profile_is_made_sound() {
        let mut file = ProfileFile::fresh();
        file.profile.stash.push(Weapon::issue(GunKind::GunnerRifle));
        file.carried.weapons[0] = Some(Weapon { mag: 999, ..Weapon::issue(GunKind::Smg) });
        *file.carried.ammo_mut(AmmoKind::Light) = 60_000;
        let file = sanitized(file);
        assert!(file.profile.stash.is_empty(), "a robot's gun in the stash");
        assert_eq!(file.carried.weapons[0].unwrap().mag, 40);
        assert_eq!(file.carried.ammo(AmmoKind::Light), AmmoKind::Light.cap());
    }
}

// ---------------------------------------------------------------------------
// Mission pay.

/// What a scenario pays each crew member: (salvage, experience) for a win
/// and for a loss, the time to beat (s) for a quick win's bonus, and pay
/// per unit of the scenario's own results (see `Mission::result`): (the
/// result's name, what each unit of it pays, what the debrief calls it).
#[derive(Clone, Copy, Debug)]
pub struct Pay {
    pub won: (u32, u32),
    pub lost: (u32, u32),
    pub par: f32,
    pub per: &'static [(&'static str, (u32, u32), &'static str)],
}

/// The pay table. A scenario that isn't a mission (the field, the gallery)
/// pays nothing.
pub fn pay_for(kind: ScenarioKind) -> Option<Pay> {
    Some(match kind {
        ScenarioKind::Boarding => Pay {
            won: (300, 400),
            lost: (60, 120),
            par: 300.0,
            per: &[("shield_dropped", (50, 60), "Shield dropped"), ("repairs", (20, 30), "Repairs at home")],
        },
        ScenarioKind::Escort => Pay {
            won: (300, 350),
            lost: (50, 100),
            par: 480.0,
            per: &[("cell_home", (100, 100), "Power cell home")],
        },
        ScenarioKind::GrappleRun => Pay {
            won: (200, 260),
            lost: (30, 60),
            par: 180.0,
            per: &[("finished", (15, 20), "Crew through the course")],
        },
        ScenarioKind::DeadShip => Pay {
            won: (250, 350),
            lost: (50, 100),
            par: 420.0,
            per: &[("core_home", (100, 100), "Data core home")],
        },
        ScenarioKind::Recovery => Pay {
            won: (150, 200),
            lost: (30, 60),
            par: 300.0,
            per: &[("survivors_home", (25, 30), "Survivors home")],
        },
        ScenarioKind::Field | ScenarioKind::Gallery | ScenarioKind::Sky => return None,
    })
}

/// Each thing a crew member did that the scenario credited them with (an
/// objective: dropped the shield, turned a key), paid on top.
pub const CREDIT_PAY: (u32, u32) = (25, 40);
/// A win inside the scenario's par time pays this much more (percent).
pub const QUICK_BONUS: u32 = 25;
/// A scenario where getting home is part of it, with everyone home at the
/// end: this to each.
pub const ALL_HOME_PAY: (u32, u32) = (40, 50);

/// One crew member's pay for a mission, for the debrief.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reward {
    pub name: String,
    pub salvage: u32,
    pub xp: u32,
    /// What it was for ("Win: 300", "Dropped the carrier's shield: 25").
    pub lines: Vec<String>,
    /// Their rank after it, and what reaching it brought, if it was new
    /// (from [`LADDER`]).
    pub rank: u8,
    pub new_rank: Vec<String>,
}

/// What a mission pays a crew member, from its results (`won`, `seconds`
/// and the scenario's own; see `Mission::result`): the scenario's pay for
/// how it ended and for its results, plus their credits, a quick win and
/// everyone getting home. Nothing for a mission not decided.
pub fn reward(kind: ScenarioKind, results: &[(String, f32)], credits: &[String], all_home: bool, name: String) -> Option<Reward> {
    let pay = pay_for(kind)?;
    let result = |n: &str| results.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
    let won = result("won")? > 0.5;
    let seconds = result("seconds").unwrap_or(f32::INFINITY);
    let mut items: Vec<((u32, u32), String)> = vec![if won {
        (pay.won, "Won".into())
    } else {
        (pay.lost, "Lost, but paid".into())
    }];
    for (what, (s, x), label) in pay.per {
        let n = result(what).unwrap_or(0.0).max(0.0).round() as u32;
        if n > 0 {
            items.push(((s * n, x * n), if n == 1 { label.to_string() } else { format!("{label} ({n})") }));
        }
    }
    items.extend(credits.iter().map(|c| (CREDIT_PAY, capitalized(c))));
    if all_home && scenarios::scenario(kind).homecoming {
        items.push((ALL_HOME_PAY, "Everyone home".into()));
    }
    let sum = |items: &[((u32, u32), String)]| items.iter().fold((0, 0), |(s, x), ((a, b), _)| (s + a, x + b));
    if won && seconds <= pay.par {
        let (s, x) = sum(&items);
        items.push(((s * QUICK_BONUS / 100, x * QUICK_BONUS / 100), format!("Inside {}", super::mission::clock(pay.par))));
    }
    let (salvage, xp) = sum(&items);
    let lines = items.iter().map(|((s, x), why)| format!("{why}: {s} salvage, {x} xp")).collect();
    Some(Reward {
        name,
        salvage,
        xp,
        lines,
        ..default()
    })
}

fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// Authority: when the world is about to change (a scenario's end, or a
/// switch away from it), every profile is settled: a decided mission pays
/// its crew (written into the mission, for its debrief), and guns of theirs
/// still out in the world go back to their stash.
pub fn settle_missions(
    director: Res<Director>,
    tally: Res<CrewTally>,
    mut mission: Single<&mut Mission>,
    homes: Query<(), With<HomeShip>>,
    mut crew: Query<(Entity, &Player, &mut Profile, Option<&Name>), Without<Robot>>,
) {
    if director.switching().is_none() {
        return;
    }
    let decided = mission.phase != Phase::Underway;
    let aboard = |p: &Player| p.mode.body().is_some_and(|b| homes.contains(b));
    let all_home = crew.iter().all(|(_, p, ..)| aboard(p));
    let mut rewards = Vec::new();
    for (entity, _, mut profile, name) in &mut crew {
        settle_out(&mut profile);
        if !decided {
            continue;
        }
        let credits = tally.0.get(&entity).map_or(&[][..], |s| &s.credits[..]);
        let name = name.map_or("?".to_string(), |n| n.to_string());
        if let Some(mut r) = reward(mission.kind, &mission.results, credits, all_home, name) {
            let before = rank(profile.xp);
            profile.salvage = profile.salvage.saturating_add(r.salvage);
            profile.xp = profile.xp.saturating_add(r.xp);
            profile.missions += 1;
            r.rank = rank(profile.xp);
            r.new_rank = LADDER
                .iter()
                .filter(|(at, _)| *at > before && *at <= r.rank)
                .map(|(at, what)| format!("Rank {at}: {what}"))
                .collect();
            rewards.push(r);
        }
    }
    rewards.sort_by(|a, b| a.name.cmp(&b.name));
    if !rewards.is_empty() {
        mission.rewards = rewards;
    }
}
