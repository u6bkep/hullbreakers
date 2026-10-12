//! The limpet's brain: a rock mimic of the asteroid field (see
//! [`crate::sim::fauna`] for the field's other life).
//!
//! A limpet clings to a rock, still, its shell plates shut: a lump of rock
//! with a thin magenta crack. It knows of the crew by what it feels through
//! its rock (anyone standing on it, wherever they are on it) and by what
//! comes close enough to see. Then it turns its snout on them, creeping over
//! the rock if they're on it, and shoots a tendril into them.
//!
//! What the tendril does is the rope's business (see `sim::player`): the
//! limpet's boots hold its rock and its winch reels, so someone adrift is
//! hauled in to the rock, while someone standing fast on something is flown
//! at: the limpet lets go of its rock and reels itself in to them. Either
//! way it ends up on them, and stays: it lets go of its rock and holds
//! itself against them on its gas sac, its winch taking up the slack (so
//! whoever it's on hauls its mass about), and bites. Its winch
//! won't hold against more than its own pull, though: a crewmate who
//! hooks it and reels tears it off. Torn off, or done with someone, it
//! makes for the nearest rock on its tendril and clings again.

use avian3d::prelude::*;
use bevy::prelude::*;

use super::super::bodies::Bodies;
use super::super::input::PlayerInput;
use super::super::player::{Mode, Player};
use super::super::tuning::Tuning;
use super::super::Layer;
use super::{body_rotation, look_toward, to_input};

/// What a limpet is doing, and since when (s).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Limpet {
    phase: Phase,
    timer: f32,
    /// Who it's after.
    prey: Option<Entity>,
    /// Seconds until it feels and looks about again.
    sense: f32,
    /// Seconds latched since its last bite; whether a bite is due (taken by
    /// [`super::limpet_bites`]).
    since_bite: f32,
    pub(super) bite: bool,
    /// Seconds its winch has been pulled past what it holds.
    strain: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    /// Clinging to its rock, still.
    #[default]
    Lurk,
    /// It knows of someone: turning on them, creeping closer over its rock.
    Stalk,
    /// Its tendril just went out at them.
    Strike,
    /// Its tendril is in them: reeling.
    Reel,
    /// On them, biting.
    Latched,
    /// Off any rock, nobody to hunt: making for a rock to cling to.
    Adrift,
    /// After a miss, a moment before it tries again.
    Rest,
}

impl Limpet {
    fn enter(&mut self, phase: Phase) {
        self.phase = phase;
        self.timer = 0.0;
        self.strain = 0.0;
    }

    /// Whether it's on someone (its plates open, biting).
    pub(super) fn latched(&self) -> bool {
        self.phase == Phase::Latched
    }

    pub(super) fn prey(&self) -> Option<Entity> {
        self.prey
    }
}

/// One of the crew, as a limpet knows them this tick.
#[derive(Clone, Copy, Debug)]
pub(super) struct Prey {
    pub entity: Entity,
    pub position: Vec3,
    pub velocity: Vec3,
    /// The body they stand on.
    pub on: Option<Entity>,
}

/// What a limpet can know of the world.
pub(super) struct Surroundings<'a, 'w, 's> {
    pub crew: &'a [Prey],
    pub bodies: &'a Bodies<'w, 's>,
    pub spatial: &'a SpatialQuery<'w, 's>,
    /// Every rock, and its radius.
    pub rocks: &'a [(Entity, f32)],
}

/// It sees someone within this (m), given a clear line.
const SIGHT: f32 = 20.0;
/// Its tendril reaches this far (m); see its tuning's grapple range.
pub(super) const TENDRIL_REACH: f32 = 16.0;
/// How close to on its prey its snout has to be to shoot (rad).
const AIM_CONE: f32 = 0.09;
/// How fast it turns (rad/s).
const TURN_RATE: f32 = 2.5;
/// It creeps over its rock toward someone on it until this close (m).
const CREEP_UNTIL: f32 = 6.0;
/// Reeling: it's on them within this (m, centre to centre), and gives up
/// after this long (s).
const LATCH: f32 = 1.4;
const REEL_GIVE_UP: f32 = 6.0;
/// Latched, it holds itself this far from their middle (m), and lets go
/// when they're further than this (m) or its winch has been pulled past
/// its hold this long (s).
const HUG: f32 = 0.62;
const LOST_HOLD: f32 = 3.0;
const TEAR: f32 = 0.35;
/// It bites someone within this (m) of it, this often (s).
const BITE_RANGE: f32 = 1.2;
pub(super) const BITE_EVERY: f32 = 0.6;
/// Reeling itself to a rock, it closes the last of the way itself from
/// this far off its tendril's end (m): the winch stops short.
const LAND_FROM: f32 = 2.2;
/// After a miss, or torn off, it waits this long (s); without its prey for
/// this long (s) it settles back down.
const REST: f32 = 2.0;
const LOSE_INTEREST: f32 = 6.0;

