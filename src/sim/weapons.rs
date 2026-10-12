//! What players hold: two hands, stances, and guns.
//!
//! A stance is what each hand holds: the main hand is used with the primary
//! button, the off hand with the secondary. Stances point at the
//! inventory's weapon *slots* by position (see [`super::items::Inventory`]),
//! so the guns in them can change. Switching stances takes a moment: as
//! long as the slowest gun coming up takes. The grapple is a suit system,
//! but its launcher takes a hand; in a stance without it, a deployed rope
//! stays as it is (no reeling, no letting go). Bulky cargo takes both hands:
//! no guns, no grapple, until it's put down.
//!
//! A one-handed gun with the other hand free is braced: it shoots tighter.
//! A hand that runs dry reloads itself, from the inventory's pool of its
//! ammo; a reload of everything goes main hand first. Firing a hand cancels
//! its reload if there's anything left in the mag.
//!
//! All of this is part of the predicted player state, so firing and
//! reloading respond at once. Shots leave as [`super::events::PlayerEvent::Fired`]
//! entries; the authority flies them (see [`super::shots`]).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::input::TickInput;
use super::items::{AmmoKind, Inventory, Item, Perk, WEAPON_SLOTS, Weapon};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GunKind {
    Smg,
    Rifle,
    /// Robots' guns: weaker than the crew's, since robots come in numbers.
    GunnerRifle,
    SwarmerSprayer,
}

/// How a gun behaves: a frame's base stats, or a weapon's rolled ones (see
/// [`Weapon::stats`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GunStats {
    /// Shots per second while the button is held.
    pub rate: f32,
    /// Muzzle speed (m/s), on top of the shooter's own velocity.
    pub muzzle_speed: f32,
    pub damage: f32,
    /// Cone half-angle (degrees), braced and not.
    pub spread_braced: f32,
    pub spread: f32,
    pub mag: u16,
    /// Seconds to reload.
    pub reload: f32,
    /// Bullet mass (kg): its momentum is the shooter's recoil and what it
    /// imparts to what it hits.
    pub bullet_mass: f32,
    /// Seconds to bring it up in a stance change.
    pub switch: f32,
    /// What it fires, drawn from the inventory's pool of it.
    pub ammo: AmmoKind,
    pub perks: [Option<Perk>; 2],
}

impl GunStats {
    pub fn has(&self, perk: Perk) -> bool {
        self.perks.contains(&Some(perk))
    }
}

impl GunKind {
    /// The frames the crew's guns are built on (what loot rolls).
    pub const CREW: [GunKind; 2] = [GunKind::Smg, GunKind::Rifle];

    pub fn name(self) -> &'static str {
        match self {
            GunKind::Smg => "SMG",
            GunKind::Rifle => "Rifle",
            GunKind::GunnerRifle => "Gunner rifle",
            GunKind::SwarmerSprayer => "Sprayer",
        }
    }

    /// The frame's base stats: what a standard-issue gun (or a robot's) has.
    #[allow(clippy::too_many_arguments)]
    pub fn base(self) -> GunStats {
        let stats = |rate, muzzle_speed, damage, spread_braced, spread, mag, reload, bullet_mass, ammo| GunStats {
            rate,
            muzzle_speed,
            damage,
            spread_braced,
            spread,
            mag,
            reload,
            bullet_mass,
            switch: SWITCH_SECONDS,
            ammo,
            perks: [None; 2],
        };
        match self {
            GunKind::Smg => stats(15.0, 380.0, 9.0, 1.2, 2.0, 40, 1.6, 0.008, AmmoKind::Light),
            GunKind::Rifle => stats(7.5, 800.0, 24.0, 0.35, 2.5, 30, 2.0, 0.012, AmmoKind::Heavy),
            GunKind::GunnerRifle => stats(5.0, 450.0, 8.0, 1.0, 1.0, 20, 2.0, 0.01, AmmoKind::Heavy),
            GunKind::SwarmerSprayer => stats(10.0, 300.0, 4.0, 3.0, 3.0, 30, 1.5, 0.005, AmmoKind::Light),
        }
    }
}

