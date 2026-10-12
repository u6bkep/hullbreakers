//! Affixes: variations on a kind of robot, rolled when its ship is crewed
//! (see [`super::crew`]). Each one changes how it fights, not only its
//! numbers, and each shows on its model (the client's business: see
//! `client::robot_fx`).
//!
//! - **Armoured**: plates take most of a body hit; only its weak spot is
//!   soft. Heavier and slower for it.
//! - **Shielded**: an energy shell takes everything, weak spot and all,
//!   until it's broken; left alone a moment, it comes back.
//! - **Overclocked**: fast, quick to turn and quick to fire, but every
//!   second on the trigger heats it, and at full heat it stops to vent: it
//!   can't move or shoot, and with its vents open every hit is a weak-spot
//!   hit.
//! - **Volatile**: it goes up when it dies, hurting and shoving everything
//!   near it, robots too (see [`super::blast`]).
//! - **Marksman** (Gunner only): a long rifle, and a slow aimed shot with a
//!   visible wind-up; it holds still for the last moment, so a target that
//!   moves then is missed.
//! - **Leader** (Gunner only): robots near it fight better while it lives
//!   (steadier aim, quicker to turn and to fire, calls that carry further).
//!
//! The authority keeps each robot's affix state in its [`Brain`]; the
//! replicated [`Condition`] carries what presentation needs of it, coarsely
//! so it rarely changes.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{Brain, BodyTuning, Mind, Robot, tuning_of};
use crate::sim::bodies::Bodies;
use crate::sim::input::PlayerInput;
use crate::sim::player::Player;
use crate::sim::tuning::Tuning;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Affix {
    Armoured,
    Shielded,
    Overclocked,
    Volatile,
    Marksman,
    Leader,
}

impl Affix {
    pub const ALL: [Affix; 6] = [
        Affix::Armoured,
        Affix::Shielded,
        Affix::Overclocked,
        Affix::Volatile,
        Affix::Marksman,
        Affix::Leader,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Affix::Armoured => "Armoured",
            Affix::Shielded => "Shielded",
            Affix::Overclocked => "Overclocked",
            Affix::Volatile => "Volatile",
            Affix::Marksman => "Marksman",
            Affix::Leader => "Leader",
        }
    }

    /// Its parts on a model are named `affix_<tag>_...` (see the robots'
    /// `build.py`).
    pub fn tag(self) -> &'static str {
        match self {
            Affix::Armoured => "armour",
            Affix::Shielded => "shield",
            Affix::Overclocked => "overclock",
            Affix::Volatile => "volatile",
            Affix::Marksman => "marksman",
            Affix::Leader => "leader",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }

    /// Whether a kind of robot can have this.
    pub fn fits(self, kind: Robot) -> bool {
        // Creatures are as they are.
        if kind.fauna() {
            return false;
        }
        match self {
            Affix::Armoured => kind != Robot::Swarmer,
            Affix::Shielded => true,
            Affix::Overclocked | Affix::Volatile => kind != Robot::Turret,
            Affix::Marksman | Affix::Leader => kind == Robot::Gunner,
        }
    }

    /// Whether the two can't be had together.
    pub fn clashes(self, other: Affix) -> bool {
        use Affix::*;
        // Patient and frantic; a leader keeps itself out of the fight.
        matches!((self, other), (Marksman, Overclocked) | (Overclocked, Marksman) | (Marksman, Leader) | (Leader, Marksman))
    }
}

/// A robot's affixes, as a set. Replicated once, with its kind.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Affixes(u8);

impl Affixes {
    pub const NONE: Affixes = Affixes(0);

    pub fn of(affixes: &[Affix]) -> Affixes {
        Affixes(affixes.iter().fold(0, |bits, a| bits | a.bit()))
    }

    pub fn has(self, affix: Affix) -> bool {
        self.0 & affix.bit() != 0
    }

    pub fn with(self, affix: Affix) -> Affixes {
        Affixes(self.0 | affix.bit())
    }

    pub fn iter(self) -> impl Iterator<Item = Affix> {
        Affix::ALL.into_iter().filter(move |a| self.has(*a))
    }

    pub fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether `affix` could be added to these on a `kind`.
    pub fn takes(self, affix: Affix, kind: Robot) -> bool {
        affix.fits(kind) && !self.has(affix) && !self.iter().any(|a| a.clashes(affix))
    }

    /// "Armoured, Volatile Gunner".
    pub fn describe(self, kind: Robot) -> String {
        let mut words: Vec<&str> = self.iter().map(Affix::name).collect();
        words.push(kind.name());
        words.join(" ")
    }
}

