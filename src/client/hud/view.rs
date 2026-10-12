//! What the HUD shows of the player's kit, read from the sim in one place.
//!
//! The HUD draws hands, stances and pickups from these small views, never
//! from the loadout directly, so when what players carry changes, only the
//! functions here change.

use crate::sim::items::Rarity;
use crate::sim::pickups::Pickup;
use crate::sim::player::Player;
use crate::sim::tuning::Tuning;
use crate::sim::weapons::{GunKind, Hand, Slot};

/// What a hand holds, as far as its card cares.
#[derive(Clone, Debug, PartialEq)]
pub enum HandItem {
    Weapon,
    /// The grapple's launcher; `line` is how much rope is out (m), of `range`.
    Grapple { line: Option<f32>, range: f32 },
    /// A stance's weapon slot with no gun in it.
    Missing,
    /// Bulky cargo fills both hands.
    Cargo,
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
    /// Zoomed (the other hand's button held, that hand free).
    Zoomed,
    /// A laser too hot to fire, cooling.
    Overheated,
}

/// One hand's card.
#[derive(Clone, Debug, PartialEq)]
pub struct HandView {
    pub hand: Hand,
    pub item: HandItem,
    /// The name, as long as it is ("Halvard Mk 3 Longshot Auto-Rifle").
    pub name: String,
    /// A line under it: what it is ("Rare rifle", "Suit system").
    pub detail: String,
    pub rarity: Option<Rarity>,
    /// Rounds in the mag, and what it holds.
    pub mag: Option<(u16, u16)>,
    pub reserve: Option<u16>,
    /// How far through a reload (0..1), if reloading.
    pub reload: Option<f32>,
    /// A laser's heat (0..1), instead of a mag.
    pub heat: Option<f32>,
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
        detail: String::new(),
        rarity: None,
        mag: None,
        reserve: None,
        reload: None,
        heat: None,
        tags: Vec::new(),
    };
    if let Some(cargo) = loadout.inventory.carrying {
        view.item = HandItem::Cargo;
        view.name = cargo.name().into();
        view.detail = "Hands full".into();
        return view;
    }
    match slot {
        Slot::Empty => {}
        Slot::Grapple => {
            view.item = HandItem::Grapple {
                line: player.grapple.map(|g| g.length),
                range: tuning.grapple_range,
            };
            view.name = "Grapple".into();
            view.detail = "Suit system".into();
        }
        Slot::Gun(i) => {
            let Some(weapon) = loadout.weapon(hand) else {
                view.item = HandItem::Missing;
                view.name = format!("Weapon slot {}", i + 1);
                view.detail = "Empty slot".into();
                return view;
            };
            let stats = weapon.stats();
            let reserve = loadout.reserve(hand).unwrap_or(0);
            view.item = HandItem::Weapon;
            view.name = weapon.name();
            view.detail = format!("{} {}", weapon.rarity.name(), weapon.kind_name().to_lowercase());
            view.rarity = Some(weapon.rarity);
            view.reload = loadout.reload_progress(hand);
            let state = loadout.hands[hand as usize];
            if stats.heat > 0.0 {
                // A laser: heat, not rounds.
                view.heat = Some(state.heat);
                if state.overheated {
                    view.tags.push(HandTag::Overheated);
                }
                if loadout.zoom > 1.0 {
                    view.tags.push(HandTag::Zoomed);
                } else if loadout.braced(hand) {
                    view.tags.push(HandTag::Braced);
                }
                return view;
            }
            view.mag = Some((weapon.mag, stats.mag));
            view.reserve = Some(reserve);
            if view.reload.is_some() {
                view.tags.push(HandTag::Reloading);
            } else if loadout.hands[hand as usize].reload_queued {
                view.tags.push(HandTag::Queued);
            }
            if weapon.mag == 0 && reserve == 0 {
                view.tags.push(HandTag::Dry);
            } else if (weapon.mag as f32) < stats.mag as f32 * 0.25 {
                view.tags.push(HandTag::Low);
            }
            if loadout.zoom > 1.0 && stats.zoom > 0.0 {
                view.tags.push(HandTag::Zoomed);
            } else if loadout.braced(hand) {
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
        Slot::Gun(i) => match loadout.inventory.weapons.get(i as usize).copied().flatten() {
            Some(g) => match g.frame {
                GunKind::Smg => "SMG",
                GunKind::Rifle => "RFL",
                GunKind::Shotgun => "SHG",
                GunKind::Sniper => "SNP",
                _ => "GUN",
            },
            None => "—",
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

/// Whether the player has any weapon in their weapon slots.
pub fn armed(player: &Player) -> bool {
    player.loadout.inventory.weapons.iter().any(Option::is_some)
}

/// What looking at a pickup offers.
pub fn pickup_label(pickup: &Pickup) -> String {
    pickup.label()
}

/// Whether dying now would leave a case behind: anything in the weapon
/// slots, the pack, the ammo pools or the hands.
pub fn carries_anything(player: &Player) -> bool {
    let inv = &player.loadout.inventory;
    inv.weapons.iter().any(Option::is_some) || inv.pack.iter().any(Option::is_some) || inv.ammo.iter().any(|&a| a > 0) || inv.carrying.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::events::EventLog;
    use crate::sim::input::InputCursor;
    use crate::sim::items::{CargoKind, Weapon};
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
        assert_eq!(main.rarity, Some(Rarity::Standard));
        assert!(main.mag.is_some_and(|(n, of)| n == of && of > 0));
        assert!(main.tags.is_empty(), "{:?}", main.tags);
        assert!(matches!(off.item, HandItem::Grapple { line: None, .. }));
    }

    #[test]
    fn a_generated_gun_shows_its_name_and_rarity() {
        let mut loadout = Loadout::default();
        let gun = Weapon::new(GunKind::Rifle, 7, Rarity::Exotic);
        loadout.inventory.weapons[0] = Some(Weapon { mag: 1, ..gun });
        let [main, _] = hand_views(&player(loadout), &Tuning::default());
        assert_eq!(main.name, gun.name());
        assert_eq!(main.rarity, Some(Rarity::Exotic));
        assert!(main.tags.contains(&HandTag::Low));
    }

    #[test]
    fn a_gun_alone_is_braced() {
        let mut loadout = Loadout::default();
        loadout.current = 2;
        let [main, off] = hand_views(&player(loadout), &Tuning::default());
        assert_eq!(main.tags, vec![HandTag::Braced]);
        assert_eq!(off.item, HandItem::Empty);
    }

    #[test]
    fn the_base_kit_is_unarmed_with_its_gun_slots_missing() {
        let mut loadout = Loadout::base();
        loadout.current = 0;
        let p = player(loadout);
        assert!(!armed(&p));
        assert!(!carries_anything(&p));
        let [main, _] = hand_views(&p, &Tuning::default());
        assert_eq!(main.item, HandItem::Missing);
        assert_eq!(stance_views(&p)[0].main, "—");
    }

    #[test]
    fn bulky_cargo_fills_both_hands() {
        let mut loadout = Loadout::default();
        loadout.inventory.carrying = Some(CargoKind::PowerCell);
        let views = hand_views(&player(loadout), &Tuning::default());
        assert!(views.iter().all(|v| v.item == HandItem::Cargo));
    }
}