/// What a hand holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum Slot {
    Empty,
    Grapple,
    /// Whatever gun is in this inventory weapon slot.
    Gun(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hand {
    Main,
    Off,
}

impl Hand {
    pub const BOTH: [Hand; 2] = [Hand::Main, Hand::Off];

    fn index(self) -> usize {
        self as usize
    }

    fn other(self) -> Hand {
        match self {
            Hand::Main => Hand::Off,
            Hand::Off => Hand::Main,
        }
    }
}

/// What each hand holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct Stance {
    pub main: Slot,
    pub off: Slot,
}

impl Stance {
    pub fn slot(&self, hand: Hand) -> Slot {
        match hand {
            Hand::Main => self.main,
            Hand::Off => self.off,
        }
    }

    /// The stance as it can be held: no slot that isn't there, and nothing
    /// in both hands at once.
    fn sanitized(self) -> Stance {
        let fit = |slot| match slot {
            Slot::Gun(i) if i as usize >= WEAPON_SLOTS => Slot::Empty,
            slot => slot,
        };
        let (main, off) = (fit(self.main), fit(self.off));
        Stance {
            main,
            off: if off == main { Slot::Empty } else { off },
        }
    }
}

/// The stances a new player starts with (see [`Loadout::default`]).
pub const DEFAULT_STANCES: [Stance; STANCES] = [
    Stance { main: Slot::Gun(0), off: Slot::Grapple },
    Stance { main: Slot::Gun(1), off: Slot::Grapple },
    Stance { main: Slot::Gun(1), off: Slot::Empty },
    Stance { main: Slot::Grapple, off: Slot::Empty },
];

pub const STANCES: usize = 4;
/// Seconds to bring up a standard gun.
pub const SWITCH_SECONDS: f32 = 0.4;
/// Seconds for a stance change with no gun coming up.
pub const MIN_SWITCH_SECONDS: f32 = 0.25;

/// A hand's state: time to its next shot, and a reload in progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HandState {
    pub cooldown: f32,
    /// Seconds left of a reload, if reloading.
    pub reloading: Option<f32>,
    /// Waiting for the other hand to finish reloading before starting.
    pub reload_queued: bool,
    /// Shots this hand has fired (for perks that count them).
    pub fired: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loadout {
    /// What's carried: the stances' weapon slots, the pack, ammo, cargo.
    pub inventory: Inventory,
    pub stances: [Stance; STANCES],
    pub current: u8,
    /// Seconds until the current stance is ready, after a switch, and how
    /// long that switch takes in all.
    pub switching: f32,
    pub switch_time: f32,
    pub hands: [HandState; 2],
    /// Shots fired so far; seeds each shot's spread.
    pub shots: u32,
}

impl Loadout {
    /// What the crew start (and respawn) with: the suit and its grapple,
    /// in the grapple stance. Guns come from the ship's weapons locker.
    pub fn base() -> Loadout {
        Loadout {
            inventory: Inventory::default(),
            stances: DEFAULT_STANCES,
            current: 3,
            switching: 0.0,
            switch_time: 0.0,
            hands: [HandState::default(); 2],
            shots: 0,
        }
    }

    /// A robot's: its one gun, in the main hand of every stance, and all
    /// the ammo it will ever fire.
    pub fn robot(frame: GunKind) -> Loadout {
        let mut l = Loadout::base();
        l.inventory.weapons[0] = Some(Weapon::issue(frame));
        *l.inventory.ammo_mut(frame.base().ammo) = u16::MAX;
        l.stances = [Stance { main: Slot::Gun(0), off: Slot::Empty }; STANCES];
        l.current = 0;
        l
    }

    /// The weapons locker: a standard-issue SMG and rifle, unless one of
    /// that frame is carried already (each to its usual slot if that's
    /// free), every ammo pool full, and every mag.
    pub fn restock(&mut self) {
        let before = self.held();
        for (frame, slot) in [(GunKind::Smg, 0), (GunKind::Rifle, 1)] {
            if self.inventory.has_frame(frame) {
                continue;
            }
            let issue = Weapon::issue(frame);
            if self.inventory.weapons[slot].is_none() {
                self.inventory.weapons[slot] = Some(issue);
            } else {
                self.inventory.take(Item::Weapon(issue));
            }
        }
        // A gun just issued into a hand comes up; topping up mags is no
        // reason to lower the guns already there.
        self.hands_changed(before);
        for kind in AmmoKind::ALL {
            let pool = self.inventory.ammo_mut(kind);
            *pool = (*pool).max(kind.cap());
        }
        for weapon in self.inventory.all_weapons_mut() {
            weapon.mag = weapon.stats().mag;
        }
    }

