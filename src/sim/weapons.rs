//! What players hold: two hands, stances, and guns.
//!
//! A stance is what each hand holds: the main hand is used with the primary
//! button, the off hand with the secondary. Switching stances takes a moment.
//! The grapple is a suit system, but its launcher takes a hand; in a stance
//! without it, a deployed rope stays as it is (no reeling, no letting go).
//!
//! A one-handed gun with the other hand free is braced: it shoots tighter.
//! A hand that runs dry reloads itself; a reload of everything goes main
//! hand first. Firing a hand cancels its reload if there's anything left in
//! the mag.
//!
//! All of this is part of the predicted player state, so firing and
//! reloading respond at once. Shots leave as [`super::events::PlayerEvent::Fired`]
//! entries; the authority flies them (see [`super::shots`]).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::input::TickInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GunKind {
    Smg,
    Rifle,
}

/// How a kind of gun behaves.
pub struct GunStats {
    pub name: &'static str,
    /// Shots per second while the button is held.
    pub rate: f32,
    /// Muzzle speed (m/s), on top of the shooter's own velocity.
    pub muzzle_speed: f32,
    pub damage: f32,
    /// Cone half-angle (degrees), braced and not.
    pub spread_braced: f32,
    pub spread: f32,
    pub mag: u16,
    /// Rounds carried besides the mag, at full.
    pub reserve: u16,
    /// Seconds to reload.
    pub reload: f32,
    /// Bullet mass (kg): its momentum is the shooter's recoil and what it
    /// imparts to what it hits.
    pub bullet_mass: f32,
}

impl GunKind {
    pub fn stats(self) -> &'static GunStats {
        match self {
            GunKind::Smg => &GunStats {
                name: "SMG",
                rate: 15.0,
                muzzle_speed: 380.0,
                damage: 9.0,
                spread_braced: 1.2,
                spread: 2.0,
                mag: 40,
                reserve: 200,
                reload: 1.6,
                bullet_mass: 0.008,
            },
            GunKind::Rifle => &GunStats {
                name: "Rifle",
                rate: 7.5,
                muzzle_speed: 800.0,
                damage: 24.0,
                spread_braced: 0.35,
                spread: 2.5,
                mag: 30,
                reserve: 150,
                reload: 2.0,
                bullet_mass: 0.012,
            },
        }
    }
}

/// A gun a player carries.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gun {
    pub kind: GunKind,
    pub mag: u16,
    pub reserve: u16,
}

impl Gun {
    pub fn full(kind: GunKind) -> Gun {
        let s = kind.stats();
        Gun {
            kind,
            mag: s.mag,
            reserve: s.reserve,
        }
    }
}

/// What a hand holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Slot {
    Empty,
    Grapple,
    /// One of the loadout's guns, by index.
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
}

pub const STANCES: usize = 4;
/// Seconds to change stance.
const SWITCH_SECONDS: f32 = 0.4;

/// A hand's state: time to its next shot, and a reload in progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HandState {
    pub cooldown: f32,
    /// Seconds left of a reload, if reloading.
    pub reloading: Option<f32>,
    /// Waiting for the other hand to finish reloading before starting.
    pub reload_queued: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Loadout {
    pub guns: Vec<Gun>,
    pub stances: [Stance; STANCES],
    pub current: u8,
    /// Seconds until the current stance is ready, after a switch.
    pub switching: f32,
    pub hands: [HandState; 2],
    /// Shots fired so far; seeds each shot's spread.
    pub shots: u32,
}

impl Default for Loadout {
    /// The starting kit: an SMG and a rifle.
    ///
    /// 1. SMG and grapple; 2. rifle and grapple; 3. rifle, braced;
    /// 4. grapple alone, on the primary button.
    fn default() -> Loadout {
        let (smg, rifle) = (Slot::Gun(0), Slot::Gun(1));
        let stance = |main, off| Stance { main, off };
        Loadout {
            guns: vec![Gun::full(GunKind::Smg), Gun::full(GunKind::Rifle)],
            stances: [
                stance(smg, Slot::Grapple),
                stance(rifle, Slot::Grapple),
                stance(rifle, Slot::Empty),
                stance(Slot::Grapple, Slot::Empty),
            ],
            current: 0,
            switching: 0.0,
            hands: [HandState::default(); 2],
            shots: 0,
        }
    }
}