/// What a robot's affixes are doing, as presentation needs it: replicated,
/// and kept coarse so it changes rarely.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// The shell's charge, in sixteenths of full (Shielded); 0 is broken.
    pub shell: u8,
    /// Counts the hits the shell has taken (wrapping): a change is a flash.
    pub shell_hits: u8,
    /// Heat in sixteenths (Overclocked), and whether it's venting.
    pub heat: u8,
    pub venting: bool,
    /// A telegraphed attack winding up (the marksman's aim, the grappler's
    /// throw): sixteenths of the way to it, 0 when there's none.
    pub winding: u8,
    /// The marksman's aim has stopped moving: the shot is coming.
    pub locked: bool,
    /// Within reach of a living leader of its own side.
    pub rallied: bool,
}

impl Condition {
    pub fn of(state: &State, affixes: Affixes) -> Condition {
        let mut c = Condition::default();
        c.update(state, affixes);
        c
    }

    fn update(&mut self, state: &State, affixes: Affixes) {
        self.shell = if state.shell_max > 0.0 {
            (state.shell / state.shell_max * 16.0).ceil().clamp(0.0, 16.0) as u8
        } else {
            0
        };
        self.heat = if affixes.has(Affix::Overclocked) {
            (state.heat * 16.0).floor().clamp(0.0, 16.0) as u8
        } else {
            0
        };
        self.venting = state.venting();
    }

    pub fn shell_fraction(&self) -> f32 {
        self.shell as f32 / 16.0
    }

    pub fn heat_fraction(&self) -> f32 {
        self.heat as f32 / 16.0
    }

    pub fn winding_fraction(&self) -> f32 {
        self.winding as f32 / 16.0
    }
}

/// The authority's exact state of a robot's affixes (kept in its
/// [`Brain`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct State {
    /// The shell's charge and full charge (hit points).
    pub shell: f32,
    pub shell_max: f32,
    /// Seconds since anything hit it.
    pub since_hit: f32,
    /// Heat (1 is full), and seconds of venting left.
    pub heat: f32,
    pub vent: f32,
    /// Who hit it last: a volatile robot's blast is theirs.
    pub last_hit_by: Option<Entity>,
}

impl State {
    pub fn fresh(affixes: Affixes, tuning: &Tuning) -> State {
        let shell_max = if affixes.has(Affix::Shielded) {
            SHELL_FRACTION * tuning.player_health
        } else {
            0.0
        };
        State {
            shell: shell_max,
            shell_max,
            since_hit: f32::INFINITY,
            ..default()
        }
    }

    pub fn venting(&self) -> bool {
        self.vent > 0.0
    }
}

/// An armoured robot's plates let this much of a body hit through.
pub const ARMOUR_BODY: f32 = 0.3;
/// The shell holds this much of the robot's health.
pub const SHELL_FRACTION: f32 = 0.6;
/// The shell starts coming back this long after the last hit (s), and
/// takes this long to come back from nothing (s).
pub const SHELL_DELAY: f32 = 3.0;
pub const SHELL_REGEN_SECONDS: f32 = 2.0;
/// Overclocked: seconds on the trigger to overheat, seconds off it to cool
/// from full, and seconds it vents for.
pub const HEAT_SECONDS: f32 = 3.0;
pub const COOL_SECONDS: f32 = 8.0;
pub const VENT_SECONDS: f32 = 3.0;
/// A leader rallies robots within this (m).
pub const RALLY_RADIUS: f32 = 16.0;

/// A kind's tuning, changed by its affixes.
pub(super) fn apply_tuning(t: &mut Tuning, affixes: Affixes) {
    if affixes.has(Affix::Armoured) {
        t.player_mass *= 1.35;
        t.walk_speed *= 0.85;
        t.sprint_speed *= 0.85;
        t.thrust_accel *= 0.8;
    }
    if affixes.has(Affix::Overclocked) {
        t.walk_speed *= 1.6;
        t.sprint_speed *= 1.6;
        t.walk_accel *= 1.5;
        t.thrust_accel *= 1.5;
        t.assist_max_speed *= 1.4;
    }
    if affixes.has(Affix::Volatile) {
        t.player_health *= 0.8;
    }
    if affixes.has(Affix::Leader) {
        t.player_health *= 1.4;
    }
}

/// A kind's mind, changed by its affixes and by a leader nearby.
pub(super) fn mind(mut m: Mind, kind: Robot, affixes: Affixes, rallied: bool) -> Mind {
    if affixes.has(Affix::Overclocked) {
        m.turn_rate *= 1.7;
        m.burst_off *= 0.4;
        m.aim_error *= 1.3;
    }
    if affixes.has(Affix::Marksman) && kind == Robot::Gunner {
        m.sight = 150.0;
        m.turn_rate = 1.4;
        m.aim_error = 0.003;
        m.fire_cone = 0.03;
        m.range = (20.0, 80.0);
        m.lead = true;
    }
    if affixes.has(Affix::Leader) {
        m.calls *= 3.0;
        // It hangs back and directs.
        m.range = (m.range.0 + 6.0, m.range.1 + 10.0);
    }
    if rallied {
        m.aim_error *= 0.5;
        m.turn_rate *= 1.3;
        m.burst_off *= 0.6;
        m.calls *= 2.0;
    }
    m
}