    /// What's in hand now (guns as they are, and whether cargo fills both
    /// hands), to see whether an inventory change touched it.
    pub fn held(&self) -> ([Option<Weapon>; 2], bool) {
        (Hand::BOTH.map(|h| self.weapon(h).copied()), self.inventory.carrying.is_some())
    }

    /// After the inventory changed: if what's in hand isn't as `before`
    /// had it, the hands bring up what's there now (a switch). That counts
    /// a gun just like the old one but with another mag: swapping an empty
    /// gun for a full twin is no faster than a reload.
    pub fn hands_changed(&mut self, before: ([Option<Weapon>; 2], bool)) {
        if self.held() != before {
            self.begin_switch();
        }
    }

    fn begin_switch(&mut self) {
        self.switch_time = self.stance_switch_time();
        self.switching = self.switch_time;
        self.cancel_reloads();
    }

    /// How long the current stance takes to bring up: its slowest gun.
    fn stance_switch_time(&self) -> f32 {
        Hand::BOTH
            .iter()
            .filter_map(|h| self.weapon(*h))
            .map(|w| w.stats().switch)
            .fold(MIN_SWITCH_SECONDS, f32::max)
    }

    /// How much of a stance switch is left (1 just begun, 0 ready).
    pub fn switch_fraction(&self) -> f32 {
        if self.switch_time > 0.0 {
            (self.switching / self.switch_time).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Bulky cargo fills both hands.
    pub fn hands_full(&self) -> bool {
        self.inventory.carrying.is_some()
    }
}

impl Default for Loadout {
    /// The full kit, as from the weapons locker: an SMG and a rifle, with
    /// the default stances.
    ///
    /// 1. SMG and grapple; 2. rifle and grapple; 3. rifle, braced;
    /// 4. grapple alone, on the primary button.
    fn default() -> Loadout {
        let mut l = Loadout::base();
        l.restock();
        l.current = 0;
        l.switching = 0.0;
        l
    }
}

/// One shot: which hand, what gun, how hard, and how far off the crosshair
/// (a unit offset in the view plane, scaled by the cone).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotOut {
    pub hand: Hand,
    /// The gun's frame (for how it looks and sounds).
    pub gun: GunKind,
    pub damage: f32,
    pub muzzle_speed: f32,
    pub bullet_mass: f32,
    /// Tangent of the angle off the crosshair, in view x and y.
    pub deviation: Vec2,
}

/// What the hands did this tick.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandsOut {
    /// The grapple's trigger was pulled (fire, or let go).
    pub grapple_trigger: bool,
    pub reel_in: bool,
    pub pay_out: bool,
    /// Whether the grapple's launcher is in hand at all.
    pub grapple_held: bool,
    pub shots: Vec<ShotOut>,
}

impl Loadout {
    pub fn stance(&self) -> Stance {
        self.stances[self.current as usize]
    }

    /// The gun in a hand, if it holds one.
    pub fn weapon(&self, hand: Hand) -> Option<&Weapon> {
        self.weapon_slot(hand).and_then(|i| self.inventory.weapons[i].as_ref())
    }

    /// The weapon slot a hand's gun is in.
    pub fn weapon_slot(&self, hand: Hand) -> Option<usize> {
        if self.hands_full() {
            return None;
        }
        match self.stance().slot(hand) {
            Slot::Gun(i) if self.inventory.weapons.get(i as usize).is_some_and(Option::is_some) => Some(i as usize),
            _ => None,
        }
    }

    /// The rounds left in the pool a hand's gun draws on.
    pub fn reserve(&self, hand: Hand) -> Option<u16> {
        self.weapon(hand).map(|w| self.inventory.ammo(w.stats().ammo))
    }

