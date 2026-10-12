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

use super::items::{AmmoKind, Inventory, Item, Weapon};
use super::player::Player;
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
pub enum ProfileOp {}

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
            (ProfileWire::Op(op), Some(mut profile)) => apply(op, &mut player, &mut profile),
            (ProfileWire::Op(_), None) => {}
        }
    }
}

fn apply(op: ProfileOp, _player: &mut Player, _profile: &mut Profile) {
    match op {}
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