/// Authority, as a hit lands (see `vitals::apply_damage`): how much of it
/// reaches the robot's health, and whether it counts as a weak-spot hit.
/// A shell takes it all while it lasts; a venting robot is all weak spot;
/// armour turns most of a body hit. (`condition` is only touched, and so
/// only re-sent, when the shell takes the hit.)
pub fn absorb(affixes: Affixes, brain: &mut Brain, condition: &mut Mut<Condition>, source: Option<Entity>, amount: f32, weak: bool) -> (f32, bool) {
    let state = &mut brain.state;
    state.since_hit = 0.0;
    if source.is_some() {
        state.last_hit_by = source;
    }
    if state.shell > 0.0 {
        // What breaks the shell is spent on it.
        state.shell = (state.shell - amount).max(0.0);
        condition.shell_hits = condition.shell_hits.wrapping_add(1);
        condition.update(state, affixes);
        return (0.0, false);
    }
    let (mut amount, mut weak) = (amount, weak);
    if state.venting() && !weak {
        amount *= super::WEAK_MULTIPLIER;
        weak = true;
    }
    if affixes.has(Affix::Armoured) && !weak {
        amount *= ARMOUR_BODY;
    }
    (amount, weak)
}

/// Authority, before robots think: shells come back, overclocked robots
/// heat and vent, and robots near a living leader are rallied.
pub fn robot_upkeep(
    time: Res<Time>,
    base: Res<Tuning>,
    bodies: Bodies,
    mut robots: Query<(Entity, &Robot, &Affixes, &Player, &PlayerInput, &mut Brain, &mut Condition, Option<&BodyTuning>)>,
) {
    let dt = time.delta_secs();
    let position = |player: &Player, own: Option<&BodyTuning>| {
        let frame = player.mode.body().and_then(|b| bodies.frame(b));
        player.mode.world_kinematics(tuning_of(&base, own), frame.as_ref()).0
    };
    let leaders: Vec<(Entity, Entity, Vec3)> = robots
        .iter()
        .filter(|(_, _, affixes, ..)| affixes.has(Affix::Leader))
        .map(|(e, _, _, player, _, brain, _, own)| (e, brain.home, position(player, own)))
        .collect();
    for (entity, _, affixes, player, input, mut brain, mut condition, own) in &mut robots {
        let at = position(player, own);
        let home = brain.home;
        // A grappler heats as it throws and reels, as a gunner does firing.
        let brain_attack = brain.attack;
        let state = &mut brain.state;
        state.since_hit += dt;
        if state.shell_max > 0.0 && state.since_hit > SHELL_DELAY {
            state.shell = (state.shell + state.shell_max / SHELL_REGEN_SECONDS * dt).min(state.shell_max);
        }
        if affixes.has(Affix::Overclocked) {
            if state.venting() {
                state.vent = (state.vent - dt).max(0.0);
                state.heat = state.vent / VENT_SECONDS;
            } else if input.main.held || matches!(brain_attack, super::Attack::Winding(_) | super::Attack::Reeling(_)) {
                state.heat += dt / HEAT_SECONDS;
                if state.heat >= 1.0 {
                    state.heat = 1.0;
                    state.vent = VENT_SECONDS;
                }
            } else {
                state.heat = (state.heat - dt / COOL_SECONDS).max(0.0);
            }
        }
        let mut next = *condition;
        next.update(state, *affixes);
        next.rallied = leaders.iter().any(|(leader, leader_home, p)| *leader != entity && *leader_home == home && p.distance(at) < RALLY_RADIUS);
        next.winding = match brain.attack {
            super::Attack::Winding(t) => {
                let full = if affixes.has(Affix::Marksman) { super::MARKSMAN_AIM } else { super::GRAPPLE_WIND_UP };
                ((t / full * 16.0).floor() as u8).clamp(1, 16)
            }
            _ => 0,
        };
        next.locked = affixes.has(Affix::Marksman) && matches!(brain.attack, super::Attack::Winding(t) if t > super::MARKSMAN_AIM - super::MARKSMAN_LOCK);
        condition.set_if_neq(next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affixes_are_a_set_and_respect_kinds_and_clashes() {
        let a = Affixes::of(&[Affix::Armoured, Affix::Volatile]);
        assert!(a.has(Affix::Armoured) && a.has(Affix::Volatile) && !a.has(Affix::Shielded));
        assert_eq!(a.len(), 2);
        assert_eq!(a.iter().collect::<Vec<_>>(), vec![Affix::Armoured, Affix::Volatile]);
        assert!(!Affixes::NONE.takes(Affix::Armoured, Robot::Swarmer));
        assert!(!Affixes::NONE.takes(Affix::Marksman, Robot::Grappler));
        assert!(!Affixes::of(&[Affix::Overclocked]).takes(Affix::Marksman, Robot::Gunner));
        assert!(Affixes::of(&[Affix::Overclocked]).takes(Affix::Volatile, Robot::Swarmer));
        assert_eq!(a.describe(Robot::Gunner), "Armoured Volatile Gunner");
    }
}