    /// How far along a hand's reload is (0..1), if it's reloading.
    pub fn reload_progress(&self, hand: Hand) -> Option<f32> {
        let left = self.hands[hand.index()].reloading?;
        let total = self.weapon(hand)?.stats().reload;
        Some(1.0 - (left / total).clamp(0.0, 1.0))
    }

    /// Whether a hand's gun is steadied by the other hand (empty, or
    /// holding a weapon slot with no gun in it).
    pub fn braced(&self, hand: Hand) -> bool {
        match self.stance().slot(hand.other()) {
            Slot::Empty => true,
            Slot::Gun(_) => self.weapon_slot(hand.other()).is_none(),
            Slot::Grapple => false,
        }
    }

    fn gun_mut(&mut self, i: usize) -> &mut Weapon {
        self.inventory.weapons[i].as_mut().expect("a gun in the slot")
    }

    /// Whether `hand`'s gun could take a reload: not full, and rounds in
    /// the pool.
    fn wants_reload(&self, hand: Hand) -> bool {
        self.weapon(hand).is_some_and(|w| {
            let stats = w.stats();
            w.mag < stats.mag && self.inventory.ammo(stats.ammo) > 0
        })
    }

    fn start_reload(&mut self, hand: Hand) -> bool {
        if !self.wants_reload(hand) {
            // Nothing to load (the other hand took the last of the pool):
            // it isn't waiting for anything.
            self.hands[hand.index()].reload_queued = false;
            return false;
        }
        let reload = self.weapon(hand).expect("a gun").stats().reload;
        let state = &mut self.hands[hand.index()];
        if state.reloading.is_some() {
            return false;
        }
        state.reloading = Some(reload);
        state.reload_queued = false;
        true
    }

    fn finish_reload(&mut self, hand: Hand) {
        if let Some(i) = self.weapon_slot(hand) {
            let stats = self.inventory.weapons[i].expect("a gun").stats();
            let pool = self.inventory.ammo(stats.ammo);
            let gun = self.gun_mut(i);
            let take = (stats.mag.saturating_sub(gun.mag)).min(pool);
            gun.mag += take;
            *self.inventory.ammo_mut(stats.ammo) -= take;
        }
        self.hands[hand.index()].reloading = None;
    }

    fn cancel_reloads(&mut self) {
        for state in &mut self.hands {
            state.reloading = None;
            state.reload_queued = false;
        }
    }

    /// Takes up a player's own choice of what each stance holds. A change
    /// to what's in hand now takes as long as a stance switch.
    pub fn set_stances(&mut self, wanted: &[Stance; STANCES]) {
        let wanted = wanted.map(Stance::sanitized);
        if wanted == self.stances {
            return;
        }
        let before = self.stance();
        self.stances = wanted;
        if self.stance() != before {
            self.begin_switch();
        }
    }

