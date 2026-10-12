//! What the HUD shows of the player's kit, read from the sim in one place.
//!
//! The HUD draws hands, stances and pickups from these small views, never
//! from the loadout directly, so when what players carry changes (weapons
//! as items, an inventory), only the functions here change.

use crate::sim::pickups::Pickup;
use crate::sim::player::Player;
use crate::sim::tuning::Tuning;
use crate::sim::weapons::{GunKind, Hand, Slot};

/// How rare a weapon is; its card's accent (see `style::RARITY`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub fn label(self) -> &'static str {
        match self {
            Rarity::Common => "Common",
            Rarity::Uncommon => "Uncommon",
            Rarity::Rare => "Rare",
            Rarity::Epic => "Epic",
            Rarity::Legendary => "Legendary",
        }
    }
}

/// What a hand holds, as far as its card cares.
#[derive(Clone, Debug, PartialEq)]
pub enum HandItem {
    Weapon,
    /// The grapple's launcher; `line` is how much rope is out (m), of `range`.
    Grapple { line: Option<f32>, range: f32 },
    /// A stance slot whose weapon isn't carried.
    Missing,
    Empty,
}

/// Something worth flagging on a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandTag {
    Reloading,
    /// Waiting for the other hand's reload.
    Queued,
    /// Nothing in the mag or in reserve.
    Dry,
    /// Under a quarter of a mag.
    Low,
    /// Steadied by the free other hand.
    Braced,
}

/// One hand's card.
#[derive(Clone, Debug, PartialEq)]
pub struct HandView {
    pub hand: Hand,
    pub item: HandItem,
    pub name: String,
    pub rarity: Option<Rarity>,
    /// Rounds in the mag, and what it holds.
    pub mag: Option<(u16, u16)>,
    pub reserve: Option<u16>,
    /// How far through a reload (0..1), if reloading.
    pub reload: Option<f32>,
    pub tags: Vec<HandTag>,
}

/// The cards for both hands, `[main, off]`.
pub fn hand_views(player: &Player, tuning: &Tuning) -> [HandView; 2] {
    Hand::BOTH.map(|hand| hand_view(player, tuning, hand))
}

fn hand_view(player: &Player, tuning: &Tuning, hand: Hand) -> HandView {
    let loadout = &player.loadout;
    let slot = loadout.stance().slot(hand);
    let mut view = HandView {
        hand,
        item: HandItem::Empty,
        name: String::new(),
        rarity: None,
        mag: None,
        reserve: None,
        reload: None,
        tags: Vec::new(),
    };
    match slot {
        Slot::Empty => {}
        Slot::Grapple => {
            view.item = HandItem::Grapple {
                line: player.grapple.map(|g| g.length),
                range: tuning.grapple_range,
            };
            view.name = "Grapple".into();
        }
        Slot::Gun(i) => {
            let Some(slot_gun) = loadout.guns.get(i as usize) else {
                return view;
            };
            view.name = slot_gun.kind.stats().name.into();
            let Some(gun) = loadout.gun(hand) else {
                view.item = HandItem::Missing;
                return view;
            };
            let stats = gun.kind.stats();
            let state = loadout.hands[hand as usize];
            view.item = HandItem::Weapon;
            view.rarity = Some(Rarity::Common);
            view.mag = Some((gun.mag, stats.mag));
            view.reserve = Some(gun.reserve);
            view.reload = state.reloading.map(|left| 1.0 - (left / stats.reload).clamp(0.0, 1.0));
            if view.reload.is_some() {
                view.tags.push(HandTag::Reloading);
            } else if state.reload_queued {
                view.tags.push(HandTag::Queued);
            }
            if gun.mag == 0 && gun.reserve == 0 {
                view.tags.push(HandTag::Dry);
            } else if (gun.mag as f32) < stats.mag as f32 * 0.25 {
                view.tags.push(HandTag::Low);
            }
            if loadout.braced(hand) {
                view.tags.push(HandTag::Braced);
            }
        }
    }
    view
}

/// A stance as its chip shows it: short names for what each hand holds.
#[derive(Clone, Debug, PartialEq)]
pub struct StanceView {
    pub main: &'static str,
    pub off: &'static str,
}

pub fn stance_views(player: &Player) -> Vec<StanceView> {
    let loadout = &player.loadout;
    let short = |slot: Slot| match slot {
        Slot::Empty => "—",
        Slot::Grapple => "GRP",
        Slot::Gun(i) => match loadout.guns.get(i as usize) {
            Some(g) if g.carried => match g.kind {
                GunKind::Smg => "SMG",
                GunKind::Rifle => "RFL",
                _ => "GUN",
            },
            _ => "—",
        },
    };
    loadout
        .stances
        .iter()
        .map(|s| StanceView {
            main: short(s.main),
            off: short(s.off),
        })
        .collect()
}

/// Whether the player carries any weapon (or anything that would be left
/// behind on death).
pub fn armed(player: &Player) -> bool {
    player.loadout.guns.iter().any(|g| g.carried)
}

/// What looking at a pickup offers.
pub fn pickup_label(pickup: &Pickup) -> String {
    let names: Vec<&str> = pickup.guns.iter().map(|g| g.kind.stats().name).collect();
    format!("Pick up: {}", names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::events::EventLog;
    use crate::sim::input::InputCursor;
    use crate::sim::player::Mode;
    use crate::sim::weapons::Loadout;
    use bevy::prelude::*;

    fn player(loadout: Loadout) -> Player {
        Player {
            mode: Mode::Eva {
                position: Vec3::ZERO,
                velocity: Vec3::ZERO,
                orientation: Quat::IDENTITY,
                pitch: 0.0,
            },
            boots_on: true,
            flight_assist: true,
            grapple: None,
            reference_body: None,
            detach_timer: 0.0,
            loadout,
            input: InputCursor::default(),
            events: EventLog::default(),
        }
    }

    #[test]
    fn the_full_kit_shows_a_gun_and_the_grapple() {
        let [main, off] = hand_views(&player(Loadout::default()), &Tuning::default());
        assert_eq!(main.item, HandItem::Weapon);
        assert_eq!(main.name, "SMG");
        assert_eq!(main.mag, Some((40, 40)));
        assert!(main.tags.is_empty(), "{:?}", main.tags);
        assert!(matches!(off.item, HandItem::Grapple { line: None, .. }));
    }

    #[test]
    fn a_gun_alone_is_braced_and_flags_a_low_mag() {
        let mut loadout = Loadout::default();
        loadout.current = 2;
        loadout.guns[1].mag = 3;
        let [main, off] = hand_views(&player(loadout), &Tuning::default());
        assert_eq!(main.tags, vec![HandTag::Low, HandTag::Braced]);
        assert_eq!(off.item, HandItem::Empty);
    }

    #[test]
    fn the_base_kit_is_unarmed_with_its_gun_slots_missing() {
        let mut loadout = Loadout::base();
        loadout.current = 0;
        let p = player(loadout);
        assert!(!armed(&p));
        let [main, _] = hand_views(&p, &Tuning::default());
        assert_eq!(main.item, HandItem::Missing);
        assert_eq!(stance_views(&p)[0].main, "—");
    }
}
