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

use super::items::{AmmoKind, Inventory, Item, Place, Rarity, Weapon};
use super::bots::Robot;
use super::mission::{CrewTally, Director, Mission, Phase, ScenarioKind};
use super::scenarios;
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

    /// The file for a player with `profile`, carrying `carried`.
    pub fn of(profile: &Profile, carried: &Inventory) -> ProfileFile {
        ProfileFile {
            version: Self::VERSION,
            profile: profile.settled(),
            carried: *carried,
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
}

/// Something asked of a profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProfileOp {
    /// The armory: a gun from the stash into the inventory (the first free
    /// place, as a pickup would go).
    Take(u16),
    /// The armory: a standard-issue gun of a frame, free.
    TakeIssue(GunKind),
    /// The armory: the gun at this place in the inventory into the stash
    /// (standard issue just goes back on the rack).
    Store(Place),
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
    for kind in AmmoKind::ALL {
        let pool = inv.ammo_mut(kind);
        *pool = (*pool).min(kind.cap());
    }
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
                commands.entity(entity).insert(file.profile);
            }
            (ProfileWire::Upload(_), Some(_)) => {}
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
        ProfileOp::Take(i) => match profile.stash.get(i as usize) {
            Some(&w) if inv.take(Item::Weapon(w)) => {
                profile.stash.remove(i as usize);
                true
            }
            _ => false,
        },
        ProfileOp::TakeIssue(frame) => GunKind::CREW.contains(&frame) && inv.take(Item::Weapon(Weapon::issue(frame))),
        ProfileOp::Store(place) => match inv.get(place) {
            Some(Item::Weapon(w)) if w.rarity == Rarity::Standard => inv.remove(place).is_some(),
            Some(Item::Weapon(w)) if profile.stash.len() < STASH_CAP => {
                inv.remove(place);
                profile.stash.push(w);
                true
            }
            _ => false,
        },
    };
    player.loadout.hands_changed(before);
    done
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
        let slow = reward(ScenarioKind::DeadShip, Phase::Won, 1000.0, &[], false, "A".into()).unwrap();
        assert_eq!((slow.salvage, slow.xp), pay_for(ScenarioKind::DeadShip).unwrap().won);
        let credits = ["restarted the reactor".to_string()];
        let quick = reward(ScenarioKind::DeadShip, Phase::Won, 60.0, &credits, true, "A".into()).unwrap();
        let (w, c, h) = (pay_for(ScenarioKind::DeadShip).unwrap().won, CREDIT_PAY, ALL_HOME_PAY);
        let base = w.0 + c.0 + h.0;
        assert_eq!(quick.salvage, base + base * QUICK_BONUS / 100, "{:#?}", quick.lines);
        assert!(quick.lines.iter().any(|l| l.starts_with("Restarted the reactor")));
        let lost = reward(ScenarioKind::Boarding, Phase::Lost, 10.0, &[], false, "A".into()).unwrap();
        assert_eq!(lost.salvage, pay_for(ScenarioKind::Boarding).unwrap().lost.0, "a loss got the quick bonus");
        assert!(reward(ScenarioKind::Field, Phase::Won, 1.0, &[], true, "A".into()).is_none());
        assert!(reward(ScenarioKind::Boarding, Phase::Underway, 1.0, &[], true, "A".into()).is_none());
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
/// and for a loss, and the time to beat (s) for a quick win's bonus.
#[derive(Clone, Copy, Debug)]
pub struct Pay {
    pub won: (u32, u32),
    pub lost: (u32, u32),
    pub par: f32,
}

/// The pay table. A scenario that isn't a mission (the field, the gallery)
/// pays nothing.
pub fn pay_for(kind: ScenarioKind) -> Option<Pay> {
    Some(match kind {
        ScenarioKind::Boarding => Pay { won: (300, 400), lost: (60, 120), par: 300.0 },
        ScenarioKind::GrappleRun => Pay { won: (150, 200), lost: (30, 60), par: 150.0 },
        ScenarioKind::DeadShip => Pay { won: (250, 350), lost: (50, 100), par: 420.0 },
        ScenarioKind::Field | ScenarioKind::Gallery => return None,
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
}

/// What a mission pays a crew member: the scenario's pay for how it
/// ended, plus their credits, a quick win and everyone getting home.
pub fn reward(kind: ScenarioKind, phase: Phase, seconds: f32, credits: &[String], all_home: bool, name: String) -> Option<Reward> {
    let pay = pay_for(kind)?;
    let mut items: Vec<((u32, u32), String)> = vec![match phase {
        Phase::Won => (pay.won, "Won".into()),
        Phase::Lost => (pay.lost, "Lost, but paid".into()),
        Phase::Underway => return None,
    }];
    items.extend(credits.iter().map(|c| (CREDIT_PAY, capitalized(c))));
    if all_home && scenarios::scenario(kind).homecoming {
        items.push((ALL_HOME_PAY, "Everyone home".into()));
    }
    let sum = |items: &[((u32, u32), String)]| items.iter().fold((0, 0), |(s, x), ((a, b), _)| (s + a, x + b));
    if phase == Phase::Won && seconds <= pay.par {
        let (s, x) = sum(&items);
        items.push(((s * QUICK_BONUS / 100, x * QUICK_BONUS / 100), format!("Inside {}", super::mission::clock(pay.par))));
    }
    let (salvage, xp) = sum(&items);
    let lines = items.iter().map(|((s, x), why)| format!("{why}: {s} salvage, {x} xp")).collect();
    Some(Reward { name, salvage, xp, lines })
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
    let seconds = mission.ended.unwrap_or(mission.started).wrapping_sub(mission.started) as f32 / super::TICK_HZ as f32;
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
        if let Some(r) = reward(mission.kind, mission.phase, seconds, credits, all_home, name) {
            profile.salvage += r.salvage;
            profile.xp += r.xp;
            profile.missions += 1;
            rewards.push(r);
        }
    }
    rewards.sort_by(|a, b| a.name.cmp(&b.name));
    if !rewards.is_empty() {
        mission.rewards = rewards;
    }
}