/// The limpet's tick: writes its input from what it knows.
pub(super) fn think(me: &Player, tuning: &Tuning, l: &mut Limpet, home: &mut (Entity, Vec3), input: &mut PlayerInput, s: &Surroundings, dt: f32) {
    l.timer += dt;
    input.movement = Vec3::ZERO;
    input.main.held = false;
    input.reel_in = false;
    input.sprint = false;
    let frame = me.mode.body().and_then(|b| s.bodies.frame(b));
    let (eye, view) = me.mode.world_view(tuning, frame.as_ref());
    let body = body_rotation(&me.mode, frame.as_ref());
    let pitch = match me.mode {
        Mode::Booted { pitch, .. } | Mode::Eva { pitch, .. } => pitch,
    };
    let (position, velocity) = me.mode.world_kinematics(tuning, frame.as_ref());
    let on = me.mode.body();
    let hooked = me.grapple.map(|g| g.target);
    let step = TURN_RATE * dt;
    let look_at = |input: &mut PlayerInput, at: Vec3| {
        input.add_look(look_toward(body, pitch, at - eye).clamp(Vec2::splat(-step), Vec2::splat(step)));
        (view * Vec3::NEG_Z).angle_between(at - eye) < AIM_CONE
    };
    let clear_line = |to: Vec3, target: Option<Entity>| {
        let Ok(dir) = Dir3::new(to - eye) else { return true };
        let distance = eye.distance(to);
        s.spatial
            .cast_ray(eye, dir, distance, true, &SpatialQueryFilter::from_mask(Layer::World))
            .is_none_or(|hit| hit.distance > distance - 0.6 || s.bodies.body_of_collider(hit.entity) == target && target.is_some())
    };
    let boots = |input: &mut PlayerInput, want: bool| {
        if me.boots_on != want {
            input.presses.toggle_boots.press();
        }
    };
    let assist = |input: &mut PlayerInput, want: bool| {
        if me.flight_assist != want {
            input.presses.toggle_assist.press();
        }
    };
    let release = |input: &mut PlayerInput| {
        if hooked.is_some() {
            input.presses.main.press();
        }
    };

    // Feeling and looking about, when it isn't busy with someone.
    l.sense -= dt;
    if l.sense <= 0.0 && matches!(l.phase, Phase::Lurk | Phase::Stalk | Phase::Adrift | Phase::Rest) {
        l.sense = 0.25;
        // Through its rock it feels anyone standing on it; it sees what's
        // near.
        let felt = |p: &&Prey| on.is_some() && p.on == on;
        let seen = |p: &&Prey| p.position.distance(eye) < SIGHT && clear_line(p.position, None);
        let found = s
            .crew
            .iter()
            .filter(|p| felt(p) || seen(p))
            .min_by(|a, b| a.position.distance(eye).total_cmp(&b.position.distance(eye)));
        match found {
            Some(p) => {
                l.prey = Some(p.entity);
                if matches!(l.phase, Phase::Lurk | Phase::Adrift) && hooked.is_none() {
                    l.enter(Phase::Stalk);
                } else if l.phase == Phase::Stalk {
                    // Still on them: it loses interest only this long after
                    // it last knew where they were.
                    l.timer = 0.0;
                }
            }
            None if l.phase == Phase::Stalk && l.timer > LOSE_INTEREST => {
                l.prey = None;
                l.enter(if on.is_some() { Phase::Lurk } else { Phase::Adrift });
            }
            None => {}
        }
    }
    let prey = l.prey.and_then(|e| s.crew.iter().find(|p| p.entity == e)).copied();
    if prey.is_none() && matches!(l.phase, Phase::Stalk | Phase::Strike | Phase::Reel | Phase::Latched) {
        // Gone (dead, gone home).
        release(input);
        l.prey = None;
        l.enter(if on.is_some() { Phase::Lurk } else { Phase::Adrift });
    }
    if l.phase != Phase::Latched {
        l.since_bite = 0.0;
    }

    match (l.phase, prey) {
        (Phase::Lurk, _) => {
            boots(input, true);
            assist(input, true);
            match on {
                // Where it clings is its home.
                Some(rock) => {
                    if let (Mode::Booted { feet, .. }, true) = (me.mode, home.0 != rock) {
                        *home = (rock, feet);
                    }
                }
                None => l.enter(Phase::Adrift),
            }
        }
        (Phase::Stalk, Some(p)) => {
            boots(input, true);
            assist(input, true);
            let aimed = look_at(input, p.position);
            let distance = p.position.distance(position);
            // Over its rock to them, if they're on it, until it's near
            // enough with a clear line (round the rock's curve).
            let clear = clear_line(p.position, Some(p.entity));
            if let (Mode::Booted { up, orientation, .. }, Some(f)) = (me.mode, frame)
                && p.on == on
                && (distance > CREEP_UNTIL || !clear)
            {
                let up_w = f.rotation * up;
                let along = (p.position - position) - up_w * (p.position - position).dot(up_w);
                if let Some(dir) = along.try_normalize() {
                    let local = (f.rotation * orientation).inverse() * dir;
                    input.movement = Vec3::new(local.x, 0.0, -local.z).normalize_or_zero();
                }
            }
            if aimed && distance < TENDRIL_REACH && clear {
                input.presses.main.press();
                l.enter(Phase::Strike);
            }
        }
        (Phase::Strike, Some(p)) => {
            look_at(input, p.position);
            if hooked == Some(p.entity) {
                // Someone standing fast on something: it lets go of its
                // rock and flies at them on its tendril. Someone adrift is
                // hauled in to its rock.
                if p.on.is_some() {
                    boots(input, false);
                }
                l.enter(Phase::Reel);
            } else if l.timer > 0.3 {
                release(input);
                l.enter(Phase::Rest);
            }
        }
        (Phase::Reel, Some(p)) => {
            look_at(input, p.position);
            if hooked != Some(p.entity) {
                l.enter(if on.is_some() { Phase::Rest } else { Phase::Adrift });
            } else if position.distance(p.position) < LATCH {
                l.enter(Phase::Latched);
            } else if l.timer > REEL_GIVE_UP {
                release(input);
                l.enter(Phase::Rest);
            } else {
                input.reel_in = true;
            }
        }
        (Phase::Latched, Some(p)) => {
            look_at(input, p.position);
            // The winch takes up whatever slack there is, and holds.
            input.reel_in = true;
            let held = me.grapple.filter(|g| g.target == p.entity);
            l.strain = if held.is_some_and(|g| g.tension > tuning.reel_force * 0.95) { l.strain + dt } else { 0.0 };
            let distance = position.distance(p.position);
            if held.is_none() || l.strain > TEAR {
                // Torn off: stunned a moment (by when it's likely been
                // hauled to whoever tore it off).
                release(input);
                l.enter(Phase::Rest);
            } else if distance > LOST_HOLD {
                release(input);
                l.prey = None;
                l.enter(if on.is_some() { Phase::Lurk } else { Phase::Adrift });
            } else {
                // Off its rock and onto them, holding itself against them
                // on its gas sac (by raw thrust: the assist would brake it
                // against its rock).
                boots(input, false);
                assist(input, false);
                let toward = (p.position - position).normalize_or_zero();
                let closing = (velocity - p.velocity).dot(toward);
                let push = ((distance - HUG) * 4.0 - closing * 2.5).clamp(-1.0, 1.0);
                input.movement = to_input(view, toward) * push;
                if distance < BITE_RANGE {
                    l.since_bite += dt;
                    if l.since_bite >= BITE_EVERY {
                        l.since_bite -= BITE_EVERY;
                        l.bite = true;
                    }
                }
            }
        }
        (Phase::Rest, _) => {
            release(input);
            boots(input, true);
            assist(input, true);
            if l.timer > REST {
                l.enter(match (prey, on) {
                    (Some(_), _) => Phase::Stalk,
                    (None, Some(_)) => Phase::Lurk,
                    (None, None) => Phase::Adrift,
                });
            }
        }
        (Phase::Adrift, _) => {
            boots(input, true);
            assist(input, true);
            if let (Some(rock), Mode::Booted { feet, .. }) = (on, me.mode) {
                *home = (rock, feet);
                l.enter(Phase::Lurk);
                return;
            }
            let rock_hooked = me.grapple.filter(|g| s.rocks.iter().any(|(r, _)| *r == g.target));
            if let Some(g) = rock_hooked {
                // Reeling itself in to it; the winch stops short of the
                // rock, so the last of the way it puffs itself on, and its
                // foot takes hold.
                let anchor = s.bodies.frame(g.target).map_or(position, |f| f.point_to_world(g.anchor));
                if anchor.distance(position) < LAND_FROM {
                    input.movement = to_input(view, anchor - position) * 0.5;
                } else {
                    input.reel_in = true;
                }
                if l.timer > REEL_GIVE_UP * 2.0 {
                    release(input);
                    l.enter(Phase::Adrift);
                }
                return;
            }
            if hooked.is_some() {
                release(input);
            }
            // The nearest rock: at it on its tendril, if it's in reach, or
            // drifting toward it on its gas sac.
            let nearest = s
                .rocks
                .iter()
                .filter_map(|&(rock, radius)| {
                    let f = s.bodies.frame(rock)?;
                    Some((f.center_of_mass, f.center_of_mass.distance(position) - radius * 0.8))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((at, gap)) = nearest {
                let aimed = look_at(input, at);
                if gap < TENDRIL_REACH * 0.9 {
                    if aimed && l.timer > 0.5 {
                        input.presses.main.press();
                        l.timer = 0.0;
                    }
                } else {
                    input.movement = to_input(view, at - position);
                }
            }
        }
        // Its prey is gone: dealt with above.
        (_, None) => {}
    }
}