/// One shot: which hand, what gun, and how far off the crosshair (a unit
/// offset in the view plane, scaled by the cone).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotOut {
    pub hand: Hand,
    pub gun: GunKind,
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

    pub fn gun(&self, hand: Hand) -> Option<&Gun> {
        match self.stance().slot(hand) {
            Slot::Gun(i) => self.guns.get(i as usize),
            _ => None,
        }
    }

    fn gun_index(&self, hand: Hand) -> Option<usize> {
        match self.stance().slot(hand) {
            Slot::Gun(i) if (i as usize) < self.guns.len() => Some(i as usize),
            _ => None,
        }
    }

    /// Whether a hand's gun is steadied by the other hand.
    pub fn braced(&self, hand: Hand) -> bool {
        self.stance().slot(hand.other()) == Slot::Empty
    }

    fn start_reload(&mut self, hand: Hand) -> bool {
        let Some(i) = self.gun_index(hand) else {
            return false;
        };
        let gun = self.guns[i];
        let state = &mut self.hands[hand.index()];
        if state.reloading.is_some() || gun.mag >= gun.kind.stats().mag || gun.reserve == 0 {
            return false;
        }
        state.reloading = Some(gun.kind.stats().reload);
        state.reload_queued = false;
        true
    }

    fn finish_reload(&mut self, hand: Hand) {
        if let Some(i) = self.gun_index(hand) {
            let gun = &mut self.guns[i];
            let take = (gun.kind.stats().mag - gun.mag).min(gun.reserve);
            gun.mag += take;
            gun.reserve -= take;
        }
        self.hands[hand.index()].reloading = None;
    }

    fn cancel_reloads(&mut self) {
        for state in &mut self.hands {
            state.reloading = None;
            state.reload_queued = false;
        }
    }

    /// Runs the hands for one tick of `dt` seconds.
    pub fn tick(&mut self, cmd: &TickInput, dt: f32) -> HandsOut {
        let mut out = HandsOut::default();
        let wanted = cmd.stance.min(STANCES as u8 - 1);
        if wanted != self.current {
            self.current = wanted;
            self.switching = SWITCH_SECONDS;
            self.cancel_reloads();
        }
        let stance = self.stance();
        out.grapple_held = Hand::BOTH.iter().any(|h| stance.slot(*h) == Slot::Grapple);
        if self.switching > 0.0 {
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
                if self.gun_index(off).is_some_and(|i| {
                    let g = self.guns[i];
                    g.mag < g.kind.stats().mag && g.reserve > 0
                }) && self.hands[off.index()].reloading.is_none()
                {
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
                    let Some(i) = self.gun_index(hand) else { continue };
                    let stats = self.guns[i].kind.stats();
                    let h = hand.index();
                    // Reloading: count down, unless firing cuts it short.
                    if let Some(left) = self.hands[h].reloading {
                        if input.held && self.guns[i].mag > 0 {
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
                        if input.held && self.guns[i].mag > 0 {
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
                        if self.guns[i].mag == 0 {
                            self.start_reload(hand);
                            break;
                        }
                        self.guns[i].mag -= 1;
                        self.hands[h].cooldown += 1.0 / stats.rate;
                        let cone = if self.braced(hand) {
                            stats.spread_braced
                        } else {
                            stats.spread
                        };
                        out.shots.push(ShotOut {
                            hand,
                            gun: self.guns[i].kind,
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

    #[test]
    fn a_held_trigger_fires_at_the_guns_rate() {
        let mut l = Loadout::default();
        let shots: usize = (0..60).map(|_| l.tick(&holding(true), DT).shots.len()).sum();
        assert_eq!(shots, 15, "SMG fires 15 a second");
        assert_eq!(l.guns[0].mag, 40 - 15);
    }

    #[test]
    fn a_dry_gun_reloads_itself_and_firing_resumes() {
        let mut l = Loadout::default();
        l.guns[0].mag = 1;
        let fired = |l: &mut Loadout, ticks| -> usize {
            (0..ticks).map(|_| l.tick(&holding(true), DT).shots.len()).sum()
        };
        assert_eq!(fired(&mut l, 30), 1);
        assert!(l.hands[0].reloading.is_some(), "didn't start reloading");
        // A 1.6 s reload, then it fires again.
        let more = fired(&mut l, 120);
        assert!(more > 0, "never resumed");
        assert_eq!(l.guns[0].reserve, 200 - 40);
    }

    #[test]
    fn switching_stance_takes_a_moment_and_cancels_reloads() {
        let mut l = Loadout::default();
        l.guns[0].mag = 0;
        let mut cmd = TickInput::default();
        cmd.reload = true;
        l.tick(&cmd, DT);
        assert!(l.hands[0].reloading.is_some());
        cmd.reload = false;
        cmd.stance = 2;
        cmd.main.held = true;
        let out = l.tick(&cmd, DT);
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
        l.guns[0].mag = 0;
        l.guns[1].mag = 0;
        let mut cmd = TickInput::default();
        cmd.reload = true;
        l.tick(&cmd, DT);
        assert!(l.hands[0].reloading.is_some());
        assert!(l.hands[1].reloading.is_none() && l.hands[1].reload_queued);
        cmd.reload = false;
        for _ in 0..100 {
            l.tick(&cmd, DT);
        }
        assert_eq!(l.guns[0].mag, 40);
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
    fn the_grapple_hand_works_the_rope() {
        let mut l = Loadout::default();
        let mut cmd = TickInput::default();
        cmd.off.pressed = true;
        cmd.off.alt = true;
        let out = l.tick(&cmd, DT);
        assert!(out.grapple_held && out.grapple_trigger && out.reel_in);
        // Stance 3 has no grapple.
        cmd.stance = 2;
        let out = l.tick(&cmd, DT);
        assert!(!out.grapple_held && !out.grapple_trigger);
    }
}