    /// Runs the hands for one tick of `dt` seconds.
    pub fn tick(&mut self, cmd: &TickInput, dt: f32) -> HandsOut {
        let mut out = HandsOut::default();
        if let Some(stances) = &cmd.stances {
            self.set_stances(stances);
        }
        let wanted = cmd.stance.min(STANCES as u8 - 1);
        if cmd.stance_pressed && wanted != self.current {
            self.current = wanted;
            self.begin_switch();
        }
        let stance = self.stance();
        let full = self.hands_full();
        out.grapple_held = !full && Hand::BOTH.iter().any(|h| stance.slot(*h) == Slot::Grapple);
        if self.switching > 0.0 || full {
            self.switching = (self.switching - dt).max(0.0);
            for state in &mut self.hands {
                state.cooldown = (state.cooldown - dt).max(0.0);
            }
            return out;
        }
        // Reloads asked for.
        if cmd.reload {
            let main = self.start_reload(Hand::Main);
            if main || self.hands[Hand::Main.index()].reloading.is_some() {
                // The off hand waits its turn.
                let off = Hand::Off;
                if self.wants_reload(off) && self.hands[off.index()].reloading.is_none() {
                    self.hands[off.index()].reload_queued = true;
                }
            } else {
                self.start_reload(Hand::Off);
            }
        }
        if cmd.reload_main {
            self.start_reload(Hand::Main);
        }
        if cmd.reload_off {
            self.start_reload(Hand::Off);
        }
        for hand in Hand::BOTH {
            let input = cmd.hand(hand);
            match stance.slot(hand) {
                Slot::Empty => {}
                Slot::Grapple => {
                    out.grapple_trigger |= input.pressed;
                    out.reel_in |= input.alt;
                    out.pay_out |= input.alt2;
                }
                Slot::Gun(_) => {
                    let Some(i) = self.weapon_slot(hand) else { continue };
                    let gun = self.inventory.weapons[i].expect("a gun");
                    let stats = gun.stats();
                    let h = hand.index();
                    // Reloading: count down, unless firing cuts it short.
                    if let Some(left) = self.hands[h].reloading {
                        if input.held && gun.mag > 0 {
                            self.hands[h].reloading = None;
                        } else {
                            let left = left - dt;
                            if left <= 0.0 {
                                self.finish_reload(hand);
                                let other = hand.other();
                                if self.hands[other.index()].reload_queued {
                                    self.start_reload(other);
                                }
                            } else {
                                self.hands[h].reloading = Some(left);
                            }
                            continue;
                        }
                    }
                    if self.hands[h].reload_queued {
                        if input.held && gun.mag > 0 {
                            self.hands[h].reload_queued = false;
                        } else {
                            continue;
                        }
                    }
                    if !input.held {
                        self.hands[h].cooldown = (self.hands[h].cooldown - dt).max(0.0);
                        continue;
                    }
                    while self.hands[h].cooldown <= 1e-5 {
                        let mag = self.gun_mut(i).mag;
                        if mag == 0 {
                            self.start_reload(hand);
                            break;
                        }
                        let last = mag == 1;
                        // Every fourth shot of this hand's, but never the
                        // last round (which stays the last word).
                        let free = stats.has(Perk::Recycler) && self.hands[h].fired % 4 == 3 && !last;
                        if !free {
                            self.gun_mut(i).mag -= 1;
                        }
                        self.hands[h].fired = self.hands[h].fired.wrapping_add(1);
                        self.hands[h].cooldown += 1.0 / stats.rate;
                        let cone = if self.braced(hand) {
                            stats.spread_braced
                        } else {
                            stats.spread
                        };
                        let damage = if last && stats.has(Perk::LastWord) {
                            stats.damage * 3.0
                        } else {
                            stats.damage
                        };
                        out.shots.push(ShotOut {
                            hand,
                            gun: gun.frame,
                            damage,
                            muzzle_speed: stats.muzzle_speed,
                            bullet_mass: stats.bullet_mass,
                            deviation: spread(self.shots, cone.to_radians()),
                        });
                        self.shots = self.shots.wrapping_add(1);
                    }
                    // Counted down after firing, keeping up to a tick of remainder so
                    // rates that aren't a whole number of ticks average out.
                    self.hands[h].cooldown = (self.hands[h].cooldown - dt).max(-dt);
                }
            }
        }
        out
    }
}

/// A point in a cone of half-angle `cone` (radians) as tangent offsets,
/// picked by hashing the shot number: the same on every peer.
fn spread(shot: u32, cone: f32) -> Vec2 {
    let hash = |x: u32| {
        let mut h = x.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
        h ^= h >> 15;
        h = h.wrapping_mul(0x2C1B_3C6D);
        h ^= h >> 12;
        (h & 0xFFFF) as f32 / 65535.0
    };
    // Uniform over the disc.
    let r = hash(shot.wrapping_mul(2)).sqrt() * cone.tan();
    let a = hash(shot.wrapping_mul(2).wrapping_add(1)) * std::f32::consts::TAU;
    Vec2::new(r * a.cos(), r * a.sin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::input::HandInput;
    use crate::sim::items::{CargoKind, Rarity};

    const DT: f32 = 1.0 / 60.0;

    fn holding(main: bool) -> TickInput {
        let mut cmd = TickInput::default();
        cmd.main = HandInput {
            held: main,
            pressed: false,
            alt: false,
            alt2: false,
        };
        cmd
    }

    fn mag(l: &Loadout, slot: usize) -> u16 {
        l.inventory.weapons[slot].unwrap().mag
    }

    #[test]
    fn a_held_trigger_fires_at_the_guns_rate() {
        let mut l = Loadout::default();
        let shots: usize = (0..60).map(|_| l.tick(&holding(true), DT).shots.len()).sum();
        assert_eq!(shots, 15, "SMG fires 15 a second");
        assert_eq!(mag(&l, 0), 40 - 15);
    }

    #[test]
    fn a_dry_gun_reloads_itself_from_the_pool_and_firing_resumes() {
        let mut l = Loadout::default();
        l.inventory.weapons[0].as_mut().unwrap().mag = 1;
        let fired = |l: &mut Loadout, ticks| -> usize {
            (0..ticks).map(|_| l.tick(&holding(true), DT).shots.len()).sum()
        };
        assert_eq!(fired(&mut l, 30), 1);
        assert!(l.hands[0].reloading.is_some(), "didn't start reloading");
        // A 1.6 s reload, then it fires again.
        let more = fired(&mut l, 120);
        assert!(more > 0, "never resumed");
        assert_eq!(l.inventory.ammo(AmmoKind::Light), AmmoKind::Light.cap() - 40);
    }

    #[test]
    fn a_reload_takes_what_the_pool_has() {
        let mut l = Loadout::default();
        l.inventory.weapons[0].as_mut().unwrap().mag = 0;
        *l.inventory.ammo_mut(AmmoKind::Light) = 7;
        let mut cmd = TickInput::default();
        cmd.reload = true;
        l.tick(&cmd, DT);
        cmd.reload = false;
        for _ in 0..120 {
            l.tick(&cmd, DT);
        }
        assert_eq!(mag(&l, 0), 7);
        assert_eq!(l.inventory.ammo(AmmoKind::Light), 0);
        // An empty pool: no reload.
        l.inventory.weapons[0].as_mut().unwrap().mag = 0;
        cmd.reload = true;
        l.tick(&cmd, DT);
        assert!(l.hands[0].reloading.is_none());
    }

    #[test]
    fn switching_stance_takes_a_moment_and_cancels_reloads() {
        let mut l = Loadout::default();
        l.inventory.weapons[0].as_mut().unwrap().mag = 0;
        let mut cmd = TickInput::default();
        cmd.reload = true;
        l.tick(&cmd, DT);
        assert!(l.hands[0].reloading.is_some());
        cmd.reload = false;
        cmd.stance = 2;
        cmd.stance_pressed = true;
        cmd.main.held = true;
        let out = l.tick(&cmd, DT);
        cmd.stance_pressed = false;
        assert!(l.hands[0].reloading.is_none());
        assert!(out.shots.is_empty(), "fired mid-switch");
        let shots: usize = (0..60).map(|_| l.tick(&cmd, DT).shots.len()).sum();
        assert!(shots > 0, "rifle never fired after the switch");
    }

    #[test]
    fn reloading_everything_goes_main_hand_first() {
        let mut l = Loadout::default();
        // Both hands with guns: SMG main, rifle off.
        l.stances[0] = Stance {
            main: Slot::Gun(0),
            off: Slot::Gun(1),
        };
        l.inventory.weapons[0].as_mut().unwrap().mag = 0;
        l.inventory.weapons[1].as_mut().unwrap().mag = 0;
        let mut cmd = TickInput::default();
        cmd.reload = true;
        l.tick(&cmd, DT);
        assert!(l.hands[0].reloading.is_some());
        assert!(l.hands[1].reloading.is_none() && l.hands[1].reload_queued);
        cmd.reload = false;
        for _ in 0..100 {
            l.tick(&cmd, DT);
        }
        assert_eq!(mag(&l, 0), 40);
        assert!(l.hands[1].reloading.is_some(), "off hand never started");
    }

    #[test]
    fn bracing_tightens_the_cone() {
        let wide = (0..200).map(|s| spread(s, 2.5f32.to_radians()).length()).fold(0.0, f32::max);
        let tight = (0..200).map(|s| spread(s, 0.35f32.to_radians()).length()).fold(0.0, f32::max);
        assert!(tight < wide / 5.0);
        assert!(wide <= 2.5f32.to_radians().tan() + 1e-6);
    }

    #[test]
    fn the_base_kit_has_no_guns_until_restocked() {
        let mut l = Loadout::base();
        assert_eq!(l.inventory, Inventory::default());
        assert!(Hand::BOTH.iter().all(|h| l.weapon(*h).is_none()));
        // Rifle and grapple: the rifle's slot is empty.
        let mut cmd = holding(true);
        cmd.stance = 1;
        cmd.stance_pressed = true;
        let shots: usize = (0..60).map(|_| l.tick(&cmd, DT).shots.len()).sum();
        assert_eq!(shots, 0);
        l.restock();
        let shots: usize = (0..60).map(|_| l.tick(&cmd, DT).shots.len()).sum();
        assert!(shots > 0);
    }

    #[test]
    fn the_locker_issues_only_what_you_lack_and_fills_the_pools() {
        let mut l = Loadout::base();
        // A looted rifle in the SMG's usual slot.
        let rifle = Weapon::new(GunKind::Rifle, 77, Rarity::Rare);
        l.inventory.weapons[0] = Some(Weapon { mag: 3, ..rifle });
        l.restock();
        let inv = &l.inventory;
        assert!(inv.weapons[0].unwrap().same_gun(&rifle), "kept the looted rifle");
        assert_eq!(inv.weapons[0].unwrap().mag, rifle.stats().mag, "its mag topped up");
        assert_eq!(inv.weapons[1], Some(Weapon::issue(GunKind::Smg)), "an SMG in the next free slot");
        assert_eq!(inv.weapons[2], None, "no second rifle");
        assert_eq!(inv.ammo, AmmoKind::ALL.map(AmmoKind::cap));
        // Again: nothing more.
        let before = l.inventory;
        l.restock();
        assert_eq!(l.inventory, before);
    }

    #[test]
    fn players_choose_what_each_stance_holds() {
        let mut l = Loadout::default();
        let mut cmd = TickInput::default();
        let mut wanted = DEFAULT_STANCES;
        // Rifle in the main hand of stance 1, and an SMG in both hands of
        // stance 3 (which can't be: the off hand comes up empty).
        wanted[0].main = Slot::Gun(1);
        wanted[2] = Stance { main: Slot::Gun(0), off: Slot::Gun(0) };
        wanted[3].off = Slot::Gun(7);
        cmd.stances = Some(wanted);
        cmd.main.held = true;
        let out = l.tick(&cmd, DT);
        assert!(out.shots.is_empty(), "changing what's in hand takes a switch");
        assert_eq!(l.stances[0].main, Slot::Gun(1));
        assert_eq!(l.stances[2], Stance { main: Slot::Gun(0), off: Slot::Empty });
        assert_eq!(l.stances[3].off, Slot::Empty, "no such gun slot");
        let shots: usize = (0..60).map(|_| l.tick(&cmd, DT).shots.len()).sum();
        assert_eq!(mag(&l, 1), 30 - shots as u16, "the rifle fired");
        // Changing only another stance doesn't interrupt this one.
        wanted[1].off = Slot::Empty;
        cmd.stances = Some(wanted);
        l.tick(&cmd, DT);
        assert_eq!(l.stances[1].off, Slot::Empty);
        assert_eq!(l.switching, 0.0);
    }

    #[test]
    fn the_grapple_hand_works_the_rope() {
        let mut l = Loadout::default();
        let mut cmd = TickInput::default();
        cmd.off.pressed = true;
        cmd.off.alt = true;
        let out = l.tick(&cmd, DT);
        assert!(out.grapple_held && out.grapple_trigger && out.reel_in);
        // Stance 3 has no grapple.
        cmd.stance = 2;
        cmd.stance_pressed = true;
        let out = l.tick(&cmd, DT);
        assert!(!out.grapple_held && !out.grapple_trigger);
    }

    #[test]
    fn bulky_cargo_takes_both_hands() {
        let mut l = Loadout::default();
        let before = l.held();
        l.inventory.carrying = Some(CargoKind::PowerCell);
        l.hands_changed(before);
        let mut cmd = holding(true);
        cmd.off.pressed = true;
        for _ in 0..60 {
            let out = l.tick(&cmd, DT);
            assert!(out.shots.is_empty() && !out.grapple_held && !out.grapple_trigger);
        }
        // Put down, the guns come back up after a switch.
        let before = l.held();
        l.inventory.carrying = None;
        l.hands_changed(before);
        assert!(l.switching > 0.0);
        let shots: usize = (0..60).map(|_| l.tick(&cmd, DT).shots.len()).sum();
        assert!(shots > 0);
    }

    #[test]
    fn a_new_gun_in_hand_is_brought_up() {
        let mut l = Loadout::default();
        let before = l.held();
        l.inventory.weapons[0] = Some(Weapon::new(GunKind::Smg, 5, Rarity::Common));
        l.hands_changed(before);
        assert!(l.switching > 0.0, "swapped the gun in hand without a switch");
        // A full twin of the empty gun in hand is a switch too, not a free
        // reload.
        l.switching = 0.0;
        l.inventory.weapons[0] = Some(Weapon { mag: 0, ..Weapon::issue(GunKind::Smg) });
        let before = l.held();
        l.inventory.weapons[0] = Some(Weapon::issue(GunKind::Smg));
        l.hands_changed(before);
        assert!(l.switching > 0.0);
        // The locker topping up the mag in hand isn't.
        l.switching = 0.0;
        l.inventory.weapons[0].as_mut().unwrap().mag = 2;
        l.restock();
        assert_eq!(l.switching, 0.0);
        assert_eq!(l.inventory.weapons[0].unwrap().mag, 40);
    }

    #[test]
    fn a_hand_queued_for_a_reload_the_other_emptied_the_pool_for_gives_up() {
        let mut l = Loadout::default();
        l.stances[0] = Stance { main: Slot::Gun(0), off: Slot::Gun(2) };
        l.inventory.weapons[2] = Some(Weapon { mag: 0, ..Weapon::issue(GunKind::Smg) });
        l.inventory.weapons[0].as_mut().unwrap().mag = 0;
        *l.inventory.ammo_mut(AmmoKind::Light) = 30;
        let mut cmd = TickInput::default();
        cmd.reload = true;
        l.tick(&cmd, DT);
        assert!(l.hands[1].reload_queued);
        cmd.reload = false;
        for _ in 0..120 {
            l.tick(&cmd, DT);
        }
        assert_eq!(mag(&l, 0), 30);
        assert!(!l.hands[1].reload_queued && l.hands[1].reloading.is_none(), "{:?}", l.hands[1]);
        // Ammo picked up: the dry off hand reloads itself when fired.
        *l.inventory.ammo_mut(AmmoKind::Light) = 100;
        cmd.off.held = true;
        let shots: usize = (0..150).map(|_| l.tick(&cmd, DT).shots.iter().filter(|s| s.hand == Hand::Off).count()).sum();
        assert!(shots > 0, "the off hand never came back");
    }

    #[test]
    fn a_recycler_saves_every_fourth_shot_of_its_own_hand() {
        let mut l = Loadout::default();
        let seed = (0..10_000)
            .find(|s| {
                let stats = Weapon::new(GunKind::Smg, *s, Rarity::Rare).stats();
                stats.has(Perk::Recycler) && stats.mag >= 40
            })
            .expect("no recycler SMG");
        let gun = Weapon::new(GunKind::Smg, seed, Rarity::Rare);
        l.inventory.weapons[0] = Some(gun);
        // Dual-wielded with another gun firing alongside.
        l.stances[0] = Stance { main: Slot::Gun(0), off: Slot::Gun(1) };
        let mut cmd = holding(true);
        cmd.off.held = true;
        let mut fired = 0;
        while fired < 40 {
            fired += l.tick(&cmd, DT).shots.iter().filter(|s| s.hand == Hand::Main).count();
        }
        assert_eq!(gun.mag - mag(&l, 0), fired as u16 - (fired as u16) / 4, "fired {fired}");
    }
}
